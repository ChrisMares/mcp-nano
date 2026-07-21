//! Rust chunker. Direct port of `chunk_code_rust.py`.
//!
//! Recursively walks the AST, emits one chunk per `function_item`,
//! `struct_item`, `enum_item`, and `trait_item`. Enclosing `impl` types
//! and `mod` names become `class_name` and `namespace` respectively.

use std::path::Path;

use tree_sitter::Node;

use super::helpers::{generate_chunk_id, make_remainder_chunk, node_text};
use crate::services::ingestion::types::{CodeChunk, CodeChunkKind, Parameter, RustFields};

const FUNCTION_NODE_TYPES: &[&str] = &["function_item"];
const TYPE_NODE_TYPES: &[&str] = &["struct_item", "enum_item", "trait_item"];

pub fn extract_dependencies<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    let mut deps: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<Node<'a>> = vec![root];
    while let Some(node) = stack.pop() {
        let kind = node.kind();
        if kind == "use_declaration" || kind == "extern_crate_declaration" {
            let dep = normalize_rust_dependency(&node_text(&node, source));
            if !dep.is_empty() && seen.insert(dep.clone()) {
                deps.push(dep);
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    deps
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
    let Some(node) = parameters_node else { return params };
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

fn normalize_type_text(text: &str) -> String {
    let mut s = text.trim().to_string();
    if let Some(stripped) = s.strip_prefix(':') {
        s = stripped.trim().to_string();
    }
    s
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
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "mod_item" || parent.kind() == "mod_declaration" {
            if let Some(name) = parent.child_by_field_name("name") {
                return Some(node_text(&name, source));
            }
            let mut cursor = parent.walk();
            for child in parent.named_children(&mut cursor) {
                if child.kind() == "identifier" || child.kind() == "type_identifier" {
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
    dependencies: &[String],
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
        dependencies: dependencies.to_vec(),
        created_at: crate::services::ingestion::types::now_iso(),
        kind: CodeChunkKind::Rust(RustFields {
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
    dependencies: &[String],
) -> Vec<CodeChunk> {
    let mut chunks = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    extract_impl(&node, source, file_path, dependencies, &mut chunks, &mut spans);
    if let Some(r) = make_remainder_chunk(source, &spans, file_path, dependencies) {
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
        let name_node = node.child_by_field_name("name");
        let function_name = name_node.map(|n| node_text(&n, source));
        let parameters_node = node.child_by_field_name("parameters");
        let parameters = extract_parameters(parameters_node.as_ref(), source);
        let return_type = get_return_type(node, source);
        let class_name = find_enclosing_impl_type(node, source);
        let namespace = find_nearest_module(node, source);
        let code =
            String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned();
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
        let name_node = node.child_by_field_name("name");
        let type_name = name_node.map(|n| node_text(&n, source));
        let namespace = find_nearest_module(node, source);
        let code =
            String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned();
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
        spans.push((node.start_byte(), node.end_byte()));
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_impl(&child, source, file_path, deps, chunks, spans);
    }
}