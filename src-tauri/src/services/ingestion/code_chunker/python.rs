//! Python chunker. Direct port of `chunk_code_python.py`.

use std::path::Path;

use tree_sitter::Node;

use super::helpers::{build_remainder_code, generate_chunk_id, node_text};
use crate::services::ingestion::types::{CodeChunk, CodeChunkKind, Parameter, PythonFields};

const FUNCTION_NODE_TYPE: &str = "function_definition";
const CLASS_NODE_TYPE: &str = "class_definition";
const TYPE_ALIAS_NODE_TYPE: &str = "type_alias_statement";
const TYPE_ALIAS_ANNOTATIONS: &[&str] = &["TypeAlias", "typing.TypeAlias"];
const DECLARATION_NODE_TYPES: &[&str] = &[
    FUNCTION_NODE_TYPE,
    CLASS_NODE_TYPE,
    TYPE_ALIAS_NODE_TYPE,
    "decorated_definition",
];

pub fn extract_dependencies<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    let mut deps: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let add = |dep: String, deps: &mut Vec<String>, seen: &mut std::collections::HashSet<String>| {
        if !dep.is_empty() && seen.insert(dep.clone()) {
            deps.push(dep);
        }
    };
    let mut stack: Vec<Node<'a>> = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "import_statement" {
            let mut cursor = node.walk();
            for name_node in node.children_by_field_name("name", &mut cursor) {
                add(import_name(&name_node, source), &mut deps, &mut seen);
            }
        } else if matches!(node.kind(), "import_from_statement" | "future_import_statement") {
            if let Some(mn) = node.child_by_field_name("module_name") {
                add(node_text(&mn, source).trim().to_string(), &mut deps, &mut seen);
            } else if node.kind() == "future_import_statement" {
                add("__future__".to_string(), &mut deps, &mut seen);
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    deps
}

fn import_name<'a>(node: &Node<'a>, source: &[u8]) -> String {
    if node.kind() == "aliased_import" {
        if let Some(n) = node.child_by_field_name("name") {
            return node_text(&n, source).trim().to_string();
        }
    }
    node_text(node, source).trim().to_string()
}

pub fn extract_chunks<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
) -> Vec<CodeChunk> {
    let mut chunks: Vec<CodeChunk> = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    extract_scope(&node, source, file_path, deps, None, false, &mut chunks, &mut spans);
    let remainder = build_remainder_code(source, &spans);
    if !remainder.trim().is_empty() {
        chunks.push(CodeChunk {
            id: uuid::Uuid::new_v4().to_string(),
            repo_name: String::new(),
            file_name: Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file_path)
                .to_string(),
            code: crate::services::ingestion::types::normalize_code(&remainder),
            r#type: "file_remainder".to_string(),
            dependencies: deps.to_vec(),
            created_at: crate::services::ingestion::types::now_iso(),
            kind: CodeChunkKind::Generic,
        });
    }
    chunks
}

fn extract_scope<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    class_name: Option<&str>,
    is_class_body: bool,
    chunks: &mut Vec<CodeChunk>,
    spans: &mut Vec<(usize, usize)>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let Some((declaration, decorators, span_node)) = unwrap_declaration(&child, source) else {
            continue;
        };

        if declaration.kind() == FUNCTION_NODE_TYPE {
            append_function_chunk(
                &declaration,
                &span_node,
                &decorators,
                source,
                file_path,
                deps,
                class_name,
                is_class_body,
                chunks,
            );
            spans.push((span_node.start_byte(), span_node.end_byte()));
            if let Some(body) = declaration.child_by_field_name("body") {
                extract_scope(&body, source, file_path, deps, class_name, false, chunks, spans);
            }
        } else if declaration.kind() == CLASS_NODE_TYPE {
            append_class_chunk(
                &declaration,
                &span_node,
                &decorators,
                source,
                file_path,
                deps,
                chunks,
            );
            spans.push((span_node.start_byte(), span_node.end_byte()));
            let new_class_name = field_text(&declaration, "name", source);
            if let Some(body) = declaration.child_by_field_name("body") {
                extract_scope(
                    &body,
                    source,
                    file_path,
                    deps,
                    new_class_name.as_deref(),
                    true,
                    chunks,
                    spans,
                );
            }
        } else if declaration.kind() == TYPE_ALIAS_NODE_TYPE {
            append_type_alias_chunk(
                &declaration,
                &span_node,
                &decorators,
                source,
                file_path,
                deps,
                class_name,
                chunks,
            );
            spans.push((span_node.start_byte(), span_node.end_byte()));
        } else if is_pep_613_alias(&declaration, source) {
            append_pep_613_alias_chunk(&declaration, source, file_path, deps, class_name, chunks);
            spans.push((child.start_byte(), child.end_byte()));
        } else if is_named_lambda_assignment(&declaration, source) {
            append_lambda_chunk(&declaration, source, file_path, deps, class_name, chunks);
            spans.push((child.start_byte(), child.end_byte()));
        } else {
            extract_scope(
                &declaration,
                source,
                file_path,
                deps,
                class_name,
                is_class_body,
                chunks,
                spans,
            );
        }
    }
}

