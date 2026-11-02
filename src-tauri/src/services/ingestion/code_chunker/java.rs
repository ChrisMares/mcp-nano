//! Java chunker. Direct port of `chunk_code_java.py`.
//!
//! Methods/classes/interfaces/enums each become their own chunk. The
//! enclosing `package` declaration becomes the `namespace` field. Methods
//! are *also* chunked when nested inside a class — the class chunk doesn't
//! reserve a removable span so we still descend into it.

use tree_sitter::Node;

use super::helpers::{
    chunks_with_remainder, collect_dependencies, field_text, member_chunk, nearest_ancestor_name,
    node_text,
};
use crate::services::ingestion::types::{CodeChunk, CodeChunkKind, JavaFields, Parameter};

const FUNCTION_NODE_TYPES: &[&str] = &["method_declaration", "constructor_declaration"];
const TYPE_NODE_TYPES: &[&str] = &[
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
];

pub fn extract_dependencies<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    collect_dependencies(root, |node, deps| {
        if node.kind() == "import_declaration" {
            let dep = normalize_java_import(&node_text(&node, source));
            if !dep.is_empty() {
                deps.push(dep);
            }
        }
    })
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
    let Some(node) = parameters_node else {
        return params;
    };
    let mut cursor = node.walk();
    for param in node.named_children(&mut cursor) {
        if param.kind() == "formal_parameter" || param.kind() == "spread_parameter" {
            let name_s = field_text(&param, "name", source).unwrap_or_default();
            let type_s = field_text(&param, "type", source).unwrap_or_default();
            if !name_s.is_empty() {
                params.push(Parameter::new(name_s, type_s));
            } else if !type_s.is_empty() {
                params.push(Parameter::new(type_s, String::new()));
            }
        }
    }
    params
}

pub fn extract_chunks<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    namespace: Option<&str>,
) -> Vec<CodeChunk> {
    chunks_with_remainder(source, file_path, deps, |chunks, spans| {
        extract_impl(&node, source, file_path, deps, namespace, chunks, spans)
    })
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
        let parameters_node = node.child_by_field_name("parameters");
        let return_type = if node.kind() == "constructor_declaration" {
            None
        } else {
            field_text(node, "type", source)
        };
        let fields = JavaFields {
            function_name: field_text(node, "name", source),
            class_name: nearest_ancestor_name(node, source, TYPE_NODE_TYPES, &[]),
            parameters: extract_parameters(parameters_node.as_ref(), source),
            return_type,
            namespace: namespace.map(str::to_string),
        };
        chunks.push(member_chunk(
            file_path,
            fields,
            node_text(node, source),
            node.kind(),
            deps,
            CodeChunkKind::Java,
        ));
        spans.push((node.start_byte(), node.end_byte()));
    }

    if TYPE_NODE_TYPES.contains(&node.kind()) {
        let fields = JavaFields {
            class_name: field_text(node, "name", source),
            namespace: namespace.map(str::to_string),
            ..Default::default()
        };
        chunks.push(member_chunk(
            file_path,
            fields,
            node_text(node, source),
            node.kind(),
            deps,
            CodeChunkKind::Java,
        ));
        // Don't add to spans — methods inside are still extracted individually.
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_impl(&child, source, file_path, deps, namespace, chunks, spans);
    }
}
