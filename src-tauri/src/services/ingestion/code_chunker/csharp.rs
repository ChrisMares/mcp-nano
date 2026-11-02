//! C# chunker. Direct port of `chunk_code_csharp.py`.

use tree_sitter::Node;

use super::helpers::{collect_dependencies, field_text, find_child_of_kind, new_chunk, node_text};
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
    collect_dependencies(root, |node, deps| {
        if node.kind() == "using_directive" {
            if let Some(dep) = normalize_csharp_using(&node_text(&node, source)) {
                deps.push(dep);
            }
        }
    })
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
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
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
    if NAMED_DECLARATION_TYPES.contains(&node.kind()) {
        if let Some(chunk) = declaration_chunk(node, source, file_path, deps) {
            chunks.push(chunk);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_impl(&child, source, file_path, deps, chunks);
    }
}

/// Chunk for one named declaration. Methods inside interfaces return `None`
/// (parity with Python).
fn declaration_chunk<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
) -> Option<CodeChunk> {
    let mut fields = CSharpFields::default();
    let mut chunk_code = node_text(node, source);
    let name = field_text(node, "name", source);

    match node.kind() {
        "method_declaration" => {
            fields.function_name = name;
            fields.return_type = field_text(node, "type", source);
            fields.parameters = parameter_list(node.child_by_field_name("parameters"), source);
            let mut current = node.parent();
            while let Some(parent) = current {
                if matches!(
                    parent.kind(),
                    "class_declaration" | "struct_declaration" | "record_declaration"
                ) {
                    fields.class_name = field_text(&parent, "name", source);
                    break;
                }
                if parent.kind() == "interface_declaration" {
                    fields.interface_name = field_text(&parent, "name", source);
                    break;
                }
                current = parent.parent();
            }
        }
        "interface_declaration" => fields.interface_name = name,
        "class_declaration" => {
            fields.class_name = name;
            let full = node_text(node, source);
            let header = match full.find('{') {
                Some(i) => full[..i].trim().to_string(),
                None => full.trim().to_string(),
            };
            let body = find_child_of_kind(node, "declaration_list");
            let declarations = property_declarations(body.as_ref().unwrap_or(node));
            fields.properties = typed_properties(&declarations, source);
            let mut property_codes: Vec<String> = Vec::new();
            for declaration in &declarations {
                property_codes.push(node_text(declaration, source));
            }
            chunk_code = format!("{header}\n{}", property_codes.join("\n"));
        }
        "record_declaration" => {
            fields.class_name = name;
            // Positional record: parameter_list is a direct child.
            if let Some(pl) = find_child_of_kind(node, "parameter_list") {
                fields.properties = parameter_list(Some(pl), source);
            } else if let Some(body) = find_child_of_kind(node, "declaration_list") {
                fields.properties = typed_properties(&property_declarations(&body), source);
            }
        }
        "delegate_declaration" => {
            fields.class_name = name;
            fields.return_type = field_text(node, "type", source);
            fields.parameters = parameter_list(node.child_by_field_name("parameters"), source);
        }
        _ => fields.class_name = name,
    }

    if fields.interface_name.is_some() && fields.function_name.is_some() {
        return None;
    }
    let identifier = fields
        .class_name
        .clone()
        .or_else(|| fields.interface_name.clone())
        .or_else(|| fields.function_name.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    Some(new_chunk(
        file_path,
        &identifier,
        chunk_code,
        node.kind(),
        deps,
        CodeChunkKind::CSharp(fields),
    ))
}

/// `parameter` children of a parameter list that have both a name and a type.
fn parameter_list<'a>(list: Option<Node<'a>>, source: &[u8]) -> Vec<Parameter> {
    let mut parameters = Vec::new();
    let Some(list) = list else {
        return parameters;
    };
    let mut cursor = list.walk();
    for pn in list.children(&mut cursor) {
        if pn.kind() == "parameter" {
            if let (Some(n), Some(t)) = (
                field_text(&pn, "name", source),
                field_text(&pn, "type", source),
            ) {
                parameters.push(Parameter::new(n, t));
            }
        }
    }
    parameters
}

fn property_declarations<'a>(parent: &Node<'a>) -> Vec<Node<'a>> {
    let mut declarations = Vec::new();
    let mut cursor = parent.walk();
    for child in parent.children(&mut cursor) {
        if child.kind() == "property_declaration" {
            declarations.push(child);
        }
    }
    declarations
}

/// Properties of `declarations` that have both a name and a type.
fn typed_properties<'a>(declarations: &[Node<'a>], source: &[u8]) -> Vec<Parameter> {
    let mut properties = Vec::new();
    for declaration in declarations {
        if let (Some(n), Some(t)) = (
            field_text(declaration, "name", source),
            field_text(declaration, "type", source),
        ) {
            properties.push(Parameter::new(n, t));
        }
    }
    properties
}
