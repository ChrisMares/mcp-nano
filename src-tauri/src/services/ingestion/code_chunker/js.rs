//! JavaScript chunker. Direct port of `chunk_code_js.py`.
//!
//! Extracts `function_declaration`/`generator_function_declaration` and
//! `method_definition` nodes. Variable declarators bound to a function
//! expression produce a separate chunk whose span is the whole enclosing
//! declaration (mirrors Python's `get_declarator_removal_span`).
//!
//! The TypeScript chunker shares this walk via [`Dialect::TypeScript`].

use tree_sitter::Node;

use super::helpers::{
    chunks_with_remainder, collect_dependencies, get_declarator_removal_span, has_child_of_kind,
    new_chunk, node_text, strip_js_import_source,
};
use crate::services::ingestion::types::{CodeChunk, CodeChunkKind, JavaScriptFields, Parameter};

const FUNCTION_DECLARATION_TYPES: &[&str] =
    &["function_declaration", "generator_function_declaration"];
const FUNCTION_EXPRESSION_TYPES: &[&str] = &[
    "arrow_function",
    "function_expression",
    "generator_function",
];
const METHOD_TYPES: &[&str] = &["method_definition"];

/// Import sources, shared by the JavaScript, TypeScript, and React chunkers.
pub fn extract_dependencies<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    collect_dependencies(root, |node, deps| {
        if matches!(node.kind(), "import_statement" | "import_declaration") {
            if let Some(src) = node.child_by_field_name("source") {
                let dep = strip_js_import_source(node_text(&src, source).trim());
                if !dep.is_empty() {
                    deps.push(dep);
                }
            }
        }
    })
}

/// Grammar-specific parts of the JavaScript/TypeScript chunk walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Dialect {
    JavaScript,
    TypeScript,
}

impl Dialect {
    fn parameters<'a>(self, parameters_node: Option<&Node<'a>>, source: &[u8]) -> Vec<Parameter> {
        match self {
            Dialect::JavaScript => extract_parameters(parameters_node, source),
            Dialect::TypeScript => super::ts::extract_parameters(parameters_node, source),
        }
    }

    fn return_type<'a>(
        self,
        node: &Node<'a>,
        parameters_node: Option<&Node<'a>>,
        source: &[u8],
    ) -> Option<String> {
        match self {
            Dialect::JavaScript => None,
            Dialect::TypeScript => super::ts::get_return_type(node, parameters_node, source),
        }
    }

    fn chunk(
        self,
        file_path: &str,
        fields: JavaScriptFields,
        code: String,
        chunk_type: &str,
        deps: &[String],
    ) -> CodeChunk {
        let identifier = fields
            .function_name
            .clone()
            .or_else(|| fields.class_name.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        let kind = match self {
            Dialect::JavaScript => CodeChunkKind::JavaScript(fields),
            Dialect::TypeScript => CodeChunkKind::TypeScript(fields),
        };
        new_chunk(file_path, &identifier, code, chunk_type, deps, kind)
    }
}

fn extract_parameters<'a>(parameters_node: Option<&Node<'a>>, source: &[u8]) -> Vec<Parameter> {
    let mut params = Vec::new();
    let Some(node) = parameters_node else {
        return params;
    };
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
    matches!(node.kind(), "async_function" | "async_function_declaration")
        || has_child_of_kind(node, &["async"])
}

fn node_is_generator<'a>(node: &Node<'a>) -> bool {
    matches!(
        node.kind(),
        "generator_function" | "generator_function_declaration"
    ) || has_child_of_kind(node, &["*"])
}

fn find_enclosing_class_name<'a>(node: &Node<'a>, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(parent.kind(), "class_declaration" | "class") {
            return parent
                .child_by_field_name("name")
                .map(|name| node_text(&name, source));
        }
        current = parent.parent();
    }
    None
}

fn function_fields<'a>(
    function: &Node<'a>,
    function_name: String,
    class_name: Option<String>,
    is_method: bool,
    source: &[u8],
    dialect: Dialect,
) -> JavaScriptFields {
    let parameters_node = function.child_by_field_name("parameters");
    JavaScriptFields {
        function_name: Some(function_name),
        class_name,
        parameters: dialect.parameters(parameters_node.as_ref(), source),
        return_type: dialect.return_type(function, parameters_node.as_ref(), source),
        is_method,
        is_async: node_is_async(function),
        is_generator: node_is_generator(function),
    }
}

pub fn extract_chunks<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
) -> Vec<CodeChunk> {
    extract_dialect_chunks(node, source, file_path, deps, Dialect::JavaScript)
}

pub(super) fn extract_dialect_chunks<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    dialect: Dialect,
) -> Vec<CodeChunk> {
    chunks_with_remainder(source, file_path, deps, |chunks, spans| {
        extract_impl(&node, source, file_path, deps, dialect, chunks, spans)
    })
}

fn extract_impl<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    dialect: Dialect,
    chunks: &mut Vec<CodeChunk>,
    spans: &mut Vec<(usize, usize)>,
) {
    let kind = node.kind();
    let is_method = METHOD_TYPES.contains(&kind);
    if is_method || FUNCTION_DECLARATION_TYPES.contains(&kind) {
        if let Some(name_node) = node.child_by_field_name("name") {
            let class_name = if is_method {
                find_enclosing_class_name(node, source)
            } else {
                None
            };
            let name = node_text(&name_node, source);
            let fields = function_fields(node, name, class_name, is_method, source, dialect);
            chunks.push(dialect.chunk(file_path, fields, node_text(node, source), kind, deps));
            spans.push((node.start_byte(), node.end_byte()));
        }
    }

    if kind == "variable_declarator" {
        let name_node = node.child_by_field_name("name");
        let initializer = node.child_by_field_name("value");
        if let (Some(name_node), Some(init)) = (name_node, initializer) {
            if FUNCTION_EXPRESSION_TYPES.contains(&init.kind()) {
                let name = node_text(&name_node, source);
                let fields = function_fields(&init, name, None, false, source, dialect);
                let code = node_text(node, source);
                chunks.push(dialect.chunk(file_path, fields, code, init.kind(), deps));
                spans.push(get_declarator_removal_span(node));
            }
        }
    }

    if dialect == Dialect::TypeScript && super::ts::TYPE_DECLARATION_TYPES.contains(&kind) {
        if let Some(name_node) = node.child_by_field_name("name") {
            let fields = JavaScriptFields {
                class_name: Some(node_text(&name_node, source)),
                ..Default::default()
            };
            chunks.push(dialect.chunk(file_path, fields, node_text(node, source), kind, deps));
            spans.push((node.start_byte(), node.end_byte()));
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_impl(&child, source, file_path, deps, dialect, chunks, spans);
    }
}
