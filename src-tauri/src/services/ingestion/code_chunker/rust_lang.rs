//! Rust chunker. Direct port of `chunk_code_rust.py`.
//!
//! Recursively walks the AST, emits one chunk per `function_item`,
//! `struct_item`, `enum_item`, and `trait_item`. Enclosing `impl` types
//! and `mod` names become `class_name` and `namespace` respectively.

use tree_sitter::Node;

use super::helpers::{
    chunks_with_remainder, collect_dependencies, field_text, member_chunk, nearest_ancestor_name,
    node_text, normalize_type_text,
};
use crate::services::ingestion::types::{CodeChunk, CodeChunkKind, Parameter, RustFields};

const FUNCTION_NODE_TYPES: &[&str] = &["function_item"];
const TYPE_NODE_TYPES: &[&str] = &["struct_item", "enum_item", "trait_item"];

pub fn extract_dependencies<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    collect_dependencies(root, |node, deps| {
        if matches!(node.kind(), "use_declaration" | "extern_crate_declaration") {
            let dep = normalize_rust_dependency(&node_text(&node, source));
            if !dep.is_empty() {
                deps.push(dep);
            }
        }
    })
}

fn normalize_rust_dependency(text: &str) -> String {
    let mut t = text.trim().to_string();
    if let Some(stripped) = t.strip_prefix("pub ") {
        t = stripped.trim().to_string();
    }
    if let Some(stripped) = t.strip_prefix("use ") {
        t = stripped.trim().to_string();
    }
    if let Some(stripped) = t.strip_prefix("extern crate ") {
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
        if param.kind() == "self_parameter" {
            params.push(Parameter::new("self", node_text(&param, source).trim()));
            continue;
        }
        if param.kind() == "parameter" {
            let name_node = param
                .child_by_field_name("pattern")
                .or_else(|| param.child_by_field_name("name"));
            let type_node = param
                .child_by_field_name("type")
                .or_else(|| param.child_by_field_name("type_annotation"));
            let name = name_node
                .map(|n| node_text(&n, source).trim().to_string())
                .unwrap_or_default();
            let param_type = type_node
                .map(|n| normalize_type_text(&node_text(&n, source)))
                .unwrap_or_default();
            if !name.is_empty() {
                params.push(Parameter::new(name, param_type));
            } else if !param_type.is_empty() {
                params.push(Parameter::new(param_type, String::new()));
            }
        }
    }
    params
}

fn get_return_type<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    node.child_by_field_name("return_type")
        .or_else(|| node.child_by_field_name("type"))
        .map(|n| node_text(&n, source))
}

fn find_enclosing_impl_type<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "impl_item" {
            if let Some(tn) = parent.child_by_field_name("type") {
                return Some(node_text(&tn, source));
            }
            let mut cursor = parent.walk();
            for child in parent.named_children(&mut cursor) {
                if child.kind() == "type_identifier" || child.kind() == "identifier" {
                    return Some(node_text(&child, source));
                }
            }
        }
        if parent.kind() == "trait_item" {
            if let Some(name) = parent.child_by_field_name("name") {
                return Some(node_text(&name, source));
            }
        }
        current = parent.parent();
    }
    None
}

fn find_nearest_module<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    nearest_ancestor_name(
        node,
        source,
        &["mod_item", "mod_declaration"],
        &["identifier", "type_identifier"],
    )
}

pub fn extract_chunks<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    dependencies: &[String],
) -> Vec<CodeChunk> {
    chunks_with_remainder(source, file_path, dependencies, |chunks, spans| {
        extract_impl(&node, source, file_path, dependencies, chunks, spans)
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
    let is_function = FUNCTION_NODE_TYPES.contains(&node.kind());
    if is_function || TYPE_NODE_TYPES.contains(&node.kind()) {
        let fields = if is_function {
            let parameters_node = node.child_by_field_name("parameters");
            RustFields {
                function_name: field_text(node, "name", source),
                class_name: find_enclosing_impl_type(node, source),
                parameters: extract_parameters(parameters_node.as_ref(), source),
                return_type: get_return_type(node, source),
                namespace: find_nearest_module(node, source),
            }
        } else {
            RustFields {
                class_name: field_text(node, "name", source),
                namespace: find_nearest_module(node, source),
                ..Default::default()
            }
        };
        chunks.push(member_chunk(
            file_path,
            fields,
            node_text(node, source),
            node.kind(),
            deps,
            CodeChunkKind::Rust,
        ));
        spans.push((node.start_byte(), node.end_byte()));
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_impl(&child, source, file_path, deps, chunks, spans);
    }
}
