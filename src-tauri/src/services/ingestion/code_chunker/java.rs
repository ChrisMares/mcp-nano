//! Java chunker. Direct port of `chunk_code_java.py`.
//!
//! Methods/classes/interfaces/enums each become their own chunk. The
//! enclosing `package` declaration becomes the `namespace` field. Methods
//! are *also* chunked when nested inside a class — the class chunk doesn't
//! reserve a removable span so we still descend into it.

use std::path::Path;

use tree_sitter::Node;

use super::helpers::{generate_chunk_id, make_remainder_chunk, node_text, walk_preorder};
use crate::services::ingestion::types::{CodeChunk, CodeChunkKind, JavaFields, Parameter};

const FUNCTION_NODE_TYPES: &[&str] = &["method_declaration", "constructor_declaration"];
const TYPE_NODE_TYPES: &[&str] = &["class_declaration", "interface_declaration", "enum_declaration"];

pub fn extract_dependencies<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    let mut deps: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    walk_preorder(root, |node| {
        if node.kind() == "import_declaration" {
            let dep = normalize_java_import(&node_text(&node, source));
            if !dep.is_empty() && seen.insert(dep.clone()) {
                deps.push(dep);
            }
        }
    });
    deps
}

pub fn extract_package<'a>(root: &Node<'a>, source: &[u8]) -> Option<String> {
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        if child.kind() == "package_declaration" {
            if let Some(name) = child.child_by_field_name("name") {
                return Some(node_text(&name, source));
            }
            let raw = node_text(&child, source);
            return Some(
                raw.replace("package", "")
                    .replace(';', "")
                    .trim()
                    .to_string(),
            );
        }
    }
    None
}

fn normalize_java_import(text: &str) -> String {
    let mut t = text.trim().to_string();
    if let Some(stripped) = t.strip_prefix("import ") {
        t = stripped.trim().to_string();
    }
    if let Some(stripped) = t.strip_prefix("static ") {
        t = stripped.trim().to_string();
    }
    if let Some(stripped) = t.strip_suffix(';') {
        t = stripped.trim().to_string();
    }
    t
}

fn extract_parameters<'a>(parameters_node: Option<&Node<'a>>, source: &[u8]) -> Vec<Parameter> {
    let mut params = Vec::new();
    let Some(node) = parameters_node else { return params };
    let mut cursor = node.walk();
    for param in node.named_children(&mut cursor) {
        if param.kind() == "formal_parameter" || param.kind() == "spread_parameter" {
            let name = param.child_by_field_name("name");
            let type_node = param.child_by_field_name("type");
            let name_s = name.map(|n| node_text(&n, source)).unwrap_or_default();
            let type_s = type_node.map(|n| node_text(&n, source)).unwrap_or_default();
            if !name_s.is_empty() {
                params.push(Parameter::new(name_s, type_s));
            } else if !type_s.is_empty() {
                params.push(Parameter::new(type_s, String::new()));
            }
        }
    }
    params
}

fn get_return_type<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    node.child_by_field_name("type").map(|n| node_text(&n, source))
}

fn find_enclosing_class_name<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if TYPE_NODE_TYPES.contains(&parent.kind()) {
            if let Some(name) = parent.child_by_field_name("name") {
                return Some(node_text(&name, source));
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
        kind: CodeChunkKind::Java(JavaFields {
            function_name,
            class_name,
            parameters,
            return_type,
            namespace,
        }),
    }
}

pub fn extract_chunks<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    namespace: Option<&str>,
) -> Vec<CodeChunk> {
    let mut chunks = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let ns_owned = namespace.map(|s| s.to_string());
    extract_impl(
        &node,
        source,
        file_path,
        deps,
        ns_owned.as_deref(),
        &mut chunks,
        &mut spans,
    );
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
    namespace: Option<&str>,
    chunks: &mut Vec<CodeChunk>,
    spans: &mut Vec<(usize, usize)>,
) {
    if FUNCTION_NODE_TYPES.contains(&node.kind()) {
        let name_node = node.child_by_field_name("name");
        let function_name = name_node.map(|n| node_text(&n, source));
        let parameters_node = node.child_by_field_name("parameters");
        let parameters = extract_parameters(parameters_node.as_ref(), source);
        let return_type = if node.kind() == "constructor_declaration" {
            None
        } else {
            get_return_type(node, source)
        };
        let class_name = find_enclosing_class_name(node, source);
        let code =
            String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned();
        chunks.push(make_chunk(
            file_path,
            function_name,
            class_name,
            parameters,
            return_type,
            namespace.map(|s| s.to_string()),
            code,
            node.kind(),
            deps,
        ));
        spans.push((node.start_byte(), node.end_byte()));
    }

    if TYPE_NODE_TYPES.contains(&node.kind()) {
        let name_node = node.child_by_field_name("name");
        let type_name = name_node.map(|n| node_text(&n, source));
        let code =
            String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned();
        chunks.push(make_chunk(
            file_path,
            None,
            type_name,
            Vec::new(),
            None,
            namespace.map(|s| s.to_string()),
            code,
            node.kind(),
            deps,
        ));
        // Don't add to spans — methods inside are still extracted individually.
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_impl(&child, source, file_path, deps, namespace, chunks, spans);
    }
}