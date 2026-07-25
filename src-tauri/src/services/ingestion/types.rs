//! Chunk data types shared across the ingestion pipeline.
//!
//! Mirrors the Python `custom_types/CodeChunk.py` and
//! `custom_types/DocumentChunk.py`. Each language-specific `CodeChunk`
//! variant carries the same field set as the Python dataclass, and exposes
//! `chunk_metadata` / `chunk_embedding_text` so the ingestion pipeline can
//! serialize payloads and build embedding text uniformly.

use std::collections::BTreeMap;

use chrono::Utc;
use serde_json::{Map, Value};

/// `{name, type}` pair used by every language variant to describe function
/// parameters and class/struct properties. Mirrors Python `Parameter` /
/// `Property` dataclasses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parameter {
    pub name: String,
    pub r#type: String,
}

impl Parameter {
    pub fn new(name: impl Into<String>, r#type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            r#type: r#type.into(),
        }
    }
}

/// Format a parameter list as `"<type> <name>, <type> <name>"` (the C/C++/
/// Java/C#/Rust convention). Empty types render as just the name. Used by
/// the metadata serializer and embedding text builder.
pub fn format_params_typed(params: &[Parameter]) -> String {
    params
        .iter()
        .map(|p| {
            let t = p.r#type.trim();
            let n = p.name.trim();
            if t.is_empty() {
                n.to_string()
            } else {
                format!("{t} {n}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Format a parameter list as `"<type>, <type>"` (the Python convention,
/// where parameter *types* include the name in the annotation).
pub fn format_param_types(params: &[Parameter]) -> String {
    params
        .iter()
        .map(|p| p.r#type.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Format property list as `"<type> <name>, <type> <name>"`.
pub fn format_props_typed(props: &[Parameter]) -> String {
    format_params_typed(props)
}

/// Format property list as `"<name>: <type>, <name>: <type>"`. Used by the
/// Python chunker's metadata output.
pub fn format_props_python(props: &[Parameter]) -> String {
    props
        .iter()
        .map(|p| {
            let t = p.r#type.trim();
            let n = p.name.trim();
            if t.is_empty() {
                n.to_string()
            } else {
                format!("{n}: {t}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Language-specific `CodeChunk` payload. One variant per Python dataclass
/// subclass; field names and shapes match the reference implementation so
/// emitted Qdrant metadata is byte-compatible.
///
/// The base fields (`id`, `repo_name`, `file_name`, `code`, `type`,
/// `dependencies`, `created_at`) live on the wrapping [`CodeChunk`] struct
/// instead of being repeated inside every variant.
#[derive(Debug, Clone, PartialEq)]
pub enum CodeChunkKind {
    Generic,
    C(CFields),
    Cpp(CppFields),
    CSharp(CSharpFields),
    Java(JavaFields),
    JavaScript(JavaScriptFields),
    TypeScript(TypeScriptFields),
    React(ReactFields),
    Rust(RustFields),
    Python(PythonFields),
    Sql(SqlFields),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CFields {
    pub function_name: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CppFields {
    pub function_name: Option<String>,
    pub class_name: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CSharpFields {
    pub class_name: Option<String>,
    pub interface_name: Option<String>,
    pub function_name: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub properties: Vec<Parameter>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct JavaFields {
    pub function_name: Option<String>,
    pub class_name: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct JavaScriptFields {
    pub function_name: Option<String>,
    pub class_name: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub is_method: bool,
    pub is_async: bool,
    pub is_generator: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TypeScriptFields {
    pub function_name: Option<String>,
    pub class_name: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub is_method: bool,
    pub is_async: bool,
    pub is_generator: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReactFields {
    pub component_name: Option<String>,
    pub is_functional: bool,
    pub hooks_used: Vec<String>,
    pub props: Vec<Parameter>,
    pub is_exported: bool,
    pub component_type: Option<String>,
    pub parent_component: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RustFields {
    pub function_name: Option<String>,
    pub class_name: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PythonFields {
    pub function_name: Option<String>,
    pub class_name: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub properties: Vec<Parameter>,
    pub decorators: Vec<String>,
    pub is_method: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub is_type_alias: bool,
    pub alias_name: Option<String>,
    pub alias_target: Option<String>,
    pub type_parameters: Option<String>,
    pub bases: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SqlFields {
    pub statement_type: Option<String>,
    pub object_name: Option<String>,
    pub schema_name: Option<String>,
    pub database_name: Option<String>,
}

/// Full code chunk: base metadata + language-specific payload.
///
/// The base metadata mirrors the shared `CodeChunk` dataclass fields
/// (`id`, `repo_name`, `file_name`, `code`, `type`, `dependencies`,
/// `created_at`). The `kind` field carries the per-language extension.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeChunk {
    pub id: String,
    pub repo_name: String,
    pub file_name: String,
    pub code: String,
    pub r#type: String,
    pub dependencies: Vec<String>,
    pub created_at: String,
    pub kind: CodeChunkKind,
}

impl CodeChunk {
    /// Construct a generic chunk (no language-specific metadata).
    pub fn generic(
        id: impl Into<String>,
        repo_name: impl Into<String>,
        file_name: impl Into<String>,
        code: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            repo_name: repo_name.into(),
            file_name: file_name.into(),
            code: code.into(),
            r#type: "generic".to_string(),
            dependencies: Vec::new(),
            created_at: now_iso(),
            kind: CodeChunkKind::Generic,
        }
    }

    /// Returns `true` if this chunk is the file remainder.
    pub fn is_file_remainder(&self) -> bool {
        self.r#type == "file_remainder"
    }

    /// Replace `id` and `code`, preserving every other field (used by the
    /// `split_oversized_code_chunks` pipeline). Matches Python `CodeChunk.split`.
    pub fn split(&self, new_id: impl Into<String>, new_code: impl Into<String>) -> Self {
        let mut copy = self.clone();
        copy.id = new_id.into();
        copy.code = normalize_code(&new_code.into());
        copy
    }

    /// Per-language metadata fields only (no base fields). Used by
    /// `chunk_metadata` to merge the language-specific payload on top of
    /// the base metadata map.
    pub fn kind_metadata(&self) -> Map<String, Value> {
        match &self.kind {
            CodeChunkKind::Generic => Map::new(),
            CodeChunkKind::C(f) => c_metadata(f),
            CodeChunkKind::Cpp(f) => cpp_metadata(f),
            CodeChunkKind::CSharp(f) => csharp_metadata(f),
            CodeChunkKind::Java(f) => java_metadata(f),
            CodeChunkKind::JavaScript(f) => js_metadata(f),
            CodeChunkKind::TypeScript(f) => ts_metadata(f),
            CodeChunkKind::React(f) => react_metadata(f),
            CodeChunkKind::Rust(f) => rust_metadata(f),
            CodeChunkKind::Python(f) => python_metadata(f),
            CodeChunkKind::Sql(f) => sql_metadata(f),
        }
    }

    /// Per-language middle lines for the embedding text. Matches the Python
    /// `_get_embedding_parts` virtual method.
    pub fn kind_embedding_parts(&self) -> Vec<String> {
        match &self.kind {
            CodeChunkKind::Generic => Vec::new(),
            CodeChunkKind::C(f) => c_embedding_parts(f),
            CodeChunkKind::Cpp(f) => cpp_embedding_parts(f),
            CodeChunkKind::CSharp(f) => csharp_embedding_parts(f),
            CodeChunkKind::Java(f) => java_embedding_parts(f),
            CodeChunkKind::JavaScript(f) => js_embedding_parts(f),
            CodeChunkKind::TypeScript(f) => ts_embedding_parts(f),
            CodeChunkKind::React(f) => react_embedding_parts(f),
            CodeChunkKind::Rust(f) => rust_embedding_parts(f),
            CodeChunkKind::Python(f) => python_embedding_parts(f),
            CodeChunkKind::Sql(f) => sql_embedding_parts(f),
        }
    }

    /// Full metadata payload written to Qdrant. Concatenates base fields
    /// with the per-language extension. Matches Python `CodeChunk.get_chunk_metadata`.
    pub fn chunk_metadata(&self) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("repo_name".into(), Value::String(self.repo_name.clone()));
        map.insert("file_name".into(), Value::String(self.file_name.clone()));
        map.insert("type".into(), Value::String(self.r#type.clone()));
        map.insert("code".into(), Value::String(self.code.clone()));
        map.insert(
            "dependencies".into(),
            Value::String(self.dependencies.join(", ")),
        );
        map.insert("created_at".into(), Value::String(self.created_at.clone()));
        for (k, v) in self.kind_metadata() {
            map.insert(k, v);
        }
        map
    }

    /// Embedding text built from base + per-language parts. Matches Python
    /// `CodeChunk.get_chunk_embedding_text`.
    pub fn chunk_embedding_text(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.push(format!("Repo Name: {}", self.repo_name));
        parts.push(format!("File Name: {}", self.file_name));
        for p in self.kind_embedding_parts() {
            if !p.is_empty() {
                parts.push(p);
            }
        }
        parts.push(format!("Code:\n{}", self.code));
        if !self.dependencies.is_empty() {
            parts.push(format!("Dependencies: {}", self.dependencies.join(", ")));
        }
        parts.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
    }
}

// --- metadata helpers -------------------------------------------------------

fn str_or_empty(s: &Option<String>) -> Value {
    Value::String(s.clone().unwrap_or_default())
}

fn c_metadata(f: &CFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("function_name".into(), str_or_empty(&f.function_name));
    m.insert(
        "parameters".into(),
        Value::String(format_params_typed(&f.parameters)),
    );
    m.insert("return_type".into(), str_or_empty(&f.return_type));
    m.insert("namespace".into(), str_or_empty(&f.namespace));
    m
}

fn cpp_metadata(f: &CppFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("class_name".into(), str_or_empty(&f.class_name));
    m.insert("function_name".into(), str_or_empty(&f.function_name));
    m.insert(
        "parameters".into(),
        Value::String(format_params_typed(&f.parameters)),
    );
    m.insert("return_type".into(), str_or_empty(&f.return_type));
    m.insert("namespace".into(), str_or_empty(&f.namespace));
    m
}

fn csharp_metadata(f: &CSharpFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("class_name".into(), str_or_empty(&f.class_name));
    m.insert("interface_name".into(), str_or_empty(&f.interface_name));
    m.insert("function_name".into(), str_or_empty(&f.function_name));
    m.insert(
        "parameters".into(),
        Value::String(format_params_typed(&f.parameters)),
    );
    m.insert("return_type".into(), str_or_empty(&f.return_type));
    m.insert(
        "properties".into(),
        Value::String(format_props_typed(&f.properties)),
    );
    m
}

fn java_metadata(f: &JavaFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("class_name".into(), str_or_empty(&f.class_name));
    m.insert("function_name".into(), str_or_empty(&f.function_name));
    m.insert(
        "parameters".into(),
        Value::String(format_params_typed(&f.parameters)),
    );
    m.insert("return_type".into(), str_or_empty(&f.return_type));
    m.insert("namespace".into(), str_or_empty(&f.namespace));
    m
}

fn js_metadata(f: &JavaScriptFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("class_name".into(), str_or_empty(&f.class_name));
    m.insert("function_name".into(), str_or_empty(&f.function_name));
    m.insert(
        "parameters".into(),
        Value::String(format_params_typed(&f.parameters)),
    );
    m.insert("return_type".into(), str_or_empty(&f.return_type));
    m.insert("is_method".into(), Value::String(f.is_method.to_string()));
    m.insert("is_async".into(), Value::String(f.is_async.to_string()));
    m.insert(
        "is_generator".into(),
        Value::String(f.is_generator.to_string()),
    );
    m
}

fn ts_metadata(f: &TypeScriptFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("class_name".into(), str_or_empty(&f.class_name));
    m.insert("function_name".into(), str_or_empty(&f.function_name));
    m.insert(
        "parameters".into(),
        Value::String(format_params_typed(&f.parameters)),
    );
    m.insert("return_type".into(), str_or_empty(&f.return_type));
    m.insert("is_method".into(), Value::String(f.is_method.to_string()));
    m.insert("is_async".into(), Value::String(f.is_async.to_string()));
    m.insert(
        "is_generator".into(),
        Value::String(f.is_generator.to_string()),
    );
    m
}

fn react_metadata(f: &ReactFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("component_name".into(), str_or_empty(&f.component_name));
    m.insert(
        "is_functional".into(),
        Value::String(f.is_functional.to_string()),
    );
    m.insert(
        "hooks_used".into(),
        Value::String(f.hooks_used.join(", ")),
    );
    m.insert(
        "props".into(),
        Value::String(format_params_typed(&f.props)),
    );
    m.insert("component_type".into(), str_or_empty(&f.component_type));
    m.insert(
        "parent_component".into(),
        str_or_empty(&f.parent_component),
    );
    m
}

fn rust_metadata(f: &RustFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("class_name".into(), str_or_empty(&f.class_name));
    m.insert("function_name".into(), str_or_empty(&f.function_name));
    m.insert(
        "parameters".into(),
        Value::String(format_params_typed(&f.parameters)),
    );
    m.insert("return_type".into(), str_or_empty(&f.return_type));
    m.insert("namespace".into(), str_or_empty(&f.namespace));
    m
}

fn python_metadata(f: &PythonFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("class_name".into(), str_or_empty(&f.class_name));
    m.insert("function_name".into(), str_or_empty(&f.function_name));
    m.insert("alias_name".into(), str_or_empty(&f.alias_name));
    m.insert("alias_target".into(), str_or_empty(&f.alias_target));
    m.insert(
        "parameters".into(),
        Value::String(format_param_types(&f.parameters)),
    );
    m.insert("return_type".into(), str_or_empty(&f.return_type));
    m.insert(
        "properties".into(),
        Value::String(format_props_python(&f.properties)),
    );
    m.insert("decorators".into(), Value::String(f.decorators.join(", ")));
    m.insert("type_parameters".into(), str_or_empty(&f.type_parameters));
    m.insert("bases".into(), str_or_empty(&f.bases));
    m.insert("is_method".into(), Value::String(f.is_method.to_string()));
    m.insert("is_async".into(), Value::String(f.is_async.to_string()));
    m.insert(
        "is_generator".into(),
        Value::String(f.is_generator.to_string()),
    );
    m.insert(
        "is_type_alias".into(),
        Value::String(f.is_type_alias.to_string()),
    );
    m
}

fn sql_metadata(f: &SqlFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("statement_type".into(), str_or_empty(&f.statement_type));
    m.insert("object_name".into(), str_or_empty(&f.object_name));
    m.insert("schema_name".into(), str_or_empty(&f.schema_name));
    m.insert("database_name".into(), str_or_empty(&f.database_name));
    m
}

// --- embedding-parts helpers ------------------------------------------------

fn c_embedding_parts(f: &CFields) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(ns) = &f.namespace {
        v.push(format!("Namespace: {ns}"));
    }
    if let Some(fn_) = &f.function_name {
        v.push(format!("Function: {fn_}"));
    }
    if !f.parameters.is_empty() {
        v.push(format!("Parameters: {}", format_params_typed(&f.parameters)));
    }
    if let Some(rt) = &f.return_type {
        v.push(format!("Return Type: {rt}"));
    }
    v
}

fn cpp_embedding_parts(f: &CppFields) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(ns) = &f.namespace {
        v.push(format!("Namespace: {ns}"));
    }
    if let Some(c) = &f.class_name {
        v.push(format!("Class: {c}"));
    }
    if let Some(fn_) = &f.function_name {
        v.push(format!("Function: {fn_}"));
    }
    if !f.parameters.is_empty() {
        v.push(format!("Parameters: {}", format_params_typed(&f.parameters)));
    }
    if let Some(rt) = &f.return_type {
        v.push(format!("Return Type: {rt}"));
    }
    v
}

fn csharp_embedding_parts(f: &CSharpFields) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(c) = &f.class_name {
        v.push(format!("Class: {c}"));
    }
    if let Some(i) = &f.interface_name {
        v.push(format!("Interface: {i}"));
    }
    if let Some(fn_) = &f.function_name {
        v.push(format!("Function: {fn_}"));
    }
    if !f.parameters.is_empty() {
        v.push(format!("Parameters: {}", format_params_typed(&f.parameters)));
    }
    if let Some(rt) = &f.return_type {
        v.push(format!("Return Type: {rt}"));
    }
    if !f.properties.is_empty() {
        v.push(format!("Properties: {}", format_props_typed(&f.properties)));
    }
    v
}

fn java_embedding_parts(f: &JavaFields) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(ns) = &f.namespace {
        v.push(format!("Namespace: {ns}"));
    }
    if let Some(c) = &f.class_name {
        v.push(format!("Class: {c}"));
    }
    if let Some(fn_) = &f.function_name {
        v.push(format!("Function: {fn_}"));
    }
    if !f.parameters.is_empty() {
        v.push(format!("Parameters: {}", format_params_typed(&f.parameters)));
    }
    if let Some(rt) = &f.return_type {
        v.push(format!("Return Type: {rt}"));
    }
    v
}

fn js_embedding_parts(f: &JavaScriptFields) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(c) = &f.class_name {
        v.push(format!("Class: {c}"));
    }
    if let Some(fn_) = &f.function_name {
        v.push(format!("Function: {fn_}"));
    }
    if !f.parameters.is_empty() {
        v.push(format!("Parameters: {}", format_params_typed(&f.parameters)));
    }
    if let Some(rt) = &f.return_type {
        v.push(format!("Return Type: {rt}"));
    }
    v.push(format!("Method: {}", f.is_method));
    v.push(format!("Async: {}", f.is_async));
    v.push(format!("Generator: {}", f.is_generator));
    v
}

fn ts_embedding_parts(f: &TypeScriptFields) -> Vec<String> {
    js_embedding_parts(&JavaScriptFields {
        function_name: f.function_name.clone(),
        class_name: f.class_name.clone(),
        parameters: f.parameters.clone(),
        return_type: f.return_type.clone(),
        is_method: f.is_method,
        is_async: f.is_async,
        is_generator: f.is_generator,
    })
}

fn react_embedding_parts(f: &ReactFields) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(c) = &f.component_name {
        v.push(format!("Component: {c}"));
    }
    if !f.hooks_used.is_empty() {
        v.push(format!("Hooks: {}", f.hooks_used.join(", ")));
    }
    if !f.props.is_empty() {
        v.push(format!("Props: {}", format_params_typed(&f.props)));
    }
    if let Some(p) = &f.parent_component {
        v.push(format!("Parent Component: {p}"));
    }
    v
}

fn rust_embedding_parts(f: &RustFields) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(ns) = &f.namespace {
        v.push(format!("Namespace: {ns}"));
    }
    if let Some(c) = &f.class_name {
        v.push(format!("Class: {c}"));
    }
    if let Some(fn_) = &f.function_name {
        v.push(format!("Function: {fn_}"));
    }
    if !f.parameters.is_empty() {
        v.push(format!("Parameters: {}", format_params_typed(&f.parameters)));
    }
    if let Some(rt) = &f.return_type {
        v.push(format!("Return Type: {rt}"));
    }
    v
}

fn python_embedding_parts(f: &PythonFields) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(c) = &f.class_name {
        v.push(format!("Class: {c}"));
    }
    if let Some(fn_) = &f.function_name {
        v.push(format!("Function: {fn_}"));
    }
    if let Some(a) = &f.alias_name {
        v.push(format!("Type Alias: {a}"));
    }
    if let Some(a) = &f.alias_target {
        v.push(format!("Alias Target: {a}"));
    }
    if !f.parameters.is_empty() {
        v.push(format!(
            "Parameters: {}",
            f.parameters
                .iter()
                .map(|p| p.r#type.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(rt) = &f.return_type {
        v.push(format!("Return Type: {rt}"));
    }
    if !f.properties.is_empty() {
        v.push(format!(
            "Properties: {}",
            f.properties
                .iter()
                .map(|p| {
                    if p.r#type.is_empty() {
                        p.name.clone()
                    } else {
                        format!("{}: {}", p.name, p.r#type)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !f.decorators.is_empty() {
        v.push(format!("Decorators: {}", f.decorators.join(", ")));
    }
    if let Some(tp) = &f.type_parameters {
        v.push(format!("Type Parameters: {tp}"));
    }
    if let Some(b) = &f.bases {
        v.push(format!("Bases: {b}"));
    }
    if f.is_method {
        v.push("Method: True".into());
    }
    if f.is_async {
        v.push("Async: True".into());
    }
    if f.is_generator {
        v.push("Generator: True".into());
    }
    if f.is_type_alias {
        v.push("Type Alias: True".into());
    }
    v
}

fn sql_embedding_parts(f: &SqlFields) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(d) = &f.database_name {
        v.push(format!("Database: {d}"));
    }
    if let Some(s) = &f.statement_type {
        v.push(format!("Statement: {s}"));
    }
    if let Some(s) = &f.schema_name {
        v.push(format!("Schema: {s}"));
    }
    if let Some(o) = &f.object_name {
        v.push(format!("Object: {o}"));
    }
    v
}

// --- DocumentChunk ----------------------------------------------------------

/// Document chunk for non-code uploads (PDF, DOCX, HTML, ...) and website
/// ingestion. Mirrors Python `custom_types/DocumentChunk.py`.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentChunk {
    pub id: String,
    pub file_name: String,
    pub content: String,
    pub doc_type: String,
    pub chunk_index: i64,
    pub page: Option<i64>,
    pub metadata: Map<String, Value>,
    pub created_at: String,
}

impl DocumentChunk {
    pub fn new(
        id: impl Into<String>,
        file_name: impl Into<String>,
        content: impl Into<String>,
        doc_type: impl Into<String>,
        chunk_index: i64,
    ) -> Self {
        Self {
            id: id.into(),
            file_name: file_name.into(),
            content: content.into(),
            doc_type: doc_type.into(),
            chunk_index,
            page: None,
            metadata: Map::new(),
            created_at: now_iso(),
        }
    }

    /// Full metadata payload written to Qdrant. Matches Python
    /// `DocumentChunk.get_chunk_metadata`.
    pub fn chunk_metadata(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("file_name".into(), Value::String(self.file_name.clone()));
        m.insert("doc_type".into(), Value::String(self.doc_type.clone()));
        m.insert(
            "chunk_index".into(),
            Value::Number(self.chunk_index.into()),
        );
        m.insert("content".into(), Value::String(self.content.clone()));
        m.insert("created_at".into(), Value::String(self.created_at.clone()));
        if let Some(p) = self.page {
            m.insert("page".into(), Value::Number(p.into()));
        }
        for (k, v) in &self.metadata {
            m.insert(k.clone(), v.clone());
        }
        m
    }

    /// Embedding text. Matches Python `DocumentChunk.get_chunk_embedding_text`.
    pub fn chunk_embedding_text(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.push(format!("File_name: {}", self.file_name));
        parts.push(format!("Type: {}", self.doc_type));
        if let Some(p) = self.page {
            parts.push(format!("Page: {p}"));
        }
        parts.push(format!("Content:\n{}", self.content));
        parts.join("\n")
    }
}

// --- shared helpers ---------------------------------------------------------

/// ISO 8601 UTC timestamp. Same format as `worker::progress::now_iso` so
/// chunk `created_at` columns align with `job_status.updated_at`.
pub fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Normalize code: strip trailing whitespace per line and collapse 3+
/// blank lines into a single blank line. Matches Python `_normalize_code`.
pub fn normalize_code(code: &str) -> String {
    let normalized: String = code
        .split_inclusive('\n')
        .map(|line| {
            let line = line.strip_suffix('\n').unwrap_or(line);
            let line = line.strip_suffix('\r').unwrap_or(line);
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = String::with_capacity(normalized.len());
    let mut blanks = 0usize;
    for ch in normalized.chars() {
        if ch == '\n' {
            blanks += 1;
            if blanks <= 2 {
                out.push(ch);
            }
        } else {
            blanks = 0;
            out.push(ch);
        }
    }
    if out.trim_matches('\n').is_empty() {
        return "\n".to_string();
    }
    out
}

/// Clean and normalize code spacing for embedding. Matches Python
/// `helpers.clean_code` (applied after chunk extraction in
/// `chunk_single_code_file`).
pub fn clean_code(code: &str) -> String {
    if code.is_empty() {
        return String::new();
    }
    let expanded = expand_tabs(code, 4);
    let dedented = dedent(&expanded);
    let lines: Vec<String> = dedented
        .split_inclusive('\n')
        .map(|line| {
            let line = line.strip_suffix('\n').unwrap_or(line);
            let line = line.strip_suffix('\r').unwrap_or(line);
            line.trim_end().to_string()
        })
        .collect();
    let mut cleaned = lines.join("\n");
    cleaned = collapse_blank_lines(&cleaned);
    cleaned = cleaned.replace(" { get; set; }", " {get;set;}");
    static EMPTY_BRACES_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = EMPTY_BRACES_RE.get_or_init(|| regex::Regex::new(r"\s*\{\s*\}").unwrap());
    cleaned = re.replace_all(&cleaned, "{}").into_owned();
    cleaned.trim().to_string()
}

fn expand_tabs(s: &str, tabsize: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for ch in s.chars() {
        if ch == '\t' {
            let spaces = tabsize - (col % tabsize);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else {
            out.push(ch);
            if ch == '\n' {
                col = 0;
            } else {
                col += 1;
            }
        }
    }
    out
}

fn dedent(text: &str) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut margin: Option<usize> = None;
    for line in &lines {
        let raw = line.strip_suffix('\n').unwrap_or(line);
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if raw.trim().is_empty() {
            continue;
        }
        let indent = raw.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        margin = Some(match margin {
            Some(m) => m.min(indent),
            None => indent,
        });
    }
    let Some(m) = margin else {
        return text.to_string();
    };
    if m == 0 {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    for line in lines {
        let has_nl = line.ends_with('\n');
        let raw = line.strip_suffix('\n').unwrap_or(line);
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if raw.trim().is_empty() {
            if has_nl {
                out.push('\n');
            }
            continue;
        }
        let stripped: String = raw.chars().skip(m).collect();
        out.push_str(&stripped);
        if has_nl {
            out.push('\n');
        }
    }
    out
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blanks = 0usize;
    for ch in s.chars() {
        if ch == '\n' {
            blanks += 1;
            if blanks <= 2 {
                out.push(ch);
            }
        } else {
            blanks = 0;
            out.push(ch);
        }
    }
    out
}

/// Serialize a `Map<String, Value>` to a deterministic JSON object with
/// sorted keys. Used in tests to compare metadata payloads structurally
/// regardless of insertion order.
#[allow(dead_code)]
pub fn sorted_json(m: &Map<String, Value>) -> Value {
    let mut tree: BTreeMap<String, Value> = BTreeMap::new();
    for (k, v) in m {
        tree.insert(k.clone(), v.clone());
    }
    serde_json::to_value(tree).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_blank_runs() {
        let src = "a\n\n\n\nb\n   \n\nc\n";
        let out = normalize_code(src);
        assert_eq!(out, "a\n\nb\n\nc");
    }

    #[test]
    fn normalize_strips_trailing_whitespace() {
        let src = "a   \nb\t\n";
        let out = normalize_code(src);
        assert_eq!(out, "a\nb");
    }

    #[test]
    fn normalize_all_blanks_returns_single_newline() {
        let src = "\n\n\n  \n\n";
        assert_eq!(normalize_code(src), "\n");
    }

    #[test]
    fn generic_chunk_metadata_includes_base_fields() {
        let c = CodeChunk::generic("id-1", "repo", "f.txt", "hello");
        let m = c.chunk_metadata();
        assert_eq!(m.get("repo_name").unwrap().as_str().unwrap(), "repo");
        assert_eq!(m.get("file_name").unwrap().as_str().unwrap(), "f.txt");
        assert_eq!(m.get("type").unwrap().as_str().unwrap(), "generic");
        assert_eq!(m.get("code").unwrap().as_str().unwrap(), "hello");
        assert!(m.get("created_at").is_some());
    }

    #[test]
    fn python_chunk_metadata_uses_param_types_only() {
        let chunk = CodeChunk {
            id: "x".into(),
            repo_name: "r".into(),
            file_name: "f.py".into(),
            code: "pass".into(),
            r#type: "function_definition".into(),
            dependencies: Vec::new(),
            created_at: now_iso(),
            kind: CodeChunkKind::Python(PythonFields {
                function_name: Some("describe".into()),
                class_name: None,
                parameters: vec![
                    Parameter::new("item", "Annotated[str, \"display\"]"),
                    Parameter::new("/", "/"),
                    Parameter::new("count", "int = 1"),
                ],
                return_type: Some("dict[str, object]".into()),
                decorators: vec!["@traced(\"sync\")".into()],
                is_method: false,
                is_async: false,
                is_generator: false,
                is_type_alias: false,
                properties: Vec::new(),
                alias_name: None,
                alias_target: None,
                type_parameters: None,
                bases: None,
            }),
        };
        let m = chunk.chunk_metadata();
        // Python: `"parameters": ", ".join(p.type for p in self.parameters)`.
        // The `positional_separator` (i.e. `/`) node carries its own text as
        // its type, so it's preserved in the join — matching Python behavior
        // verified against the reference implementation.
        let expected_params = r#"Annotated[str, "display"], /, int = 1"#;
        assert_eq!(
            m.get("parameters").unwrap().as_str().unwrap(),
            expected_params
        );
        assert_eq!(
            m.get("decorators").unwrap().as_str().unwrap(),
            "@traced(\"sync\")"
        );
    }

    #[test]
    fn python_chunk_embedding_text_lists_decorators_after_class_function() {
        let chunk = CodeChunk {
            id: "x".into(),
            repo_name: "r".into(),
            file_name: "f.py".into(),
            code: "pass".into(),
            r#type: "function_definition".into(),
            dependencies: Vec::new(),
            created_at: now_iso(),
            kind: CodeChunkKind::Python(PythonFields {
                function_name: Some("describe".into()),
                class_name: None,
                parameters: Vec::new(),
                return_type: Some("dict[str, object]".into()),
                decorators: vec!["@traced(\"sync\")".into()],
                is_method: false,
                is_async: false,
                is_generator: false,
                is_type_alias: false,
                properties: Vec::new(),
                alias_name: None,
                alias_target: None,
                type_parameters: None,
                bases: None,
            }),
        };
        let t = chunk.chunk_embedding_text();
        assert!(t.contains("Function: describe"));
        assert!(t.contains("Return Type: dict[str, object]"));
        assert!(t.contains("Decorators: @traced(\"sync\")"));
        assert!(t.contains("Code:\n"));
    }

    #[test]
    fn document_chunk_metadata_merges_user_metadata() {
        let mut c = DocumentChunk::new("id", "f.pdf", "text", "pdf", 0);
        c.page = Some(3);
        c.metadata
            .insert("group".into(), Value::String("docs".into()));
        let m = c.chunk_metadata();
        assert_eq!(m.get("file_name").unwrap().as_str().unwrap(), "f.pdf");
        assert_eq!(m.get("doc_type").unwrap().as_str().unwrap(), "pdf");
        assert_eq!(m.get("chunk_index").unwrap().as_i64().unwrap(), 0);
        assert_eq!(m.get("page").unwrap().as_i64().unwrap(), 3);
        assert_eq!(m.get("group").unwrap().as_str().unwrap(), "docs");
    }

    #[test]
    fn document_chunk_embedding_text_includes_page_when_present() {
        let mut c = DocumentChunk::new("id", "f.pdf", "body", "pdf", 2);
        c.page = Some(5);
        let t = c.chunk_embedding_text();
        assert!(t.contains("File_name: f.pdf"));
        assert!(t.contains("Type: pdf"));
        assert!(t.contains("Page: 5"));
        assert!(t.contains("Content:\nbody"));
    }

    #[test]
    fn document_chunk_embedding_text_omits_page_when_absent() {
        let c = DocumentChunk::new("id", "f.txt", "body", "txt", 0);
        let t = c.chunk_embedding_text();
        assert!(!t.contains("Page:"));
    }

    #[test]
    fn code_chunk_split_preserves_kind_and_metadata() {
        let chunk = CodeChunk::generic("orig", "r", "f.txt", "x = 1\n".repeat(20));
        let split = chunk.split("new", "y = 2\n");
        assert_eq!(split.id, "new");
        assert_eq!(split.repo_name, "r");
        assert_eq!(split.file_name, "f.txt");
        assert!(matches!(split.kind, CodeChunkKind::Generic));
        assert_eq!(split.code, "y = 2");
    }

    #[test]
    fn clean_code_expands_tabs_and_collapses_empty_braces() {
        assert_eq!(clean_code("a\tb"), "a   b");
        // Python re.sub(r"\s*{\s*}", "{}") also eats the space before `{`.
        assert_eq!(clean_code("const obj = {  }"), "const obj ={}");
        assert_eq!(clean_code("noop = () => { }"), "noop = () =>{}");
        assert_eq!(clean_code("prop { get; set; }"), "prop {get;set;}");
    }

    #[test]
    fn clean_code_dedents_and_strips() {
        let src = "    def foo():\n        return 1\n";
        assert_eq!(clean_code(src), "def foo():\n    return 1");
    }

    #[test]
    fn normalize_code_handles_crlf_and_blank_runs() {
        // Matches Python `_normalize_code` / `str.splitlines()` (drops final newline).
        assert_eq!(normalize_code("foo\r\nbar\r\n"), "foo\nbar");
        assert_eq!(normalize_code("a\n\n\n\nb"), "a\n\nb");
        assert_eq!(normalize_code("a  \nb   "), "a\nb");
    }
}