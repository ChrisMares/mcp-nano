//! TypeScript chunker. Direct port of `chunk_code_ts.py`.
//!
//! Walks the tree like the JavaScript chunker ([`super::js::Dialect`]) and
//! adds parameter/return types plus interface, type alias, and enum chunks.

use tree_sitter::Node;

use super::helpers::{node_text, normalize_type_text};
use super::js::{extract_dialect_chunks, Dialect};
use crate::services::ingestion::types::{CodeChunk, Parameter};

pub(super) const TYPE_DECLARATION_TYPES: &[&str] = &[
    "interface_declaration",
    "type_alias_declaration",
    "enum_declaration",
];

pub use super::js::extract_dependencies;

pub(super) fn extract_parameters<'a>(
    parameters_node: Option<&Node<'a>>,
    source: &[u8],
) -> Vec<Parameter> {
    let mut params = Vec::new();
    let Some(node) = parameters_node else {
        return params;
    };
    let mut cursor = node.walk();
    for param in node.named_children(&mut cursor) {
        let name = get_param_name(&param, source);
        let ptype = get_param_type(&param, source);
        if !name.is_empty() {
            params.push(Parameter::new(name, ptype));
        }
    }
    params
}

fn get_param_name<'a>(node: &Node<'a>, source: &[u8]) -> String {
    let name_node = node
        .child_by_field_name("pattern")
        .or_else(|| node.child_by_field_name("name"))
        .or_else(|| node.child_by_field_name("left"))
        .or_else(|| node.child_by_field_name("argument"));
    if let Some(n) = name_node {
        return node_text(&n, source);
    }
    node_text(node, source)
}

fn get_param_type<'a>(node: &Node<'a>, source: &[u8]) -> String {
    let type_node = node
        .child_by_field_name("type")
        .or_else(|| node.child_by_field_name("type_annotation"));
    if let Some(tn) = type_node {
        return normalize_type_text(&node_text(&tn, source));
    }
    String::new()
}

pub(super) fn get_return_type<'a>(
    node: &Node<'a>,
    parameters_node: Option<&Node<'a>>,
    source: &[u8],
) -> Option<String> {
    if let Some(rn) = node
        .child_by_field_name("return_type")
        .or_else(|| node.child_by_field_name("type"))
    {
        return Some(normalize_type_text(&node_text(&rn, source)));
    }
    if let Some(params) = parameters_node {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "type_annotation" && child.start_byte() > params.end_byte() {
                return Some(normalize_type_text(&node_text(&child, source)));
            }
        }
    }
    None
}

pub fn extract_chunks<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
) -> Vec<CodeChunk> {
    extract_dialect_chunks(node, source, file_path, deps, Dialect::TypeScript)
}
