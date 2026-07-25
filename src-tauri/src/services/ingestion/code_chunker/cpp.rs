//! C++ chunker. Direct port of `chunk_code_cpp.py`.

use std::path::Path;

use tree_sitter::Node;

use super::helpers::{generate_chunk_id, make_remainder_chunk, node_text};
use crate::services::ingestion::types::{CodeChunk, CodeChunkKind, CppFields, Parameter};

const FUNCTION_NODE_TYPES: &[&str] = &[
    "function_definition",
    "method_definition",
    "constructor_definition",
    "destructor_definition",
    "operator_cast_definition",
];
const TYPE_NODE_TYPES: &[&str] = &["class_specifier", "struct_specifier"];

pub fn extract_dependencies<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    super::c::extract_dependencies_same(root, source)
}

pub fn extract_chunks<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
) -> Vec<CodeChunk> {
    let mut chunks = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    extract_impl(&node, source, file_path, deps, &mut chunks, &mut spans);
    if let Some(r) = make_remainder_chunk(source, &spans, file_path, deps) {
        chunks.push(r);
    }
    chunks
}

fn extract_impl<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    chunks: &mut Vec<CodeChunk>,
    spans: &mut Vec<(usize, usize)>,
) {
    if FUNCTION_NODE_TYPES.contains(&node.kind()) {
        // Skip misparse: `class MACRO RealName {...}` produces a function_definition
        // whose first child is a class_specifier — not a real function.
        let has_type_child = {
            let mut cursor = node.walk();
            let has = node
                .children(&mut cursor)
                .any(|c| TYPE_NODE_TYPES.contains(&c.kind()));
            has
        };
        if has_type_child {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_impl(&child, source, file_path, deps, chunks, spans);
            }
            return;
        }

        let declarator = node.child_by_field_name("declarator");
        let (function_name, qualified_class) =
            parse_qualified_name_from_declarator(declarator.as_ref(), source);
        let function_name = function_name.or_else(|| {
            super::c::extract_declarator_name_pub(declarator.as_ref(), source)
        });
        let parameters_node = super::c::find_parameters_node_pub(declarator.as_ref());
        let parameters = super::c::extract_parameters_pub(parameters_node.as_ref(), source);

        let return_type = if !matches!(node.kind(), "constructor_definition" | "destructor_definition") {
            super::c::extract_return_type_pub(node, source)
        } else {
            None
        };

        let class_name = find_enclosing_class_name(node, source).or(qualified_class);
        let namespace = find_nearest_namespace(node, source);
        let code =
            node_text(&node, source);

        chunks.push(make_chunk(
            file_path,
            function_name,
            class_name,
            parameters,
            return_type,
            namespace,
            code,
            node.kind(),
            deps,
        ));
        spans.push((node.start_byte(), node.end_byte()));
    }

    if TYPE_NODE_TYPES.contains(&node.kind()) {
        // Handle misparse where `class MACRO RealName { ... }` is wrapped by a
        // parent `function_definition` whose identifier sibling carries the
        // real class name.
        let parent = node.parent();
        if let Some(parent) = parent {
            if parent.kind() == "function_definition" {
                let mut cursor = parent.walk();
                let sibling_id = {
                    let mut iter = parent.children(&mut cursor);
                    let found = iter.find(|c| c.kind() == "identifier");
                    found
                };
                if let Some(sibling_id) = sibling_id {
                    let type_name = Some(node_text(&sibling_id, source));
                    let namespace = find_nearest_namespace(node, source);
                    let code = node_text(&parent, source);
                    chunks.push(make_chunk(
                        file_path,
                        None,
                        type_name,
                        Vec::new(),
                        None,
                        namespace,
                        code,
                        node.kind(),
                        deps,
                    ));
                    // Don't push spans — children will be visited separately.
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        extract_impl(&child, source, file_path, deps, chunks, spans);
                    }
                    return;
                }
            }
        }

        let name_node = node.child_by_field_name("name");
        let type_name = name_node.map(|n| node_text(&n, source));
        let namespace = find_nearest_namespace(node, source);
        let code =
            node_text(&node, source);
        if let Some(type_name) = type_name.filter(|s| !s.is_empty()) {
            chunks.push(make_chunk(
                file_path,
                None,
                Some(type_name),
                Vec::new(),
                None,
                namespace,
                code,
                node.kind(),
                deps,
            ));
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_impl(&child, source, file_path, deps, chunks, spans);
    }
}

fn parse_qualified_name_from_declarator<'a>(
    declarator: Option<&Node<'a>>,
    source: &[u8],
) -> (Option<String>, Option<String>) {
    let Some(declarator) = declarator else { return (None, None) };
    let text = node_text(declarator, source);
    let text = text.split('(').next().unwrap_or("").trim();
    if text.is_empty() {
        return (None, None);
    }
    if !text.contains("::") {
        return (Some(text.to_string()), None);
    }
    let parts: Vec<&str> = text.split("::").filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return (None, None);
    }
    let last = parts.last().copied().map(|s| s.to_string());
    let second_last = if parts.len() > 1 {
        Some(parts[parts.len() - 2].to_string())
    } else {
        None
    };
    (last, second_last)
}

fn find_enclosing_class_name<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if TYPE_NODE_TYPES.contains(&parent.kind()) {
            if let Some(name) = parent.child_by_field_name("name") {
                return Some(node_text(&name, source));
            }
            let mut cursor = parent.walk();
            for child in parent.named_children(&mut cursor) {
                if child.kind() == "type_identifier" || child.kind() == "identifier" {
                    return Some(node_text(&child, source));
                }
            }
        }
        // Misparse: function_definition with a class_specifier child is really a class.
        if parent.kind() == "function_definition" {
            let mut cursor = parent.walk();
            let has_type_child = {
                let v = parent
                    .children(&mut cursor)
                    .any(|c| TYPE_NODE_TYPES.contains(&c.kind()));
                v
            };
            if has_type_child {
                let mut cursor = parent.walk();
                let id_opt = { parent.children(&mut cursor).find(|c| c.kind() == "identifier") };
                if let Some(id) = id_opt {
                    return Some(node_text(&id, source));
                }
            }
        }
        current = parent.parent();
    }
    None
}

fn find_nearest_namespace<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "namespace_definition" {
            if let Some(name) = parent.child_by_field_name("name") {
                return Some(node_text(&name, source));
            }
            let mut cursor = parent.walk();
            for child in parent.named_children(&mut cursor) {
                if child.kind() == "namespace_identifier" || child.kind() == "identifier" {
                    return Some(node_text(&child, source));
                }
            }
        }
        current = parent.parent();
    }
    None
}

fn make_chunk(
    file_path: &str,
    function_name: Option<String>,
    class_name: Option<String>,
    parameters: Vec<Parameter>,
    return_type: Option<String>,
    namespace: Option<String>,
    code: String,
    chunk_type: &str,
    deps: &[String],
) -> CodeChunk {
    let id = generate_chunk_id(
        file_path,
        &function_name
            .clone()
            .or(class_name.clone())
            .unwrap_or("Unknown".to_string()),
    );
    CodeChunk {
        id,
        repo_name: String::new(),
        file_name: Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(file_path)
            .to_string(),
        code,
        r#type: chunk_type.to_string(),
        dependencies: deps.to_vec(),
        created_at: crate::services::ingestion::types::now_iso(),
        kind: CodeChunkKind::Cpp(CppFields {
            function_name,
            class_name,
            parameters,
            return_type,
            namespace,
        }),
    }
}