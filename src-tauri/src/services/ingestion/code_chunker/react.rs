//! React/TSX chunker. Direct port of `chunk_code_react.py`.

use tree_sitter::Node;

use super::helpers::{
    chunks_with_remainder, field_text, get_declarator_removal_span, new_chunk, node_text,
};
use crate::services::ingestion::types::{CodeChunk, CodeChunkKind, Parameter, ReactFields};

const JSX_TYPES: &[&str] = &["jsx_element", "jsx_self_closing_element", "jsx_fragment"];

pub use super::js::extract_dependencies;

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

fn extract_props<'a>(parameters_node: Option<&Node<'a>>, source: &[u8]) -> Vec<Parameter> {
    let mut props = Vec::new();
    let Some(node) = parameters_node else {
        return props;
    };
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

/// `use*` hooks called anywhere under `node`, de-duplicated in call order.
fn collect_hooks<'a>(node: &Node<'a>, source: &[u8], hooks: &mut Vec<String>) {
    if node.kind() == "call_expression" {
        if let Some(func) = node.child_by_field_name("function") {
            if func.kind() == "identifier" {
                let name = node_text(&func, source);
                if name.starts_with("use") && !hooks.contains(&name) {
                    hooks.push(name);
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_hooks(&child, source, hooks);
    }
}

fn make_chunk(
    file_path: &str,
    fields: ReactFields,
    code: String,
    chunk_type: &str,
    deps: &[String],
) -> CodeChunk {
    let identifier = fields
        .component_name
        .clone()
        .unwrap_or_else(|| "Unknown".to_string());
    new_chunk(
        file_path,
        &identifier,
        code,
        chunk_type,
        deps,
        CodeChunkKind::React(fields),
    )
}

/// Fields for a function-like component, hook, or inner function named
/// `name`, taking props from `function`'s parameters and hooks from its body.
fn function_fields<'a>(
    function: &Node<'a>,
    name: String,
    source: &[u8],
    parent_component: Option<String>,
    is_exported: bool,
) -> ReactFields {
    let parameters_node = function.child_by_field_name("parameters");
    let mut hooks_used = Vec::new();
    if let Some(body) = function.child_by_field_name("body") {
        collect_hooks(&body, source, &mut hooks_used);
    }
    ReactFields {
        component_name: Some(name),
        is_functional: true,
        hooks_used,
        props: extract_props(parameters_node.as_ref(), source),
        is_exported,
        component_type: None,
        parent_component,
    }
}

fn extract_functional_component<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    chunks: &mut Vec<CodeChunk>,
) {
    let Some(component_name) = field_text(node, "name", source) else {
        return;
    };
    let fields = function_fields(
        node,
        component_name.clone(),
        source,
        None,
        is_exported(node),
    );
    chunks.push(make_chunk(
        file_path,
        fields,
        node_text(node, source),
        "functional_component",
        deps,
    ));
    if let Some(body) = node.child_by_field_name("body") {
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
    let Some(component_name) = field_text(declarator, "name", source) else {
        return;
    };
    let Some(initializer) = declarator.child_by_field_name("value") else {
        return;
    };
    let exported = declarator
        .parent()
        .map(|p| is_exported(&p))
        .unwrap_or(false);
    let mut fields = function_fields(&initializer, component_name.clone(), source, None, exported);
    if let Some(type_text) = field_text(declarator, "type", source) {
        if type_text.contains("PropsWithChildren") {
            fields.props.push(Parameter::new("children", "unknown"));
        }
    }
    chunks.push(make_chunk(
        file_path,
        fields,
        node_text(declarator, source),
        "functional_component",
        deps,
    ));
    if let Some(body) = initializer.child_by_field_name("body") {
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
            if let Some(func_name) = field_text(&child, "name", source) {
                let fields = function_fields(
                    &child,
                    func_name,
                    source,
                    Some(parent_component.to_string()),
                    is_exported(&child),
                );
                chunks.push(make_chunk(
                    file_path,
                    fields,
                    node_text(&child, source),
                    "function",
                    deps,
                ));
            }
        }
        if matches!(child.kind(), "lexical_declaration" | "variable_declaration") {
            let mut cur2 = child.walk();
            for declarator in child.named_children(&mut cur2) {
                if declarator.kind() != "variable_declarator" {
                    continue;
                }
                let name_node = declarator.child_by_field_name("name");
                let initializer = declarator.child_by_field_name("value");
                if let (Some(name_node), Some(initializer)) = (name_node, initializer) {
                    if matches!(initializer.kind(), "arrow_function" | "function_expression") {
                        let fields = function_fields(
                            &initializer,
                            node_text(&name_node, source),
                            source,
                            Some(parent_component.to_string()),
                            is_exported(&declarator),
                        );
                        chunks.push(make_chunk(
                            file_path,
                            fields,
                            node_text(&declarator, source),
                            "function",
                            deps,
                        ));
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
    let Some(component_name) = field_text(node, "name", source) else {
        return;
    };
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
    let fields = ReactFields {
        component_name: Some(component_name),
        props,
        is_exported: is_exported(node),
        ..Default::default()
    };
    chunks.push(make_chunk(
        file_path,
        fields,
        node_text(node, source),
        "class_component",
        deps,
    ));
}

pub fn extract_chunks<'a>(
    root: Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
) -> Vec<CodeChunk> {
    chunks_with_remainder(source, file_path, deps, |chunks, spans| {
        extract_react(&root, source, file_path, deps, chunks, spans)
    })
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
        if let Some(name) = field_text(node, "name", source) {
            let first_char = name.chars().next().unwrap_or(' ');
            if first_char.is_uppercase() && contains_jsx(node) {
                spans.push((node.start_byte(), node.end_byte()));
                extract_functional_component(node, source, file_path, deps, chunks);
            } else if name.starts_with("use") && first_char.is_lowercase() {
                spans.push((node.start_byte(), node.end_byte()));
                let fields = function_fields(node, name, source, None, is_exported(node));
                chunks.push(make_chunk(
                    file_path,
                    fields,
                    node_text(node, source),
                    "hook",
                    deps,
                ));
            }
        }
    } else if node.kind() == "class_declaration" {
        if let Some(superclass) = field_text(node, "superclass", source) {
            if superclass.contains("Component") {
                spans.push((node.start_byte(), node.end_byte()));
                extract_class_component(node, source, file_path, deps, chunks);
            }
        }
    } else if node.kind() == "lexical_declaration" {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "variable_declarator" {
                continue;
            }
            let name_node = child.child_by_field_name("name");
            let initializer = child.child_by_field_name("value");
            if let (Some(name_node), Some(init)) = (name_node, initializer) {
                let starts_upper = node_text(&name_node, source)
                    .chars()
                    .next()
                    .is_some_and(char::is_uppercase);
                if matches!(init.kind(), "arrow_function" | "function_expression")
                    && starts_upper
                    && contains_jsx(&init)
                {
                    spans.push(get_declarator_removal_span(&child));
                    extract_functional_component_from_declarator(
                        &child, source, file_path, deps, chunks,
                    );
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_react(&child, source, file_path, deps, chunks, spans);
    }
}
