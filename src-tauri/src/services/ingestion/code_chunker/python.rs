//! Python chunker. Direct port of `chunk_code_python.py`.

use tree_sitter::Node;

use super::helpers::{
    build_remainder_code, collect_dependencies, has_child_of_kind, new_chunk, node_text,
};
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
    let deps = collect_dependencies(root, |node, deps| {
        if node.kind() == "import_statement" {
            let mut cursor = node.walk();
            for name_node in node.children_by_field_name("name", &mut cursor) {
                deps.push(import_name(&name_node, source));
            }
        } else if matches!(
            node.kind(),
            "import_from_statement" | "future_import_statement"
        ) {
            if let Some(mn) = node.child_by_field_name("module_name") {
                deps.push(node_text(&mn, source).trim().to_string());
            } else if node.kind() == "future_import_statement" {
                deps.push("__future__".to_string());
            }
        }
    });
    deps.into_iter().filter(|dep| !dep.is_empty()).collect()
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
    let mut extractor = Extractor {
        source,
        file_path,
        deps,
        chunks: Vec::new(),
        spans: Vec::new(),
    };
    extractor.extract_scope(&node, None, false);
    let mut chunks = extractor.chunks;
    let remainder = build_remainder_code(source, &extractor.spans);
    if !remainder.trim().is_empty() {
        chunks.push(make_chunk(
            file_path,
            PythonFields::default(),
            remainder,
            "file_remainder",
            deps,
        ));
    }
    chunks
}

struct Extractor<'s> {
    source: &'s [u8],
    file_path: &'s str,
    deps: &'s [String],
    chunks: Vec<CodeChunk>,
    spans: Vec<(usize, usize)>,
}

impl Extractor<'_> {
    fn push_chunk(&mut self, fields: Option<PythonFields>, code: String, chunk_type: &str) {
        if let Some(fields) = fields {
            let chunk = make_chunk(self.file_path, fields, code, chunk_type, self.deps);
            self.chunks.push(chunk);
        }
    }

    fn push_span<'a>(&mut self, node: &Node<'a>) {
        self.spans.push((node.start_byte(), node.end_byte()));
    }

    fn extract_scope<'a>(
        &mut self,
        node: &Node<'a>,
        class_name: Option<&str>,
        is_class_body: bool,
    ) {
        let source = self.source;
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let Some((declaration, decorators, span_node)) = unwrap_declaration(&child, source)
            else {
                continue;
            };
            let kind = declaration.kind();
            if kind == FUNCTION_NODE_TYPE {
                let fields =
                    function_fields(&declaration, &decorators, source, class_name, is_class_body);
                self.push_chunk(fields, node_text(&span_node, source), kind);
                self.push_span(&span_node);
                if let Some(body) = declaration.child_by_field_name("body") {
                    self.extract_scope(&body, class_name, false);
                }
            } else if kind == CLASS_NODE_TYPE {
                let fields = class_fields(&declaration, &decorators, source);
                let code = class_header_and_properties(&declaration, &span_node, source);
                self.push_chunk(fields, code, kind);
                self.push_span(&span_node);
                let new_class_name = field_text(&declaration, "name", source);
                if let Some(body) = declaration.child_by_field_name("body") {
                    self.extract_scope(&body, new_class_name.as_deref(), true);
                }
            } else if kind == TYPE_ALIAS_NODE_TYPE {
                let fields = type_alias_fields(&declaration, &decorators, source, class_name);
                self.push_chunk(fields, node_text(&span_node, source), kind);
                self.push_span(&span_node);
            } else if is_pep_613_alias(&declaration, source) {
                let fields = pep_613_alias_fields(&declaration, source, class_name);
                let code = node_text(&declaration, source);
                self.push_chunk(fields, code, "type_alias_assignment");
                self.push_span(&child);
            } else if is_named_lambda_assignment(&declaration) {
                let fields = lambda_fields(&declaration, source, class_name);
                self.push_chunk(fields, node_text(&declaration, source), "lambda");
                self.push_span(&child);
            } else {
                self.extract_scope(&declaration, class_name, is_class_body);
            }
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

fn function_fields<'a>(
    node: &Node<'a>,
    decorators: &[String],
    source: &[u8],
    class_name: Option<&str>,
    is_method: bool,
) -> Option<PythonFields> {
    let name_node = node.child_by_field_name("name")?;
    let parameters_node = node.child_by_field_name("parameters");
    Some(PythonFields {
        function_name: Some(node_text(&name_node, source)),
        class_name: class_name.map(str::to_string),
        parameters: extract_parameters(parameters_node.as_ref(), source),
        return_type: field_text(node, "return_type", source),
        decorators: decorators.to_vec(),
        is_method,
        is_async: has_child_of_kind(node, &["async"]),
        is_generator: node
            .child_by_field_name("body")
            .map(|b| contains_yield(&b))
            .unwrap_or(false),
        type_parameters: field_text(node, "type_parameters", source),
        ..Default::default()
    })
}

