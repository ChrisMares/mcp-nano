//! C chunker. Direct port of `chunk_code_c.py`.
//!
//! Extracts `function_definition` nodes plus a trailing `file_remainder`
//! chunk. Includes are normalized into the dependency list (angle-bracket
//! and quote forms both supported).

use std::path::Path;

use tree_sitter::Node;

use super::helpers::{generate_chunk_id, make_remainder_chunk, node_text, walk_preorder};
use crate::services::ingestion::types::{CFields, CodeChunk, CodeChunkKind, Parameter};

const FUNCTION_NODE_TYPES: &[&str] = &["function_definition"];

/// Shared dependency extractor for C and C++ (both scan `preproc_include`).
pub fn extract_dependencies<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    let mut deps: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    walk_preorder(root, |node| {
        if node.kind() == "preproc_include" {
            if let Some(dep) = extract_include_target(&node, source) {
                if seen.insert(dep.clone()) {
                    deps.push(dep);
                }
            }
        }
    });
    deps
}

/// Same as `extract_dependencies` — re-exported under a clearer name for
/// the C++ chunker to reuse.
pub(super) fn extract_dependencies_same<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    extract_dependencies(root, source)
}

fn extract_include_target<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    if let Some(pn) = node
        .child_by_field_name("path")
        .or_else(|| node.child_by_field_name("name"))
    {
        return Some(normalize_include_target(&node_text(&pn, source)));
    }
    let raw = node_text(node, source);
    if let Some(idx) = raw.find("#include") {
        return Some(normalize_include_target(&raw[idx + "#include".len()..]));
    }
    Some(normalize_include_target(&raw))
}

fn normalize_include_target(value: &str) -> String {
    let cleaned = value.trim();
    let bytes = cleaned.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0] as char;
        if first == '<' && cleaned.ends_with('>') {
            return cleaned[1..cleaned.len() - 1].trim().to_string();
        }
        if (first == '"' || first == '\'') && cleaned.ends_with(first) {
            return cleaned[1..cleaned.len() - 1].trim().to_string();
        }
    }
    cleaned.to_string()
}

pub(super) fn extract_declarator_name_pub<'a>(
    node: Option<&Node<'a>>,
    source: &[u8],
) -> Option<String> {
    extract_declarator_name(node, source)
}

fn extract_declarator_name<'a>(node: Option<&Node<'a>>, source: &[u8]) -> Option<String> {
    let node = node?;
    let mut name: Option<String> = None;
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "type_identifier"
    ) {
        name = Some(node_text(node, source));
    }
    if matches!(node.kind(), "parameter_list" | "parameter_declaration") {
        return name;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(cn) = extract_declarator_name(Some(&child), source) {
            name = Some(cn);
        }
    }
    name
}

fn extract_function_name_from_declarator<'a>(
    declarator: Option<&Node<'a>>,
    source: &[u8],
) -> Option<String> {
    let declarator = declarator?;
    let text = node_text(declarator, source);
    let mut name_text = text.split('(').next().unwrap_or("").trim().to_string();
    if name_text.is_empty() {
        return None;
    }
    name_text = name_text.replace(['(', ')'], "").trim().to_string();
    while let Some(stripped) = name_text.strip_prefix(|c| c == '*' || c == '&') {
        name_text = stripped.trim().to_string();
    }
    if name_text.contains(' ') {
        name_text = name_text
            .split_whitespace()
            .last()
            .unwrap_or("")
            .to_string();
    }
    if name_text.is_empty() {
        None
    } else {
        Some(name_text)
    }
}

pub(super) fn find_parameters_node_pub<'a>(node: Option<&Node<'a>>) -> Option<Node<'a>> {
    find_parameters_node(node)
}

fn find_parameters_node<'a>(node: Option<&Node<'a>>) -> Option<Node<'a>> {
    let node = node?;
    if let Some(pn) = node.child_by_field_name("parameters") {
        return Some(pn);
    }
    if node.kind() == "parameter_list" {
        return Some(*node);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_parameters_node(Some(&child)) {
            return Some(found);
        }
    }
    None
}

pub(super) fn extract_parameters_pub<'a>(
    parameters_node: Option<&Node<'a>>,
    source: &[u8],
) -> Vec<Parameter> {
    extract_parameters(parameters_node, source)
}

pub(super) fn extract_parameters<'a>(
    parameters_node: Option<&Node<'a>>,
    source: &[u8],
) -> Vec<Parameter> {
    let mut parameters = Vec::new();
    let Some(node) = parameters_node else { return parameters };
    let mut cursor = node.walk();
    for param in node.named_children(&mut cursor) {
        if param.kind() == "parameter_declaration" {
            let name_node = param
                .child_by_field_name("declarator")
                .or_else(|| param.child_by_field_name("name"));
            let name = name_node.and_then(|n| extract_declarator_name(Some(&n), source));
            let param_text = node_text(&param, source)
                .trim()
                .split('=')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            let param_type = extract_param_type_from_text(&param_text, name.as_deref());
            if let Some(name) = name.as_ref() {
                if !name.is_empty() {
                    parameters.push(Parameter::new(name.clone(), param_type));
                    continue;
                }
            }
            if !param_type.is_empty() {
                parameters.push(Parameter::new(param_type, String::new()));
            }
        } else if param.kind() == "variadic_parameter" {
            parameters.push(Parameter::new("...", String::new()));
        }
    }
    parameters
}

fn extract_param_type_from_text(param_text: &str, name: Option<&str>) -> String {
    if param_text.is_empty() {
        return String::new();
    }
    if let Some(name) = name {
        if let Some(stripped) = param_text.strip_suffix(name) {
            return stripped.trim().to_string();
        }
    }
    param_text.to_string()
}

pub(super) fn extract_return_type_pub<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    extract_return_type(node, source)
}

fn extract_return_type<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    if let Some(tn) = node.child_by_field_name("type") {
        return Some(node_text(&tn, source));
    }
    if let Some(specs) = node.child_by_field_name("declaration_specifiers") {
        return Some(node_text(&specs, source));
    }
    let declarator = node.child_by_field_name("declarator");
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if declarator.is_some() && Some(child) == declarator {
            break;
        }
        if matches!(
            child.kind(),
            "primitive_type"
                | "type_identifier"
                | "sized_type_specifier"
                | "struct_specifier"
                | "enum_specifier"
                | "union_specifier"
        ) {
            return Some(node_text(&child, source));
        }
    }
    None
}

fn make_chunk(
    file_path: &str,
    function_name: Option<String>,
    parameters: Vec<Parameter>,
    return_type: Option<String>,
    code: String,
    chunk_type: &str,
    deps: &[String],
) -> CodeChunk {
    let id = generate_chunk_id(
        file_path,
        &function_name.clone().unwrap_or("Unknown".to_string()),
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
        kind: CodeChunkKind::C(CFields {
            function_name,
            parameters,
            return_type,
            namespace: None,
        }),
    }
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
        let declarator = node.child_by_field_name("declarator");
        let mut function_name = extract_function_name_from_declarator(declarator.as_ref(), source);
        if function_name.is_none() {
            function_name = extract_declarator_name(declarator.as_ref(), source);
        }
        let parameters_node = find_parameters_node(declarator.as_ref());
        let parameters = extract_parameters(parameters_node.as_ref(), source);
        let return_type = extract_return_type(node, source);
        let code =
            String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned();
        chunks.push(make_chunk(
            file_path,
            function_name,
            parameters,
            return_type,
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