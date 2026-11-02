//! C++ chunker. Direct port of `chunk_code_cpp.py`.

use tree_sitter::Node;

use super::c::{
    extract_declarator_name, extract_parameters, extract_return_type, find_parameters_node,
};
use super::helpers::{
    chunks_with_remainder, find_child_of_kind, has_child_of_kind, member_chunk,
    nearest_ancestor_name, node_text,
};
use crate::services::ingestion::types::{CodeChunk, CodeChunkKind, CppFields};

const FUNCTION_NODE_TYPES: &[&str] = &[
    "function_definition",
    "method_definition",
    "constructor_definition",
    "destructor_definition",
    "operator_cast_definition",
];
const TYPE_NODE_TYPES: &[&str] = &["class_specifier", "struct_specifier"];

pub use super::c::extract_dependencies;

pub fn extract_chunks<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
) -> Vec<CodeChunk> {
    chunks_with_remainder(source, file_path, deps, |chunks, spans| {
        extract_impl(&node, source, file_path, deps, chunks, spans)
    })
}

fn extract_impl<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    chunks: &mut Vec<CodeChunk>,
    spans: &mut Vec<(usize, usize)>,
) {
    // Skip misparse: `class MACRO RealName {...}` produces a function_definition
    // whose first child is a class_specifier — not a real function.
    if FUNCTION_NODE_TYPES.contains(&node.kind()) && !has_child_of_kind(node, TYPE_NODE_TYPES) {
        let declarator = node.child_by_field_name("declarator");
        let (function_name, qualified_class) =
            parse_qualified_name_from_declarator(declarator.as_ref(), source);
        let parameters_node = find_parameters_node(declarator.as_ref());
        let return_type = if matches!(
            node.kind(),
            "constructor_definition" | "destructor_definition"
        ) {
            None
        } else {
            extract_return_type(node, source)
        };
        let fields = CppFields {
            function_name: function_name
                .or_else(|| extract_declarator_name(declarator.as_ref(), source)),
            class_name: find_enclosing_class_name(node, source).or(qualified_class),
            parameters: extract_parameters(parameters_node.as_ref(), source),
            return_type,
            namespace: find_nearest_namespace(node, source),
        };
        chunks.push(member_chunk(
            file_path,
            fields,
            node_text(node, source),
            node.kind(),
            deps,
            CodeChunkKind::Cpp,
        ));
        spans.push((node.start_byte(), node.end_byte()));
    }

    if TYPE_NODE_TYPES.contains(&node.kind()) {
        if let Some((type_name, code)) = type_name_and_code(node, source) {
            let fields = CppFields {
                class_name: Some(type_name),
                namespace: find_nearest_namespace(node, source),
                ..Default::default()
            };
            // Don't push spans — members are visited separately.
            chunks.push(member_chunk(
                file_path,
                fields,
                code,
                node.kind(),
                deps,
                CodeChunkKind::Cpp,
            ));
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_impl(&child, source, file_path, deps, chunks, spans);
    }
}

/// Name and code of a class/struct definition. Handles the misparse where
/// `class MACRO RealName { ... }` is wrapped by a parent `function_definition`
/// whose identifier child carries the real class name. Forward declarations
/// (no `body`) and unnamed types return `None`.
fn type_name_and_code<'a>(node: &Node<'a>, source: &[u8]) -> Option<(String, String)> {
    if let Some(parent) = node.parent() {
        if parent.kind() == "function_definition" {
            if let Some(id) = find_child_of_kind(&parent, "identifier") {
                return Some((node_text(&id, source), node_text(&parent, source)));
            }
        }
    }
    node.child_by_field_name("body")?;
    let name = node_text(&node.child_by_field_name("name")?, source);
    if name.is_empty() {
        return None;
    }
    Some((name, node_text(node, source)))
}

fn parse_qualified_name_from_declarator<'a>(
    declarator: Option<&Node<'a>>,
    source: &[u8],
) -> (Option<String>, Option<String>) {
    let Some(declarator) = declarator else {
        return (None, None);
    };
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
        if parent.kind() == "function_definition" && has_child_of_kind(&parent, TYPE_NODE_TYPES) {
            if let Some(id) = find_child_of_kind(&parent, "identifier") {
                return Some(node_text(&id, source));
            }
        }
        current = parent.parent();
    }
    None
}

fn find_nearest_namespace<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    nearest_ancestor_name(
        node,
        source,
        &["namespace_definition"],
        &["namespace_identifier", "identifier"],
    )
}