fn unwrap_declaration<'a>(
    node: &Node<'a>,
    source: &[u8],
) -> Option<(Node<'a>, Vec<String>, Node<'a>)> {
    if node.kind() != "decorated_definition" {
        return Some((*node, Vec::new(), *node));
    }
    let definition = node.child_by_field_name("definition")?;
    let mut cursor = node.walk();
    let decorators: Vec<String> = node
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "decorator")
        .map(|c| node_text(&c, source).trim().to_string())
        .collect();
    Some((definition, decorators, *node))
}

fn append_function_chunk<'a>(
    node: &Node<'a>,
    span_node: &Node<'a>,
    decorators: &[String],
    source: &[u8],
    file_path: &str,
    deps: &[String],
    class_name: Option<&str>,
    is_method: bool,
    chunks: &mut Vec<CodeChunk>,
) {
    let Some(name_node) = node.child_by_field_name("name") else { return };
    let function_name = node_text(&name_node, source);
    let parameters_node = node.child_by_field_name("parameters");
    let parameters = extract_parameters(parameters_node.as_ref(), source);
    let return_type = field_text(node, "return_type", source);
    let is_async = {
        let mut cursor = node.walk();
        let v = node.children(&mut cursor).any(|c| c.kind() == "async");
        v
    };
    let is_generator = node
        .child_by_field_name("body")
        .map(|b| contains_yield(&b))
        .unwrap_or(false);
    let type_parameters = field_text(node, "type_parameters", source);
    let code = source_text(span_node, source);
    chunks.push(make_chunk(
        file_path,
        Some(function_name),
        class_name.map(|s| s.to_string()),
        parameters,
        return_type,
        Vec::new(),
        decorators.to_vec(),
        is_method,
        is_async,
        is_generator,
        false,
        None,
        None,
        type_parameters,
        None,
        code,
        node.kind(),
        deps,
    ));
}

fn append_class_chunk<'a>(
    node: &Node<'a>,
    span_node: &Node<'a>,
    decorators: &[String],
    source: &[u8],
    file_path: &str,
    deps: &[String],
    chunks: &mut Vec<CodeChunk>,
) {
    let Some(class_name) = field_text(node, "name", source) else { return };
    let properties = extract_class_properties(node, source);
    let code = class_header_and_properties(node, span_node, source);
    let type_parameters = field_text(node, "type_parameters", source);
    let bases = class_bases(node, source);
    chunks.push(make_chunk(
        file_path,
        None,
        Some(class_name),
        Vec::new(),
        None,
        properties,
        decorators.to_vec(),
        false,
        false,
        false,
        false,
        None,
        None,
        type_parameters,
        bases,
        code,
        node.kind(),
        deps,
    ));
}