fn class_fields<'a>(node: &Node<'a>, decorators: &[String], source: &[u8]) -> Option<PythonFields> {
    Some(PythonFields {
        class_name: Some(field_text(node, "name", source)?),
        properties: extract_class_properties(node, source),
        decorators: decorators.to_vec(),
        type_parameters: field_text(node, "type_parameters", source),
        bases: class_bases(node, source),
        ..Default::default()
    })
}

fn type_alias_fields<'a>(
    node: &Node<'a>,
    decorators: &[String],
    source: &[u8],
    class_name: Option<&str>,
) -> Option<PythonFields> {
    let mut cursor = node.walk();
    let types: Vec<Node<'a>> = node
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "type")
        .collect();
    if types.len() < 2 {
        return None;
    }
    Some(PythonFields {
        class_name: class_name.map(str::to_string),
        decorators: decorators.to_vec(),
        is_type_alias: true,
        alias_name: Some(type_alias_name(&types[0], source)),
        alias_target: Some(node_text(&types[types.len() - 1], source)),
        ..Default::default()
    })
}

fn pep_613_alias_fields<'a>(
    node: &Node<'a>,
    source: &[u8],
    class_name: Option<&str>,
) -> Option<PythonFields> {
    let assignment = assignment(node)?;
    let name_node = assignment.child_by_field_name("left")?;
    Some(PythonFields {
        class_name: class_name.map(str::to_string),
        is_type_alias: true,
        alias_name: Some(node_text(&name_node, source)),
        alias_target: field_text(&assignment, "right", source),
        ..Default::default()
    })
}

fn lambda_fields<'a>(
    node: &Node<'a>,
    source: &[u8],
    class_name: Option<&str>,
) -> Option<PythonFields> {
    let assignment = assignment(node)?;
    let name_node = assignment.child_by_field_name("left")?;
    let lambda_node = assignment.child_by_field_name("right")?;
    let parameters_node = lambda_node.child_by_field_name("parameters");
    Some(PythonFields {
        function_name: Some(node_text(&name_node, source)),
        class_name: class_name.map(str::to_string),
        parameters: extract_parameters(parameters_node.as_ref(), source),
        ..Default::default()
    })
}

fn extract_parameters<'a>(parameters_node: Option<&Node<'a>>, source: &[u8]) -> Vec<Parameter> {
    let Some(node) = parameters_node else {
        return Vec::new();
    };
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .map(|c| {
            Parameter::new(
                parameter_name(&c, source),
                node_text(&c, source).trim().to_string(),
            )
        })
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
                .unwrap_or_default();
            if TYPE_ALIAS_ANNOTATIONS.contains(&prop_type.trim()) {
                continue;
            }
            properties.push(Parameter::new(node_text(&left, source), prop_type));
        }
    }
    properties
}

fn class_header_and_properties<'a>(node: &Node<'a>, span_node: &Node<'a>, source: &[u8]) -> String {
    let code = node_text(span_node, source);
    let Some(body) = node.child_by_field_name("body") else {
        return code;
    };
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
                node_text(&parent, source)
            } else {
                node_text(&a, source)
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
    Some(
        raw.trim_start_matches('(')
            .trim_end_matches(')')
            .to_string(),
    )
}

fn field_text<'a>(node: &Node<'a>, field: &str, source: &[u8]) -> Option<String> {
    node.child_by_field_name(field)
        .map(|n| node_text(&n, source).trim().to_string())
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
    let Some(assignment) = assignment(node) else {
        return false;
    };
    let annotation = assignment.child_by_field_name("type");
    annotation
        .map(|a| TYPE_ALIAS_ANNOTATIONS.contains(&node_text(&a, source).trim()))
        .unwrap_or(false)
}

fn is_named_lambda_assignment<'a>(node: &Node<'a>) -> bool {
    let Some(assignment) = assignment(node) else {
        return false;
    };
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
    let Some(body) = class_node.child_by_field_name("body") else {
        return Vec::new();
    };
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

fn make_chunk(
    file_path: &str,
    fields: PythonFields,
    code: String,
    chunk_type: &str,
    deps: &[String],
) -> CodeChunk {
    let identifier = fields
        .function_name
        .clone()
        .or_else(|| fields.class_name.clone())
        .or_else(|| fields.alias_name.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    new_chunk(
        file_path,
        &identifier,
        code,
        chunk_type,
        deps,
        CodeChunkKind::Python(fields),
    )
}
