//! React/TSX chunker. Direct port of `chunk_code_react.py`.

use std::path::Path;

use tree_sitter::Node;

use super::helpers::{
    build_remainder_code, generate_chunk_id, get_declarator_removal_span, node_text,
    strip_js_import_source, walk_preorder,
};
use crate::services::ingestion::types::{CodeChunk, CodeChunkKind, Parameter, ReactFields};

const JSX_TYPES: &[&str] = &["jsx_element", "jsx_self_closing_element", "jsx_fragment"];

pub fn extract_dependencies<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    let mut deps: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    walk_preorder(root, |node| {
        if node.kind() == "import_statement" || node.kind() == "import_declaration" {
            if let Some(src) = node.child_by_field_name("source") {
                let dep = strip_js_import_source(&node_text(&src, source).trim());
                if !dep.is_empty() && seen.insert(dep.clone()) {
                    deps.push(dep);
                }
            }
        }
    });
    deps
}

fn contains_jsx<'a>(node: &Node<'a>) -> bool {
    if JSX_TYPES.contains(&node.kind()) {
        return true;
    }
    let mut cursor = node.walk();
    let has = node.children(&mut cursor).any(|c| contains_jsx(&c));
    has
}

fn is_exported<'a>(node: &Node<'a>) -> bool {
    node.parent()
        .map(|p| p.kind() == "export_statement")
        .unwrap_or(false)
}

fn get_superclass_name<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    node.child_by_field_name("superclass").map(|n| node_text(&n, source))
}

fn extract_props<'a>(parameters_node: Option<&Node<'a>>, source: &[u8]) -> Vec<Parameter> {
    let mut props = Vec::new();
    let Some(node) = parameters_node else { return props };
    let mut cursor = node.walk();
    for param in node.named_children(&mut cursor) {
        match param.kind() {
            "required_parameter" | "optional_parameter" => {
                let name_node = param
                    .child_by_field_name("pattern")
                    .or_else(|| param.child_by_field_name("name"));
                let type_node = param.child_by_field_name("type");
                if let Some(n) = name_node {
                    let name = node_text(&n, source);
                    let ptype = type_node.map(|t| node_text(&t, source)).unwrap_or_default();
                    props.push(Parameter::new(name, ptype));
                }
            }
            "object_pattern" => {
                let mut cur2 = param.walk();
                for prop_node in param.named_children(&mut cur2) {
                    if prop_node.kind() == "property_identifier" {
                        let name = node_text(&prop_node, source);
                        let mut ptype = String::new();
                        if let Some(next) = prop_node.next_named_sibling() {
                            if next.kind() == "type_annotation" {
                                ptype = node_text(&next, source);
                            }
                        }
                        props.push(Parameter::new(name, ptype));
                    }
                }
            }
            _ => {}
        }
    }
    props
}