fn append_type_alias_chunk<'a>(
    node: &Node<'a>,
    span_node: &Node<'a>,
    decorators: &[String],
    source: &[u8],
    file_path: &str,
    deps: &[String],
    class_name: Option<&str>,
    chunks: &mut Vec<CodeChunk>,
) {
    let mut cursor = node.walk();
    let types: Vec<Node<'a>> = node
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "type")
        .collect();
    if types.len() < 2 {
        return;
    }
    let name = type_alias_name(&types[0], source);
    let target = node_text(&types[types.len() - 1], source);
    let code = source_text(span_node, source);
    chunks.push(make_chunk(
        file_path,
        None,
        class_name.map(|s| s.to_string()),
        Vec::new(),
        None,
        Vec::new(),
        decorators.to_vec(),
        false,
        false,
        false,
        true,
        Some(name),
        Some(target),
        None,
        None,
        code,
        node.kind(),
        deps,
    ));
}

fn append_pep_613_alias_chunk<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    class_name: Option<&str>,
    chunks: &mut Vec<CodeChunk>,
) {
    let Some(assignment) = assignment(node) else { return };
    let Some(name_node) = assignment.child_by_field_name("left") else { return };
    let name = node_text(&name_node, source);
    let target = field_text(&assignment, "right", source);
    let code = source_text(node, source);
    chunks.push(make_chunk(
        file_path,
        None,
        class_name.map(|s| s.to_string()),
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        false,
        false,
        false,
        true,
        Some(name),
        target,
        None,
        None,
        code,
        "type_alias_assignment",
        deps,
    ));
}

fn append_lambda_chunk<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    class_name: Option<&str>,
    chunks: &mut Vec<CodeChunk>,
) {
    let Some(assignment) = assignment(node) else { return };
    let Some(name_node) = assignment.child_by_field_name("left") else { return };
    let Some(lambda_node) = assignment.child_by_field_name("right") else { return };
    let function_name = node_text(&name_node, source);
    let parameters_node = lambda_node.child_by_field_name("parameters");
    let parameters = extract_parameters(parameters_node.as_ref(), source);
    let code = source_text(node, source);
    chunks.push(make_chunk(
        file_path,
        Some(function_name),
        class_name.map(|s| s.to_string()),
        parameters,
        None,
        Vec::new(),
        Vec::new(),
        false,
        false,
        false,
        false,
        None,
        None,
        None,
        None,
        code,
        "lambda",
        deps,
    ));
}

fn extract_parameters<'a>(parameters_node: Option<&Node<'a>>, source: &[u8]) -> Vec<Parameter> {
    let Some(node) = parameters_node else { return Vec::new() };
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .map(|c| Parameter::new(parameter_name(&c, source), node_text(&c, source).trim().to_string()))
        .collect()
}

fn parameter_name<'a>(node: &Node<'a>, source: &[u8]) -> String {
    if matches!(node.kind(), "positional_separator" | "keyword_separator") {
        return node_text(node, source);
    }
    if let Some(name) = node.child_by_field_name("name") {
        return node_text(&name, source);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "type" {
            let name = parameter_name(&child, source);
            if !name.is_empty() {
                return name;
            }
        }
    }
    node_text(node, source)
}

fn extract_class_properties<'a>(class_node: &Node<'a>, source: &[u8]) -> Vec<Parameter> {
    let mut properties = Vec::new();
    for assignment in class_property_assignments(class_node) {
        if let Some(left) = assignment.child_by_field_name("left") {
            if left.kind() != "identifier" {
                continue;
            }
            let annotation = assignment.child_by_field_name("type");
            let prop_type = annotation
                .map(|a| node_text(&a, source))
                .filter(|s| !TYPE_ALIAS_ANNOTATIONS.contains(&s.trim()))
                .unwrap_or_default();
            properties.push(Parameter::new(node_text(&left, source), prop_type));
        }
    }
    properties
}

fn class_header_and_properties<'a>(node: &Node<'a>, span_node: &Node<'a>, source: &[u8]) -> String {
    let code = source_text(span_node, source);
    let Some(body) = node.child_by_field_name("body") else { return code };
    let body_start = body.start_byte() - span_node.start_byte();
    let header = code[..body_start].trim_end().to_string();
    let properties = extract_class_properties(node, source);
    if properties.is_empty() {
        return header;
    }
    let property_lines: Vec<String> = class_property_assignments(node)
        .into_iter()
        .filter(|a| is_class_property_assignment(a, source))
        .map(|a| {
            if let Some(parent) = a.parent() {
                source_text(&parent, source)
            } else {
                source_text(&a, source)
            }
        })
        .collect();
    let mut parts: Vec<String> = vec![header];
    parts.extend(property_lines);
    parts.join("\n")
}

