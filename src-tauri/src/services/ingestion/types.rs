//! Chunk data types shared across the ingestion pipeline.
//!
//! Mirrors the Python `custom_types/CodeChunk.py` and
//! `custom_types/DocumentChunk.py`. Each language-specific `CodeChunk`
//! variant carries the same field set as the Python dataclass, and exposes
//! `chunk_metadata` / `chunk_embedding_text` so the ingestion pipeline can
//! serialize payloads and build embedding text uniformly.

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

/// Fields shared by the C++, Java, and Rust chunkers: a function inside an
/// optional class (C++ class, Java type, Rust `impl`/trait) and namespace
/// (C++ namespace, Java package, Rust `mod`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MemberFields {
    pub function_name: Option<String>,
    pub class_name: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub namespace: Option<String>,
}

pub type CppFields = MemberFields;
pub type JavaFields = MemberFields;
pub type RustFields = MemberFields;

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
pub struct JavaScriptFields {
    pub function_name: Option<String>,
    pub class_name: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub is_method: bool,
    pub is_async: bool,
    pub is_generator: bool,
}

pub type TypeScriptFields = JavaScriptFields;

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
            CodeChunkKind::Cpp(f) | CodeChunkKind::Java(f) | CodeChunkKind::Rust(f) => {
                member_metadata(f)
            }
            CodeChunkKind::CSharp(f) => csharp_metadata(f),
            CodeChunkKind::JavaScript(f) | CodeChunkKind::TypeScript(f) => js_metadata(f),
            CodeChunkKind::React(f) => react_metadata(f),
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
            CodeChunkKind::Cpp(f) | CodeChunkKind::Java(f) | CodeChunkKind::Rust(f) => {
                member_embedding_parts(f)
            }
            CodeChunkKind::CSharp(f) => csharp_embedding_parts(f),
            CodeChunkKind::JavaScript(f) | CodeChunkKind::TypeScript(f) => js_embedding_parts(f),
            CodeChunkKind::React(f) => react_embedding_parts(f),
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
        if let Some(symbol) = self.symbol() {
            map.insert("symbol".into(), Value::String(symbol));
        }
        if let Some(qualified) = self.qualified_symbol() {
            map.insert("qualified_symbol".into(), Value::String(qualified));
        }
        for (k, v) in self.kind_metadata() {
            map.insert(k, v);
        }
        map
    }

    /// `(container, name)` this chunk defines: the enclosing class/impl type
    /// (or interface/schema) and the function, component, alias, or object
    /// name. A type definition has only a container.
    fn defined_names(&self) -> (Option<&str>, Option<&str>) {
        match &self.kind {
            CodeChunkKind::Generic => (None, None),
            CodeChunkKind::C(f) => (None, f.function_name.as_deref()),
            CodeChunkKind::Cpp(f) | CodeChunkKind::Java(f) | CodeChunkKind::Rust(f) => {
                (f.class_name.as_deref(), f.function_name.as_deref())
            }
            CodeChunkKind::CSharp(f) => (
                f.class_name.as_deref().or(f.interface_name.as_deref()),
                f.function_name.as_deref(),
            ),
            CodeChunkKind::JavaScript(f) | CodeChunkKind::TypeScript(f) => {
                (f.class_name.as_deref(), f.function_name.as_deref())
            }
            CodeChunkKind::React(f) => (None, f.component_name.as_deref()),
            CodeChunkKind::Python(f) => (
                f.class_name.as_deref(),
                f.function_name.as_deref().or(f.alias_name.as_deref()),
            ),
            CodeChunkKind::Sql(f) => (f.schema_name.as_deref(), f.object_name.as_deref()),
        }
    }

    /// Name this chunk defines (`run_rag_query`, `RagService`), used for
    /// definition lookups. `None` for generic chunks and file remainders.
    pub fn symbol(&self) -> Option<String> {
        let (container, name) = self.defined_names();
        name.or(container)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    /// `Container::name` (Rust, C++) or `Container.name` (other languages)
    /// when the chunk is a member of a type; otherwise `None`.
    pub fn qualified_symbol(&self) -> Option<String> {
        let (container, name) = self.defined_names();
        let (Some(container), Some(name)) = (container, name) else {
            return None;
        };
        if container.is_empty() || name.is_empty() {
            return None;
        }
        let separator = match &self.kind {
            CodeChunkKind::Rust(_) | CodeChunkKind::Cpp(_) => "::",
            _ => ".",
        };
        Some(format!("{container}{separator}{name}"))
    }

    /// Structural header for the embedding text: repo, file, and the
    /// language-specific identity (class, function, parameters, return type,
    /// ...). Capped at [`MAX_CODE_HEADER_CHARS`] so a huge parameter or
    /// property list cannot crowd the code out of the model's 512-token window.
    pub fn embedding_header(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.push(format!("Repo Name: {}", self.repo_name));
        parts.push(format!("File Name: {}", self.file_name));
        for p in self.kind_embedding_parts() {
            if !p.is_empty() {
                parts.push(p);
            }
        }
        truncate_chars(&parts.join("\n"), MAX_CODE_HEADER_CHARS)
    }

    /// Text sent to the dense embedder: [`Self::embedding_header`] followed by
    /// the code. File-level dependencies are deliberately left out: they are
    /// identical for every chunk of a file, so they would pull all of a file's
    /// vectors together and spend tokens without helping retrieval. They stay
    /// available in the payload metadata.
    pub fn chunk_embedding_text(&self) -> String {
        format!("{}\nCode:\n{}", self.embedding_header(), self.code)
    }
}

