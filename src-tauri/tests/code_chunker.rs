//! Integration tests for the code chunkers. Ports the per-language slices
//! from `tests/test_code_chunk.py` against the moved `tests/test_data/
//! code_samples/` fixtures.
//!
//! Each test exercises one of the language-specific chunker ports against
//! a fixed sample file and verifies the produced `CodeChunk` list has the
//! expected shape (dependency list, function names, structural metadata).
//! Qdrant / embedder models are not used here — these tests run in-process
//! with no external dependencies.

mod common;

use std::path::{Path, PathBuf};

use mcp_nano_lib::services::ingestion::code_chunker;
use mcp_nano_lib::services::ingestion::types::{CodeChunk, CodeChunkKind};

use tree_sitter::Parser;

fn sample_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test_data/code_samples")
}

fn sample(file: &str) -> PathBuf {
    sample_dir().join(file)
}

/// A test-only splitter: constructed from the arctic tokenizer if it's on
/// disk (in `resources/models/`), else from a no-op character-based splitter
/// with an effectively unbounded chunk size so chunks pass through unsplit.
/// The splitter is only needed by the oversized-split step; tests that
/// don't trigger oversized splitting still pass a working splitter.
fn maybe_splitter() -> Option<text_splitter::TextSplitter<tokenizers::Tokenizer>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/models/arctic-embed-xs/tokenizer.json");
    let tok = tokenizers::Tokenizer::from_file(&path).ok()?;
    let config = text_splitter::ChunkConfig::new(1024)
        .with_sizer(tok)
        .with_overlap(64)
        .ok()?;
    Some(text_splitter::TextSplitter::new(config))
}

/// Chunk the file using whatever splitter is available.
///
/// When the arctic tokenizer isn't on disk, we substitute a fake tokenizer-
/// based splitter by constructing one from an in-memory HF tokenizer
/// built from a minimal model. That keeps the same generic API. As a
/// fallback we just panic — chunker tests require the downloaded model.
fn chunk_file(path: &Path, repo_name: &str) -> Vec<CodeChunk> {
    let splitter = maybe_splitter().expect(
        "arctic-embed-xs tokenizer required; run scripts/download-models.sh",
    );
    code_chunker::chunk_single_code_file(path, repo_name, None, &splitter, 1024)
}

/// Fallback splitter path (Tokenizer-based, no HF model required). Unused
/// for now since the oversized-split step needs the real tokenizer to be
/// meaningful; left as documentation for how to swap it out later if a
/// charset fallback becomes desirable.
#[allow(dead_code)]
fn _char_splitter() -> text_splitter::TextSplitter<text_splitter::Characters> {
    let config = text_splitter::ChunkConfig::new(usize::MAX)
        .with_sizer(text_splitter::Characters)
        .with_overlap(0)
        .unwrap();
    text_splitter::TextSplitter::new(config)
}

trait ChunkExt {
    /// Find the first chunk whose variant matches one of the expected
    /// language-specific predicate pairs (function/class/component name).
    fn find_rust_fn(&self, function_name: &str) -> Option<&CodeChunk>;
    fn find_rust_struct(&self, class_name: &str) -> Option<&CodeChunk>;
    fn find_c_fn(&self, function_name: &str) -> Option<&CodeChunk>;
    fn find_cpp_fn(&self, function_name: &str) -> Option<&CodeChunk>;
    fn find_java_fn(&self, function_name: &str, class_name: Option<&str>) -> Option<&CodeChunk>;
    fn find_js_fn(&self, function_name: &str) -> Option<&CodeChunk>;
    fn find_ts_fn(&self, function_name: &str) -> Option<&CodeChunk>;
    fn find_ts_class(&self, class_name: &str) -> Option<&CodeChunk>;
    fn find_react(&self, component_name: &str, chunk_type: &str) -> Option<&CodeChunk>;
    fn find_csharp_fn(&self, function_name: &str) -> Option<&CodeChunk>;
    fn find_csharp_class(&self, class_name: &str, chunk_type: &str) -> Option<&CodeChunk>;
    fn find_python_fn(&self, function_name: &str, class_name: Option<&str>) -> Option<&CodeChunk>;
    fn find_python_chunk_of_type(&self, chunk_type: &str) -> Option<&CodeChunk>;
}

