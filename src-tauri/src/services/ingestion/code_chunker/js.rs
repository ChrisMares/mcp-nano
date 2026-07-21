//! JavaScript chunker. Direct port of `chunk_code_js.py`.
//!
//! Extracts `function_declaration`/`generator_function_declaration` and
//! `method_definition` nodes. Variable declarators bound to a function
//! expression produce a separate chunk whose span is the whole enclosing
//! declaration (mirrors Python's `get_declarator_removal_span`).

use std::path::Path;

use tree_sitter::Node;

use super::helpers::{
    generate_chunk_id, get_declarator_removal_span, make_remainder_chunk, node_text,
    strip_js_import_source,
};
use crate::services::ingestion::types::{CodeChunk, CodeChunkKind, JavaScriptFields, Parameter};

const FUNCTION_DECLARATION_TYPES: &[&str] = &["function_declaration", "generator_function_declaration"];
const FUNCTION_EXPRESSION_TYPES: &[&str] = &["arrow_function", "function_expression", "generator_function"];
const METHOD_TYPES: &[&str] = &["method_definition"];

pub fn extract_dependencies<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    let mut deps: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<Node<'a>> = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "import_statement" || node.kind() == "import_declaration" {
            if let Some(src) = node.child_by_field_name("source") {
                let dep = strip_js_import_source(&node_text(&src, source).trim());
                if !dep.is_empty() && seen.insert(dep.clone()) {
                    deps.push(dep);
                }
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    deps
}

fn extract_parameters<'a>(parameters_node: Option<&Node<'a>>, source: &[u8]) -> Vec<Parameter> {
    let mut params = Vec::new();
    let Some(node) = parameters_node else { return params };
    let mut cursor = node.walk();
    for param in node.named_children(&mut cursor) {
        let name = get_param_name(&param, source);
        if !name.is_empty() {
            params.push(Parameter::new(name, String::new()));
        }
    }
    params
}

fn get_param_name<'a>(node: &Node<'a>, source: &[u8]) -> String {
    let name_node = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("pattern"))
        .or_else(|| node.child_by_field_name("left"))
        .or_else(|| node.child_by_field_name("argument"));
    if let Some(n) = name_node {
        return node_text(&n, source);
    }
    node_text(node, source)
}

fn node_is_async<'a>(node: &Node<'a>) -> bool {
    if matches!(node.kind(), "async_function" | "async_function_declaration") {
        return true;
    }
    let mut cursor = node.walk();
    let has_async = node.children(&mut cursor).any(|c| c.kind() == "async");
    has_async
}

fn node_is_generator<'a>(node: &Node<'a>) -> bool {
    if matches!(node.kind(), "generator_function" | "generator_function_declaration") {
        return true;
    }
    let mut cursor = node.walk();
    let has_gen = node.children(&mut cursor).any(|c| c.kind() == "*");
    has_gen
}

fn find_enclosing_class_name<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(parent.kind(), "class_declaration" | "class") {
            if let Some(name) = parent.child_by_field_name("name") {
                return Some(node_text(&name, source));
            }
            return None;
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
    is_method: bool,
    is_async: bool,
    is_generator: bool,
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
        kind: CodeChunkKind::JavaScript(JavaScriptFields {
            function_name,
            class_name,
            parameters,
            return_type: None,
            is_method,
            is_async,
            is_generator,
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
    if FUNCTION_DECLARATION_TYPES.contains(&node.kind()) {
        if let Some(name_node) = node.child_by_field_name("name") {
            let function_name = node_text(&name_node, source);
            let parameters_node = node.child_by_field_name("parameters");
            let parameters = extract_parameters(parameters_node.as_ref(), source);
            let code = String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned();
            chunks.push(make_chunk(
                file_path,
                Some(function_name),
                None,
                parameters,
                false,
                node_is_async(node),
                node_is_generator(node),
                code,
                node.kind(),
                deps,
            ));
            spans.push((node.start_byte(), node.end_byte()));
        }
    }

    if METHOD_TYPES.contains(&node.kind()) {
        if let Some(name_node) = node.child_by_field_name("name") {
            let function_name = node_text(&name_node, source);
            let parameters_node = node.child_by_field_name("parameters");
            let parameters = extract_parameters(parameters_node.as_ref(), source);
            let class_name = find_enclosing_class_name(node, source);
            let code = String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned();
            chunks.push(make_chunk(
                file_path,
                Some(function_name),
                class_name,
                parameters,
                true,
                node_is_async(node),
                node_is_generator(node),
                code,
                node.kind(),
                deps,
            ));
            spans.push((node.start_byte(), node.end_byte()));
        }
    }

    if node.kind() == "variable_declarator" {
        let name_node = node.child_by_field_name("name");
        let initializer = node.child_by_field_name("value");
        if let (Some(name_node), Some(init)) = (name_node, initializer) {
            if FUNCTION_EXPRESSION_TYPES.contains(&init.kind()) {
                let function_name = node_text(&name_node, source);
                let parameters_node = init.child_by_field_name("parameters");
                let parameters = extract_parameters(parameters_node.as_ref(), source);
                let code = String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned();
                chunks.push(make_chunk(
                    file_path,
                    Some(function_name),
                    None,
                    parameters,
                    false,
                    node_is_async(&init),
                    node_is_generator(&init),
                    code,
                    init.kind(),
                    deps,
                ));
                spans.push(get_declarator_removal_span(node));
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_impl(&child, source, file_path, deps, chunks, spans);
    }
}