/// Upper bound on the code embedding header, in characters (roughly 100-130
/// tokens for identifier-heavy text).
pub const MAX_CODE_HEADER_CHARS: usize = 400;

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}

// --- metadata helpers -------------------------------------------------------

fn str_or_empty(s: &Option<String>) -> Value {
    Value::String(s.clone().unwrap_or_default())
}

fn bool_value(b: bool) -> Value {
    Value::String(b.to_string())
}

fn insert_function_metadata(
    m: &mut Map<String, Value>,
    function_name: &Option<String>,
    parameters: &[Parameter],
    return_type: &Option<String>,
) {
    m.insert("function_name".into(), str_or_empty(function_name));
    m.insert(
        "parameters".into(),
        Value::String(format_params_typed(parameters)),
    );
    m.insert("return_type".into(), str_or_empty(return_type));
}

fn c_metadata(f: &CFields) -> Map<String, Value> {
    let mut m = Map::new();
    insert_function_metadata(&mut m, &f.function_name, &f.parameters, &f.return_type);
    m.insert("namespace".into(), str_or_empty(&f.namespace));
    m
}

fn member_metadata(f: &MemberFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("class_name".into(), str_or_empty(&f.class_name));
    insert_function_metadata(&mut m, &f.function_name, &f.parameters, &f.return_type);
    m.insert("namespace".into(), str_or_empty(&f.namespace));
    m
}

fn csharp_metadata(f: &CSharpFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("class_name".into(), str_or_empty(&f.class_name));
    m.insert("interface_name".into(), str_or_empty(&f.interface_name));
    insert_function_metadata(&mut m, &f.function_name, &f.parameters, &f.return_type);
    m.insert(
        "properties".into(),
        Value::String(format_params_typed(&f.properties)),
    );
    m
}

fn js_metadata(f: &JavaScriptFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("class_name".into(), str_or_empty(&f.class_name));
    insert_function_metadata(&mut m, &f.function_name, &f.parameters, &f.return_type);
    m.insert("is_method".into(), bool_value(f.is_method));
    m.insert("is_async".into(), bool_value(f.is_async));
    m.insert("is_generator".into(), bool_value(f.is_generator));
    m
}

fn react_metadata(f: &ReactFields) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("component_name".into(), str_or_empty(&f.component_name));
    m.insert("is_functional".into(), bool_value(f.is_functional));
    m.insert("hooks_used".into(), Value::String(f.hooks_used.join(", ")));
    m.insert("props".into(), Value::String(format_params_typed(&f.props)));
    m.insert("component_type".into(), str_or_empty(&f.component_type));
    m.insert("parent_component".into(), str_or_empty(&f.parent_component));
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
    m.insert("is_method".into(), bool_value(f.is_method));
    m.insert("is_async".into(), bool_value(f.is_async));
    m.insert("is_generator".into(), bool_value(f.is_generator));
    m.insert("is_type_alias".into(), bool_value(f.is_type_alias));
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

fn push_opt(v: &mut Vec<String>, label: &str, value: &Option<String>) {
    if let Some(value) = value {
        v.push(format!("{label}: {value}"));
    }
}

fn push_flag(v: &mut Vec<String>, label: &str, flag: bool) {
    if flag {
        v.push(format!("{label}: True"));
    }
}

fn push_function_parts(
    v: &mut Vec<String>,
    function_name: &Option<String>,
    parameters: &[Parameter],
    return_type: &Option<String>,
) {
    push_opt(v, "Function", function_name);
    if !parameters.is_empty() {
        v.push(format!("Parameters: {}", format_params_typed(parameters)));
    }
    push_opt(v, "Return Type", return_type);
}

fn c_embedding_parts(f: &CFields) -> Vec<String> {
    let mut v = Vec::new();
    push_opt(&mut v, "Namespace", &f.namespace);
    push_function_parts(&mut v, &f.function_name, &f.parameters, &f.return_type);
    v
}

fn member_embedding_parts(f: &MemberFields) -> Vec<String> {
    let mut v = Vec::new();
    push_opt(&mut v, "Namespace", &f.namespace);
    push_opt(&mut v, "Class", &f.class_name);
    push_function_parts(&mut v, &f.function_name, &f.parameters, &f.return_type);
    v
}