fn collect_hooks<'a>(node: &Node<'a>, source: &[u8], hooks: &mut std::collections::HashSet<String>) {
    if node.kind() == "call_expression" {
        if let Some(func) = node.child_by_field_name("function") {
            if func.kind() == "identifier" {
                let name = node_text(&func, source);
                if name.starts_with("use") {
                    hooks.insert(name);
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_hooks(&child, source, hooks);
    }
}

fn create_chunk(
    file_path: &str,
    component_name: Option<String>,
    is_functional: bool,
    props: Vec<Parameter>,
    hooks_used: Vec<String>,
    code: String,
    chunk_type: &str,
    deps: &[String],
    parent_component: Option<String>,
    is_exported: bool,
) -> CodeChunk {
    let id = generate_chunk_id(
        file_path,
        &component_name.clone().unwrap_or("Unknown".to_string()),
    );
    CodeChunk {
        id,
        repo_name: String::new(),
        file_name: Path::new(file_path).file_name().and_then(|n| n.to_str()).unwrap_or(file_path).to_string(),
        code,
        r#type: chunk_type.to_string(),
        dependencies: deps.to_vec(),
        created_at: crate::services::ingestion::types::now_iso(),
        kind: CodeChunkKind::React(ReactFields {
            component_name,
            is_functional,
            hooks_used,
            props,
            is_exported,
            component_type: None,
            parent_component,
        }),
    }
}

fn extract_functional_component<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    chunks: &mut Vec<CodeChunk>,
) {
    let Some(name_node) = node.child_by_field_name("name") else { return };
    let component_name = node_text(&name_node, source);
    let parameters_node = node.child_by_field_name("parameters");
    let props = extract_props(parameters_node.as_ref(), source);
    let body_node = node.child_by_field_name("body");
    let mut hooks_set = std::collections::HashSet::new();
    if let Some(body) = body_node {
        collect_hooks(&body, source, &mut hooks_set);
    }
    let hooks_used: Vec<String> = hooks_set.into_iter().collect();
    let code = node_text(&node, source);
    chunks.push(create_chunk(
        file_path,
        Some(component_name.clone()),
        true,
        props,
        hooks_used,
        code,
        "functional_component",
        deps,
        None,
        is_exported(node),
    ));
    if let Some(body) = body_node {
        extract_inner_functions(&body, source, file_path, &component_name, deps, chunks);
    }
}

fn extract_functional_component_from_declarator<'a>(
    declarator: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    chunks: &mut Vec<CodeChunk>,
) {
    let Some(name_node) = declarator.child_by_field_name("name") else { return };
    let component_name = node_text(&name_node, source);
    let Some(initializer) = declarator.child_by_field_name("value") else { return };
    let parameters_node = initializer.child_by_field_name("parameters");
    let mut props = extract_props(parameters_node.as_ref(), source);
    let type_node = declarator.child_by_field_name("type");
    if let Some(tn) = type_node {
        let ttext = node_text(&tn, source);
        if ttext.contains("PropsWithChildren") {
            props.push(Parameter::new("children", "unknown"));
        }
    }
    let body_node = initializer.child_by_field_name("body");
    let mut hooks_set = std::collections::HashSet::new();
    if let Some(body) = body_node {
        collect_hooks(&body, source, &mut hooks_set);
    }
    let hooks_used: Vec<String> = hooks_set.into_iter().collect();
    let code = node_text(&declarator, source);
    let exported = declarator
        .parent()
        .map(|p| is_exported(&p))
        .unwrap_or(false);
    chunks.push(create_chunk(
        file_path,
        Some(component_name.clone()),
        true,
        props,
        hooks_used,
        code,
        "functional_component",
        deps,
        None,
        exported,
    ));
    if let Some(body) = body_node {
        extract_inner_functions(&body, source, file_path, &component_name, deps, chunks);
    }
}

fn extract_inner_functions<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    parent_component: &str,
    deps: &[String],
    chunks: &mut Vec<CodeChunk>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "function_declaration" {
            if let Some(name_node) = child.child_by_field_name("name") {
                let func_name = node_text(&name_node, source);
                let parameters_node = child.child_by_field_name("parameters");
                let props = extract_props(parameters_node.as_ref(), source);
                let func_code = node_text(&child, source);
                let body_node = child.child_by_field_name("body");
                let mut hooks_set = std::collections::HashSet::new();
                if let Some(body) = body_node {
                    collect_hooks(&body, source, &mut hooks_set);
                }
                let hooks_used: Vec<String> = hooks_set.into_iter().collect();
                chunks.push(create_chunk(
                    file_path,
                    Some(func_name),
                    true,
                    props,
                    hooks_used,
                    func_code,
                    "function",
                    deps,
                    Some(parent_component.to_string()),
                    is_exported(&child),
                ));
            }
        }
        if matches!(child.kind(), "lexical_declaration" | "variable_declaration") {
            let mut cur2 = child.walk();
            for declarator in child.named_children(&mut cur2) {
                if declarator.kind() == "variable_declarator" {
                    let name_node = declarator.child_by_field_name("name");
                    let initializer = declarator.child_by_field_name("value");
                    if let (Some(name_node), Some(initializer)) = (name_node, initializer) {
                        if matches!(initializer.kind(), "arrow_function" | "function_expression") {
                            let func_name = node_text(&name_node, source);
                            let func_code = node_text(&declarator, source);
                            let parameters_node = initializer.child_by_field_name("parameters");
                            let props = extract_props(parameters_node.as_ref(), source);
                            let body_node = initializer.child_by_field_name("body");
                            let mut hooks_set = std::collections::HashSet::new();
                            if let Some(body) = body_node {
                                collect_hooks(&body, source, &mut hooks_set);
                            }
                            let hooks_used: Vec<String> = hooks_set.into_iter().collect();
                            chunks.push(create_chunk(
                                file_path,
                                Some(func_name),
                                true,
                                props,
                                hooks_used,
                                func_code,
                                "function",
                                deps,
                                Some(parent_component.to_string()),
                                is_exported(&declarator),
                            ));
                        }
                    }
                }
            }
        }
        extract_inner_functions(&child, source, file_path, parent_component, deps, chunks);
    }
}

