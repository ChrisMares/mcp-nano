//! C# chunker. Direct port of `chunk_code_csharp.py`.

use std::path::Path;

use tree_sitter::Node;

use super::helpers::{generate_chunk_id, node_text};
use crate::services::ingestion::types::{CSharpFields, CodeChunk, CodeChunkKind, Parameter};

const NAMED_DECLARATION_TYPES: &[&str] = &[
    "method_declaration",
    "interface_declaration",
    "class_declaration",
    "struct_declaration",
    "record_declaration",
    "enum_declaration",
    "delegate_declaration",
];

pub fn extract_dependencies<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    let mut deps: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<Node<'a>> = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "using_directive" {
            let using_text = node_text(&node, source);
            if let Some(dep) = normalize_csharp_using(&using_text) {
                if seen.insert(dep.clone()) {
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

fn normalize_csharp_using(using_text: &str) -> Option<String> {
    let mut t = using_text.trim().to_string();
    if !t.starts_with("using") {
        return None;
    }
    t = t["using".len()..].trim().to_string();
    if let Some(stripped) = t.strip_prefix("static") {
        t = stripped.trim().to_string();
    }
    if let Some(stripped) = t.strip_suffix(';') {
        t = stripped.trim().to_string();
    }
    if let Some(idx) = t.find('=') {
        t = t[idx + 1..].trim().to_string();
    }
    if let Some(stripped) = t.strip_prefix("global::") {
        t = stripped.to_string();
    }
    if t.is_empty() { None } else { Some(t) }
}

pub fn extract_chunks<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
) -> Vec<CodeChunk> {
    let mut chunks = Vec::new();
    extract_impl(&node, source, file_path, deps, &mut chunks);
    chunks
}

fn extract_impl<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    chunks: &mut Vec<CodeChunk>,
) {
    if !NAMED_DECLARATION_TYPES.contains(&node.kind()) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            extract_impl(&child, source, file_path, deps, chunks);
        }
        return;
    }

    let mut class_name: Option<String> = None;
    let mut interface_name: Option<String> = None;
    let mut method_name: Option<String> = None;
    let mut parameters: Vec<Parameter> = Vec::new();
    let mut return_type: Option<String> = None;
    let mut properties: Vec<Parameter> = Vec::new();
    let mut chunk_code = String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned();

    match node.kind() {
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                method_name = Some(node_text(&n, source));
            }
            if let Some(t) = node.child_by_field_name("type") {
                return_type = Some(node_text(&t, source));
            }
            if let Some(pl) = node.child_by_field_name("parameters") {
                let mut cursor = pl.walk();
                for pn in pl.children(&mut cursor) {
                    if pn.kind() == "parameter" {
                        let name = pn.child_by_field_name("name").map(|n| node_text(&n, source));
                        let ty = pn.child_by_field_name("type").map(|n| node_text(&n, source));
                        if let (Some(n), Some(t)) = (name, ty) {
                            parameters.push(Parameter::new(n, t));
                        }
                    }
                }
            }
            let mut current = node.parent();
            while let Some(parent) = current {
                if matches!(parent.kind(), "class_declaration" | "struct_declaration" | "record_declaration") {
                    if let Some(cn) = parent.child_by_field_name("name") {
                        class_name = Some(node_text(&cn, source));
                    }
                    break;
                }
                if parent.kind() == "interface_declaration" {
                    if let Some(in_) = parent.child_by_field_name("name") {
                        interface_name = Some(node_text(&in_, source));
                    }
                    break;
                }
                current = parent.parent();
            }
        }
        "interface_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                interface_name = Some(node_text(&n, source));
            }
        }
        "class_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                class_name = Some(node_text(&n, source));
            }
            let full = String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned();
            let header = match full.find('{') {
                Some(i) => full[..i].trim().to_string(),
                None => full.trim().to_string(),
            };
            let body_node = {
                let mut cursor = node.walk();
                let found = node
                    .children(&mut cursor)
                    .find(|c| c.kind() == "declaration_list");
                found
            };
            let prop_parent = body_node.as_ref().unwrap_or(node);
            let mut property_codes: Vec<String> = Vec::new();
            let mut cursor = prop_parent.walk();
            for child in prop_parent.children(&mut cursor) {
                if child.kind() == "property_declaration" {
                    if let (Some(n), Some(t)) = (
                        child.child_by_field_name("name"),
                        child.child_by_field_name("type"),
                    ) {
                        properties.push(Parameter::new(node_text(&n, source), node_text(&t, source)));
                    }
                    let pc = String::from_utf8_lossy(&source[child.start_byte()..child.end_byte()]).into_owned();
                    property_codes.push(pc);
                }
            }
            chunk_code = format!("{header}\n{}", property_codes.join("\n"));
        }
        "struct_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                class_name = Some(node_text(&n, source));
            }
        }
        "record_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                class_name = Some(node_text(&n, source));
            }
            // Positional record: parameter_list is a direct child.
            let mut cursor = node.walk();
            let param_list = {
                let found = node
                    .children(&mut cursor)
                    .find(|c| c.kind() == "parameter_list");
                found
            };
            if let Some(pl) = param_list {
                let mut cur2 = pl.walk();
                for pn in pl.children(&mut cur2) {
                    if pn.kind() == "parameter" {
                        let name = pn.child_by_field_name("name").map(|n| node_text(&n, source));
                        let ty = pn.child_by_field_name("type").map(|n| node_text(&n, source));
                        if let (Some(n), Some(t)) = (name, ty) {
                            properties.push(Parameter::new(n, t));
                        }
                    }
                }
            } else {
                let body = {
                    let found = node
                        .children(&mut cursor)
                        .find(|c| c.kind() == "declaration_list");
                    found
                };
                if let Some(body) = body {
                    let mut cur2 = body.walk();
                    for child in body.children(&mut cur2) {
                        if child.kind() == "property_declaration" {
                            if let (Some(n), Some(t)) = (
                                child.child_by_field_name("name"),
                                child.child_by_field_name("type"),
                            ) {
                                properties.push(Parameter::new(node_text(&n, source), node_text(&t, source)));
                            }
                        }
                    }
                }
            }
        }
        "enum_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                class_name = Some(node_text(&n, source));
            }
        }
        "delegate_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                class_name = Some(node_text(&n, source));
            }
            if let Some(t) = node.child_by_field_name("type") {
                return_type = Some(node_text(&t, source));
            }
            if let Some(pl) = node.child_by_field_name("parameters") {
                let mut cursor = pl.walk();
                for pn in pl.children(&mut cursor) {
                    if pn.kind() == "parameter" {
                        let name = pn.child_by_field_name("name").map(|n| node_text(&n, source));
                        let ty = pn.child_by_field_name("type").map(|n| node_text(&n, source));
                        if let (Some(n), Some(t)) = (name, ty) {
                            parameters.push(Parameter::new(n, t));
                        }
                    }
                }
            }
        }
        _ => {}
    }

    // Functions inside interfaces are not added (parity with Python).
    if interface_name.is_some() && method_name.is_some() {
        // continue descending into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            extract_impl(&child, source, file_path, deps, chunks);
        }
        return;
    }

    let identifier = class_name
        .clone()
        .or(interface_name.clone())
        .or(method_name.clone())
        .unwrap_or("Unknown".to_string());
    let chunk_id = generate_chunk_id(file_path, &identifier);
    let chunk = CodeChunk {
        id: chunk_id,
        repo_name: String::new(),
        file_name: Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(file_path)
            .to_string(),
        code: chunk_code,
        r#type: node.kind().to_string(),
        dependencies: deps.to_vec(),
        created_at: crate::services::ingestion::types::now_iso(),
        kind: CodeChunkKind::CSharp(CSharpFields {
            class_name,
            interface_name,
            function_name: method_name,
            parameters,
            return_type,
            properties,
        }),
    };
    chunks.push(chunk);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_impl(&child, source, file_path, deps, chunks);
    }
}