fn class_bases<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    let superclasses = node.child_by_field_name("superclasses")?;
    let raw = node_text(&superclasses, source).trim().to_string();
    Some(raw.trim_start_matches('(').trim_end_matches(')').to_string())
}

fn field_text<'a>(node: &Node<'a>, field: &str, source: &[u8]) -> Option<String> {
    node.child_by_field_name(field).map(|n| node_text(&n, source).trim().to_string())
}

fn contains_yield<'a>(node: &Node<'a>) -> bool {
    if DECLARATION_NODE_TYPES.contains(&node.kind()) {
        return false;
    }
    if node.kind() == "yield" {
        return true;
    }
    let mut cursor = node.walk();
    let has_yield = node.children(&mut cursor).any(|c| contains_yield(&c));
    has_yield
}

fn is_pep_613_alias<'a>(node: &Node<'a>, source: &[u8]) -> bool {
    let Some(assignment) = assignment(node) else { return false };
    let annotation = assignment.child_by_field_name("type");
    annotation
        .map(|a| TYPE_ALIAS_ANNOTATIONS.contains(&node_text(&a, source).trim()))
        .unwrap_or(false)
}

fn is_named_lambda_assignment<'a>(node: &Node<'a>, _source: &[u8]) -> bool {
    let Some(assignment) = assignment(node) else { return false };
    let left = assignment.child_by_field_name("left");
    let right = assignment.child_by_field_name("right");
    left.map(|n| n.kind() == "identifier").unwrap_or(false)
        && right.map(|n| n.kind() == "lambda").unwrap_or(false)
}

fn assignment<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    if node.kind() != "expression_statement" {
        return None;
    }
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .find(|c| c.kind() == "assignment");
    found
}

fn class_property_assignments<'a>(class_node: &Node<'a>) -> Vec<Node<'a>> {
    let Some(body) = class_node.child_by_field_name("body") else { return Vec::new() };
    let mut assignments = Vec::new();
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        if let Some(a) = assignment(&child) {
            assignments.push(a);
        }
    }
    assignments
}

fn is_class_property_assignment<'a>(assignment: &Node<'a>, source: &[u8]) -> bool {
    let left = assignment.child_by_field_name("left");
    let annotation = assignment.child_by_field_name("type");
    left.map(|n| n.kind() == "identifier").unwrap_or(false)
        && (annotation.is_none()
            || !TYPE_ALIAS_ANNOTATIONS.contains(&node_text(&annotation.unwrap(), source).trim()))
}

fn type_alias_name<'a>(node: &Node<'a>, source: &[u8]) -> String {
    let mut cur = node.walk();
    if let Some(first) = node.named_children(&mut cur).next() {
        let mut cur2 = first.walk();
        if let Some(inner) = first.named_children(&mut cur2).next() {
            return node_text(&inner, source);
        }
        return node_text(&first, source);
    }
    node_text(node, source)
}

fn source_text<'a>(node: &Node<'a>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned()
}

#[allow(clippy::too_many_arguments)]
fn make_chunk(
    file_path: &str,
    function_name: Option<String>,
    class_name: Option<String>,
    parameters: Vec<Parameter>,
    return_type: Option<String>,
    properties: Vec<Parameter>,
    decorators: Vec<String>,
    is_method: bool,
    is_async: bool,
    is_generator: bool,
    is_type_alias: bool,
    alias_name: Option<String>,
    alias_target: Option<String>,
    type_parameters: Option<String>,
    bases: Option<String>,
    code: String,
    chunk_type: &str,
    deps: &[String],
) -> CodeChunk {
    let id = generate_chunk_id(
        file_path,
        &function_name
            .clone()
            .or(class_name.clone())
            .or(alias_name.clone())
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
        kind: CodeChunkKind::Python(PythonFields {
            function_name,
            class_name,
            parameters,
            return_type,
            properties,
            decorators,
            is_method,
            is_async,
            is_generator,
            is_type_alias,
            alias_name,
            alias_target,
            type_parameters,
            bases,
        }),
    }
}