fn extract_class_component<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    chunks: &mut Vec<CodeChunk>,
) {
    let Some(name_node) = node.child_by_field_name("name") else { return };
    let component_name = node_text(&name_node, source);
    let mut props = Vec::new();
    if let Some(tp) = node.child_by_field_name("type_parameters") {
        let mut cursor = tp.walk();
        for child in tp.named_children(&mut cursor) {
            if child.kind() == "type_identifier" {
                props.push(Parameter::new("props", node_text(&child, source)));
                break;
            }
        }
    }
    let code = node_text(&node, source);
    chunks.push(create_chunk(
        file_path,
        Some(component_name),
        false,
        props,
        Vec::new(),
        code,
        "class_component",
        deps,
        None,
        is_exported(node),
    ));
}

pub fn extract_chunks<'a>(
    root: Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
) -> Vec<CodeChunk> {
    let mut chunks = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    extract_react(&root, source, file_path, deps, &mut chunks, &mut spans);
    let remainder_code = build_remainder_code(source, &spans);
    if !remainder_code.trim().is_empty() {
        chunks.push(CodeChunk {
            id: uuid::Uuid::new_v4().to_string(),
            repo_name: String::new(),
            file_name: Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file_path)
                .to_string(),
            code: crate::services::ingestion::types::normalize_code(&remainder_code),
            r#type: "file_remainder".to_string(),
            dependencies: deps.to_vec(),
            created_at: crate::services::ingestion::types::now_iso(),
            kind: CodeChunkKind::Generic,
        });
    }
    chunks
}

fn extract_react<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    chunks: &mut Vec<CodeChunk>,
    spans: &mut Vec<(usize, usize)>,
) {
    if node.kind() == "function_declaration" {
        if let Some(name_node) = node.child_by_field_name("name") {
            let name = node_text(&name_node, source);
            let first_char = name.chars().next().unwrap_or(' ');
            if first_char.is_uppercase() && contains_jsx(node) {
                spans.push((node.start_byte(), node.end_byte()));
                extract_functional_component(node, source, file_path, deps, chunks);
            } else if name.starts_with("use") && first_char.is_lowercase() {
                spans.push((node.start_byte(), node.end_byte()));
                let parameters_node = node.child_by_field_name("parameters");
                let props = extract_props(parameters_node.as_ref(), source);
                let body_node = node.child_by_field_name("body");
                let mut hooks_set = std::collections::HashSet::new();
                if let Some(body) = body_node {
                    collect_hooks(&body, source, &mut hooks_set);
                }
                let hooks_used: Vec<String> = hooks_set.into_iter().collect();
                let code = node_text(&node, source);
                chunks.push(create_chunk(
                    file_path,
                    Some(name),
                    true,
                    props,
                    hooks_used,
                    code,
                    "hook",
                    deps,
                    None,
                    is_exported(node),
                ));
            }
        }
    } else if node.kind() == "class_declaration" {
        if let Some(superclass) = get_superclass_name(node, source) {
            if superclass.contains("Component") || superclass.ends_with(".Component") {
                spans.push((node.start_byte(), node.end_byte()));
                extract_class_component(node, source, file_path, deps, chunks);
            }
        }
    } else if node.kind() == "lexical_declaration" {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variable_declarator" {
                let name_node = child.child_by_field_name("name");
                let initializer = child.child_by_field_name("value");
                if let (Some(name_node), Some(init)) = (name_node, initializer) {
                    if matches!(init.kind(), "arrow_function" | "function_expression") {
                        let name = node_text(&name_node, source);
                        let first_char = name.chars().next().unwrap_or(' ');
                        if first_char.is_uppercase() && contains_jsx(&init) {
                            spans.push(get_declarator_removal_span(&child));
                            extract_functional_component_from_declarator(
                                &child, source, file_path, deps, chunks,
                            );
                        }
                    }
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_react(&child, source, file_path, deps, chunks, spans);
    }
}