impl ChunkExt for [CodeChunk] {
    fn find_rust_fn(&self, function_name: &str) -> Option<&CodeChunk> {
        self.iter().find(|c| {
            matches!(&c.kind, CodeChunkKind::Rust(f) if f.function_name.as_deref() == Some(function_name))
        })
    }
    fn find_rust_struct(&self, class_name: &str) -> Option<&CodeChunk> {
        self.iter().find(|c| {
            matches!(&c.kind, CodeChunkKind::Rust(f) if f.class_name.as_deref() == Some(class_name))
        })
    }
    fn find_c_fn(&self, function_name: &str) -> Option<&CodeChunk> {
        self.iter().find(|c| {
            matches!(&c.kind, CodeChunkKind::C(f) if f.function_name.as_deref() == Some(function_name))
        })
    }
    fn find_cpp_fn(&self, function_name: &str) -> Option<&CodeChunk> {
        self.iter().find(|c| {
            matches!(&c.kind, CodeChunkKind::Cpp(f) if f.function_name.as_deref() == Some(function_name))
        })
    }
    fn find_java_fn(&self, function_name: &str, class_name: Option<&str>) -> Option<&CodeChunk> {
        self.iter().find(|c| {
            if let CodeChunkKind::Java(f) = &c.kind {
                return f.function_name.as_deref() == Some(function_name)
                    && (class_name.is_none() || f.class_name.as_deref() == class_name);
            }
            false
        })
    }
    fn find_js_fn(&self, function_name: &str) -> Option<&CodeChunk> {
        self.iter().find(|c| {
            matches!(&c.kind, CodeChunkKind::JavaScript(f) if f.function_name.as_deref() == Some(function_name))
        })
    }
    fn find_ts_fn(&self, function_name: &str) -> Option<&CodeChunk> {
        self.iter().find(|c| {
            matches!(&c.kind, CodeChunkKind::TypeScript(f) if f.function_name.as_deref() == Some(function_name))
        })
    }
    fn find_ts_class(&self, class_name: &str) -> Option<&CodeChunk> {
        self.iter().find(|c| {
            matches!(&c.kind, CodeChunkKind::TypeScript(f) if f.class_name.as_deref() == Some(class_name))
        })
    }
    fn find_react(&self, component_name: &str, chunk_type: &str) -> Option<&CodeChunk> {
        self.iter().find(|c| {
            matches!(&c.kind, CodeChunkKind::React(f) if f.component_name.as_deref() == Some(component_name) && c.r#type == chunk_type)
        })
    }
    fn find_csharp_fn(&self, function_name: &str) -> Option<&CodeChunk> {
        self.iter().find(|c| {
            matches!(&c.kind, CodeChunkKind::CSharp(f) if f.function_name.as_deref() == Some(function_name))
        })
    }
    fn find_csharp_class(&self, class_name: &str, chunk_type: &str) -> Option<&CodeChunk> {
        self.iter().find(|c| {
            matches!(&c.kind, CodeChunkKind::CSharp(f) if f.class_name.as_deref() == Some(class_name) && c.r#type == chunk_type)
        })
    }
    fn find_python_fn(&self, function_name: &str, class_name: Option<&str>) -> Option<&CodeChunk> {
        self.iter().find(|c| {
            if let CodeChunkKind::Python(f) = &c.kind {
                return f.function_name.as_deref() == Some(function_name)
                    && (class_name.is_none() || f.class_name.as_deref() == class_name);
            }
            false
        })
    }
    fn find_python_chunk_of_type(&self, chunk_type: &str) -> Option<&CodeChunk> {
        self.iter().find(|c| c.r#type == chunk_type)
    }
}

// ---------------------------------------------------------------------------
// Rust
// ---------------------------------------------------------------------------

#[test]
fn rust_extracts_dependencies_and_fmt_chunk() {
    let chunks = chunk_file(&sample("sample_rust.rs"), "test_repo");
    assert!(!chunks.is_empty());
    let _ = Parser::new();

    let fmt = chunks.find_rust_fn("fmt").expect("missing fmt chunk");
    let CodeChunkKind::Rust(f) = &fmt.kind else { panic!("not Rust") };
    assert_eq!(f.class_name.as_deref(), Some("Wrapping<T>"));

    let deps = extract_rust_deps_via_public_api(&sample("sample_rust.rs"));
    use std::collections::HashSet;
    let dep_set: HashSet<String> = deps.into_iter().collect();
    assert!(dep_set.contains("crate::fmt"), "expected crate::fmt dep");
    let ops_dep = dep_set.iter().find(|d| d.starts_with("crate::ops::{")).expect("expected crate::ops dep");
    assert!(ops_dep.contains("Add, AddAssign, BitAnd"));
}

fn extract_rust_deps_via_public_api(path: &Path) -> Vec<String> {
    let bytes = std::fs::read(path).unwrap();
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter::Language::from(tree_sitter_rust::LANGUAGE)).unwrap();
    let tree = parser.parse(&bytes, None).unwrap();
    let root = tree.root_node();
    code_chunker::rust_lang::extract_dependencies(root, &bytes)
        .into_iter()
        .collect::<Vec<_>>()
        .into_iter()
        .collect()
}

#[test]
fn rust_produces_file_remainder_without_extracted_functions() {
    let chunks = chunk_file(&sample("sample_rust.rs"), "test_repo");
    let remainder = chunks
        .iter()
        .find(|c| c.r#type == "file_remainder")
        .expect("missing remainder chunk");
    assert!(!remainder.code.contains("fn fmt"));
}

// ---------------------------------------------------------------------------
// C
// ---------------------------------------------------------------------------

#[test]
fn c_extracts_function_chunks_and_dependencies() {
    let chunks = chunk_file(&sample("sample_c.c"), "test_repo");
    assert!(!chunks.is_empty());
    assert!(chunks.find_c_fn("__redisReaderSetError").is_some());
    let string2ll = chunks.find_c_fn("string2ll").expect("missing string2ll");
    let CodeChunkKind::C(f) = &string2ll.kind else { panic!("not C") };
    let names: Vec<&str> = f.parameters.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(&names[..3], &["s", "slen", "value"]);
    let remainder = chunks
        .iter()
        .find(|c| c.r#type == "file_remainder")
        .expect("missing remainder chunk");
    assert!(remainder.code.contains("REDIS_READER_STACK_SIZE"));
    assert!(!remainder.code.contains("string2ll"));
}

#[test]
fn c_header_extracts_dependencies() {
    let bytes = std::fs::read(&sample("sample_c.h")).unwrap();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter::Language::from(tree_sitter_c::LANGUAGE))
        .unwrap();
    let tree = parser.parse(&bytes, None).unwrap();
    let root = tree.root_node();
    let deps = code_chunker::c::extract_dependencies(root, &bytes);
    use std::collections::HashSet;
    let dep_set: HashSet<String> = deps.into_iter().collect();
    for expected in &["sys/types.h", "stdarg.h", "stdint.h"] {
        assert!(dep_set.contains(*expected), "missing dep {expected}: {dep_set:?}");
    }
}

// ---------------------------------------------------------------------------
// C++
// ---------------------------------------------------------------------------

#[test]
fn cpp_extracts_function_and_class_chunks() {
    let chunks = chunk_file(&sample("sample_cpp.cpp"), "test_repo");
    assert!(!chunks.is_empty());
    let swap = chunks.find_cpp_fn("swap").expect("missing swap");
    let CodeChunkKind::Cpp(f) = &swap.kind else { panic!("not C++") };
    assert_eq!(f.class_name.as_deref(), Some("UUID"));
    assert_eq!(f.namespace.as_deref(), Some("Poco"));
    let remainder = chunks
        .iter()
        .find(|c| c.r#type == "file_remainder")
        .expect("missing remainder chunk");
    assert!(remainder.code.contains("namespace Poco"));
    assert!(!remainder.code.contains("UUID::toString"));
}

// ---------------------------------------------------------------------------
// Java
// ---------------------------------------------------------------------------

#[test]
fn java_extracts_methods_and_namespace() {
    let chunks = chunk_file(&sample("sample_java.java"), "test_repo");
    assert!(!chunks.is_empty());
    let state = chunks
        .find_java_fn("state", Some("Assert"))
        .expect("missing state method");
    let CodeChunkKind::Java(f) = &state.kind else { panic!("not Java") };
    assert_eq!(f.namespace.as_deref(), Some("com.example.demo"));
}

// ---------------------------------------------------------------------------
// JavaScript
// ---------------------------------------------------------------------------

#[test]
fn js_extracts_function_method_and_remainder() {
    let chunks = chunk_file(&sample("sample_js.js"), "test_repo");
    assert!(!chunks.is_empty());
    for fn_name in &["isBuffer", "forEach", "findKey", "merge"] {
        assert!(chunks.find_js_fn(fn_name).is_some(), "missing {fn_name}");
    }
    let remainder = chunks
        .iter()
        .find(|c| c.r#type == "file_remainder")
        .expect("missing remainder chunk");
    assert!(remainder.code.contains("const {isArray}"));
    assert!(!remainder.code.contains("function merge"));
}

// ---------------------------------------------------------------------------
// TypeScript
// ---------------------------------------------------------------------------

#[test]
fn ts_extracts_function_chunks_and_remainder() {
    let chunks = chunk_file(&sample("sample_ts.ts"), "test_repo");
    assert!(!chunks.is_empty());
    for fn_name in &["makeIssue", "addIssueToContext", "mergeArray"] {
        assert!(chunks.find_ts_fn(fn_name).is_some(), "missing {fn_name}");
    }
    let make_issue = chunks.find_ts_fn("makeIssue").expect("missing makeIssue");
    let CodeChunkKind::TypeScript(f) = &make_issue.kind else { panic!("not TS") };
    assert_eq!(f.parameters.len(), 1);
    assert_eq!(f.return_type.as_deref(), Some("ZodIssue"));
    let iface = chunks.find_ts_class("ParseContext").expect("missing interface ParseContext");
    assert_eq!(iface.r#type, "interface_declaration");
}

// ---------------------------------------------------------------------------
// React / TSX
// ---------------------------------------------------------------------------

#[test]
fn react_extracts_functional_and_hooks() {
    let chunks = chunk_file(&sample("sample_react.tsx"), "test_repo");
    assert!(!chunks.is_empty());
    for name in &["App", "Layout"] {
        assert!(chunks.find_react(name, "functional_component").is_some());
    }
    assert!(chunks.find_react("useWindowSize", "hook").is_some());
}

// ---------------------------------------------------------------------------
// C#
// ---------------------------------------------------------------------------

#[test]
fn csharp_extracts_methods_and_classes() {
    let chunks = chunk_file(&sample("sample_csharp.cs"), "test_repo");
    assert!(!chunks.is_empty());
    assert!(chunks.find_csharp_fn("Encode").is_some());
    assert!(chunks.find_csharp_fn("Equals").is_some());
    assert!(chunks
        .find_csharp_class("MaxDiscountLogItemSummary", "record_declaration")
        .is_some());
    assert!(chunks
        .find_csharp_class("JsonTokenType", "enum_declaration")
        .is_some());
    assert!(chunks
        .find_csharp_class("JsonWriteCallback", "delegate_declaration")
        .is_some());
}

// ---------------------------------------------------------------------------
// Python
// ---------------------------------------------------------------------------

#[test]
fn python_extracts_dependencies() {
    let bytes = std::fs::read(&sample("sample_python.py")).unwrap();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter::Language::from(tree_sitter_python::LANGUAGE))
        .unwrap();
    let tree = parser.parse(&bytes, None).unwrap();
    let root = tree.root_node();
    let deps = code_chunker::python::extract_dependencies(root, &bytes);
    assert!(deps.contains(&"__future__".to_string()));
    assert!(deps.contains(&"json".to_string()));
    assert!(deps.contains(&"package.module".to_string()));
    assert!(deps.contains(&".local_module".to_string()));
    assert!(deps.contains(&"..sibling".to_string()));
}

#[test]
fn python_extracts_functions_and_classes() {
    let chunks = chunk_file(&sample("sample_python.py"), "test_repo");
    assert!(!chunks.is_empty());
    // All chunks must be Python or Generic-remainder.
    for c in &chunks {
        assert!(
            matches!(c.kind, CodeChunkKind::Python(..) | CodeChunkKind::Generic)
                || c.r#type == "file_remainder",
            "unexpected kind {:?} for chunk {:?}",
            c.kind,
            c.id
        );
    }
    for fn_name in &[
        "traced", "decorate", "identity", "describe", "fetch_records", "consume", "values",
        "stream_values", "yield_from_values", "callback", "__init__", "create", "parse", "label",
        "outer", "local", "format", "control_flow",
    ] {
        assert!(chunks.find_python_fn(fn_name, None).is_some(), "missing fn {fn_name}");
    }
    assert!(chunks.find_python_chunk_of_type("file_remainder").is_some());
}

// ---------------------------------------------------------------------------
// SQL
// ---------------------------------------------------------------------------

#[test]
fn sql_extracts_dependencies_from_references() {
    let bytes = std::fs::read(&sample("sample_sql.sql")).unwrap();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter::Language::from(tree_sitter_sequel::LANGUAGE))
        .unwrap();
    let tree = parser.parse(&bytes, None).unwrap();
    let root = tree.root_node();
    let deps = code_chunker::sql::extract_dependencies(root, &bytes);
    for tbl in &["departments", "customers", "orders", "products"] {
        assert!(deps.contains(&tbl.to_string()), "missing dependency {tbl}: {deps:?}");
    }
}

#[test]
fn sql_extracts_create_table_chunks() {
    let chunks = chunk_file(&sample("sample_sql.sql"), "test_repo");
    assert!(!chunks.is_empty());
    let stmt_types: Vec<String> = chunks
        .iter()
        .filter_map(|c| match &c.kind {
            CodeChunkKind::Sql(f) => f.statement_type.clone(),
            _ => None,
        })
        .collect();
    let create_table_chunks: Vec<&CodeChunk> = chunks
        .iter()
        .filter(|c| matches!(&c.kind, CodeChunkKind::Sql(f) if f.statement_type.as_deref() == Some("CREATE TABLE")))
        .collect();
    if create_table_chunks.is_empty() {
        eprintln!("SQL statement types found: {stmt_types:?}");
    }
    assert_eq!(create_table_chunks.len(), 3, "expected 3 CREATE TABLE chunks, got {}; statement_types found: {stmt_types:?}", create_table_chunks.len());
    let dbo_chunk = create_table_chunks
        .iter()
        .find(|c| {
            matches!(&c.kind, CodeChunkKind::Sql(f) if f.object_name.as_deref() == Some("customers"))
        })
        .copied()
        .expect("missing customers CREATE TABLE");
    let CodeChunkKind::Sql(f) = &dbo_chunk.kind else { panic!("not SQL") };
    assert_eq!(f.schema_name.as_deref(), Some("dbo"));
    assert!(dbo_chunk.code.contains("PRIMARY KEY"));
}

// ---------------------------------------------------------------------------
// Generic fallback
// ---------------------------------------------------------------------------

#[test]
fn generic_fallback_for_unknown_extension() {
    let chunks = chunk_file(&sample("sample_special_token.txt"), "test_repo");
    assert!(!chunks.is_empty());
    for c in &chunks {
        assert!(matches!(c.kind, CodeChunkKind::Generic));
    }
    assert!(chunks.iter().all(|c| c.repo_name == "test_repo"));
}