fn csharp_embedding_parts(f: &CSharpFields) -> Vec<String> {
    let mut v = Vec::new();
    push_opt(&mut v, "Class", &f.class_name);
    push_opt(&mut v, "Interface", &f.interface_name);
    push_function_parts(&mut v, &f.function_name, &f.parameters, &f.return_type);
    if !f.properties.is_empty() {
        v.push(format!(
            "Properties: {}",
            format_params_typed(&f.properties)
        ));
    }
    v
}

fn js_embedding_parts(f: &JavaScriptFields) -> Vec<String> {
    let mut v = Vec::new();
    push_opt(&mut v, "Class", &f.class_name);
    push_function_parts(&mut v, &f.function_name, &f.parameters, &f.return_type);
    push_flag(&mut v, "Method", f.is_method);
    push_flag(&mut v, "Async", f.is_async);
    push_flag(&mut v, "Generator", f.is_generator);
    v
}

fn react_embedding_parts(f: &ReactFields) -> Vec<String> {
    let mut v = Vec::new();
    push_opt(&mut v, "Component", &f.component_name);
    if !f.hooks_used.is_empty() {
        v.push(format!("Hooks: {}", f.hooks_used.join(", ")));
    }
    if !f.props.is_empty() {
        v.push(format!("Props: {}", format_params_typed(&f.props)));
    }
    push_opt(&mut v, "Parent Component", &f.parent_component);
    v
}

fn python_embedding_parts(f: &PythonFields) -> Vec<String> {
    let mut v = Vec::new();
    push_opt(&mut v, "Class", &f.class_name);
    push_opt(&mut v, "Function", &f.function_name);
    push_opt(&mut v, "Type Alias", &f.alias_name);
    push_opt(&mut v, "Alias Target", &f.alias_target);
    if !f.parameters.is_empty() {
        v.push(format!("Parameters: {}", format_param_types(&f.parameters)));
    }
    push_opt(&mut v, "Return Type", &f.return_type);
    if !f.properties.is_empty() {
        v.push(format!(
            "Properties: {}",
            format_props_python(&f.properties)
        ));
    }
    if !f.decorators.is_empty() {
        v.push(format!("Decorators: {}", f.decorators.join(", ")));
    }
    push_opt(&mut v, "Type Parameters", &f.type_parameters);
    push_opt(&mut v, "Bases", &f.bases);
    push_flag(&mut v, "Method", f.is_method);
    push_flag(&mut v, "Async", f.is_async);
    push_flag(&mut v, "Generator", f.is_generator);
    push_flag(&mut v, "Type Alias", f.is_type_alias);
    v
}

fn sql_embedding_parts(f: &SqlFields) -> Vec<String> {
    let mut v = Vec::new();
    push_opt(&mut v, "Database", &f.database_name);
    push_opt(&mut v, "Statement", &f.statement_type);
    push_opt(&mut v, "Schema", &f.schema_name);
    push_opt(&mut v, "Object", &f.object_name);
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
    /// Set for code chunks: the [`CodeChunk::embedding_header`] that replaces
    /// the generic file/type header in [`Self::chunk_embedding_text`].
    pub code_header: Option<String>,
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
            code_header: None,
        }
    }

    /// Full metadata payload written to Qdrant. Matches Python
    /// `DocumentChunk.get_chunk_metadata`.
    pub fn chunk_metadata(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("file_name".into(), Value::String(self.file_name.clone()));
        m.insert("doc_type".into(), Value::String(self.doc_type.clone()));
        m.insert("chunk_index".into(), Value::Number(self.chunk_index.into()));
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

    /// Text sent to the dense embedder. Code chunks use their structural
    /// header; everything else gets file name, type, and page.
    pub fn chunk_embedding_text(&self) -> String {
        if let Some(header) = &self.code_header {
            return format!("{header}\nCode:\n{}", self.content);
        }
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

/// ISO 8601 UTC timestamp. Delegates to `worker::progress::now_iso` so
/// chunk `created_at` columns align with `job_status.updated_at`.
pub fn now_iso() -> String {
    crate::worker::progress::now_iso()
}

/// Force-split `text` into pieces of at most `max_chars` (char boundary
/// safe). Used when the token splitter cannot break minified one-liners.
pub fn hard_split_chars(text: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 || text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut buf = String::with_capacity(max_chars);
    let mut n = 0usize;
    for ch in text.chars() {
        if n >= max_chars {
            out.push(std::mem::take(&mut buf));
            n = 0;
        }
        buf.push(ch);
        n += 1;
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
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
    let out = collapse_blank_lines(&normalized);
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
                parameters: vec![
                    Parameter::new("item", "Annotated[str, \"display\"]"),
                    Parameter::new("/", "/"),
                    Parameter::new("count", "int = 1"),
                ],
                return_type: Some("dict[str, object]".into()),
                decorators: vec!["@traced(\"sync\")".into()],
                ..Default::default()
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
                return_type: Some("dict[str, object]".into()),
                decorators: vec!["@traced(\"sync\")".into()],
                ..Default::default()
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
