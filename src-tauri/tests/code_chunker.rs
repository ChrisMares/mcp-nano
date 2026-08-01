//! Integration tests for the code chunkers. Ports assertions from
//! VectorFlow `tests/test_code_chunk.py` against `tests/test_data/code_samples/`.
//!
//! Chunk size is 768 tokens with 64 overlap (Python `CODE_CHUNK_SIZE=768`).


use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mcp_nano_lib::services::ingestion::code_chunker;
use mcp_nano_lib::services::ingestion::code_chunker::oversized::split_oversized_code_chunks;
use mcp_nano_lib::services::ingestion::types::{CodeChunk, CodeChunkKind};

use tree_sitter::Parser;

fn sample_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test_data/code_samples")
}

fn sample(file: &str) -> PathBuf {
    sample_dir().join(file)
}

fn code_splitter() -> text_splitter::TextSplitter<tokenizers::Tokenizer> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources/models/arctic-embed-xs/tokenizer.json");
    let tok = tokenizers::Tokenizer::from_file(&path)
        .expect("arctic-embed-xs tokenizer required; run scripts/download-models.sh");
    let config = text_splitter::ChunkConfig::new(768)
        .with_sizer(tok)
        .with_overlap(64)
        .expect("chunk config");
    text_splitter::TextSplitter::new(config)
}

fn chunk_file(path: &Path, repo_name: &str) -> Vec<CodeChunk> {
    code_chunker::chunk_single_code_file(path, repo_name, None, &code_splitter(), 768)
}

fn sql_chunks(path: &Path) -> Vec<CodeChunk> {
    chunk_file(path, "test_repo")
        .into_iter()
        .filter(|c| matches!(c.kind, CodeChunkKind::Sql(_)))
        .collect()
}

fn sql_of_type<'a>(chunks: &'a [CodeChunk], stmt: &str) -> Vec<&'a CodeChunk> {
    chunks
        .iter()
        .filter(|c| matches!(&c.kind, CodeChunkKind::Sql(f) if f.statement_type.as_deref() == Some(stmt)))
        .collect()
}

fn sql_obj_names(chunks: &[&CodeChunk]) -> HashSet<String> {
    chunks
        .iter()
        .filter_map(|c| match &c.kind {
            CodeChunkKind::Sql(f) => f.object_name.clone(),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Rust
// ---------------------------------------------------------------------------

#[test]
fn rust_extracts_dependencies_and_fmt_chunk() {
    let chunks = chunk_file(&sample("sample_rust.rs"), "test_repo");
    assert!(!chunks.is_empty());
    let fmt = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::Rust(f) if f.function_name.as_deref() == Some("fmt")))
        .expect("missing fmt");
    let CodeChunkKind::Rust(f) = &fmt.kind else { panic!() };
    assert_eq!(f.class_name.as_deref(), Some("Wrapping<T>"));

    let bytes = std::fs::read(sample("sample_rust.rs")).unwrap();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter::Language::from(tree_sitter_rust::LANGUAGE))
        .unwrap();
    let tree = parser.parse(&bytes, None).unwrap();
    let deps = code_chunker::rust_lang::extract_dependencies(tree.root_node(), &bytes);
    assert_eq!(
        deps,
        vec![
            "crate::fmt".to_string(),
            "crate::ops::{\n    Add, AddAssign, BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Div, DivAssign,\n    Mul, MulAssign, Neg, Not, Rem, RemAssign, Shl, ShlAssign, Shr, ShrAssign, Sub, SubAssign,\n}".to_string(),
        ]
    );
}

#[test]
fn rust_struct_enum_trait_and_poco() {
    let chunks = chunk_file(&sample("sample_rust.rs"), "test_repo");
    let struct_chunk = chunks
        .iter()
        .find(|c| c.r#type == "struct_item" && matches!(&c.kind, CodeChunkKind::Rust(f) if f.class_name.as_deref() == Some("Wrapping")))
        .expect("struct Wrapping");
    assert!(struct_chunk.code.contains("pub struct Wrapping"));

    assert!(chunks.iter().any(|c| {
        c.r#type == "enum_item"
            && matches!(&c.kind, CodeChunkKind::Rust(f) if f.class_name.as_deref() == Some("ShiftDirection"))
            && c.code.contains("Left")
    }));
    assert!(chunks.iter().any(|c| {
        c.r#type == "trait_item"
            && matches!(&c.kind, CodeChunkKind::Rust(f) if f.class_name.as_deref() == Some("WrappingArith"))
            && c.code.contains("wrapping_add")
    }));

    let poco = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::Rust(f) if f.class_name.as_deref() == Some("CustomerProfile")))
        .expect("CustomerProfile");
    for field in ["CustomerProfile", "first_name", "last_name", "age", "email", "is_active"] {
        assert!(poco.code.contains(field), "missing {field}");
    }
    let embed = poco.chunk_embedding_text();
    assert!(embed.contains("CustomerProfile"));
    assert!(embed.contains("first_name"));
}

#[test]
fn rust_remainder_excludes_extracted_fn() {
    let chunks = chunk_file(&sample("sample_rust.rs"), "test_repo");
    let rem = chunks.iter().find(|c| c.r#type == "file_remainder").unwrap();
    assert!(!rem.code.contains("fn fmt"));
}

// ---------------------------------------------------------------------------
// C
// ---------------------------------------------------------------------------

#[test]
fn c_functions_params_deps_and_remainder() {
    let chunks = chunk_file(&sample("sample_c.c"), "test_repo");
    let names: HashSet<_> = chunks
        .iter()
        .filter_map(|c| match &c.kind {
            CodeChunkKind::C(f) => f.function_name.clone(),
            _ => None,
        })
        .collect();
    assert!(names.contains("__redisReaderSetError"));
    assert!(names.contains("string2ll"));
    assert!(names.contains("processItem"));

    let string2ll = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::C(f) if f.function_name.as_deref() == Some("string2ll")))
        .unwrap();
    let CodeChunkKind::C(f) = &string2ll.kind else { panic!() };
    let pnames: Vec<_> = f.parameters.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(&pnames[..3], &["s", "slen", "value"]);
    assert_eq!(f.return_type.as_deref(), Some("int"));

    let bytes = std::fs::read(sample("sample_c.c")).unwrap();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter::Language::from(tree_sitter_c::LANGUAGE))
        .unwrap();
    let tree = parser.parse(&bytes, None).unwrap();
    let deps = code_chunker::c::extract_dependencies(tree.root_node(), &bytes);
    assert_eq!(
        deps,
        vec![
            "fmacros.h", "string.h", "stdlib.h", "unistd.h", "strings.h", "assert.h",
            "errno.h", "ctype.h", "limits.h", "math.h", "alloc.h", "read.h", "sds.h", "win32.h",
        ]
    );

    let rem = chunks.iter().find(|c| c.r#type == "file_remainder").unwrap();
    assert!(rem.code.contains("REDIS_READER_STACK_SIZE"));
    assert!(!rem.code.contains("string2ll"));
}

#[test]
fn c_header_and_struct_in_remainder() {
    let chunks = chunk_file(&sample("sample_c.h"), "test_repo");
    assert!(chunks.iter().any(|c| {
        matches!(&c.kind, CodeChunkKind::C(f) if f.function_name.as_deref() == Some("sdslen"))
    }));
    let rem = chunks.iter().find(|c| c.r#type == "file_remainder").unwrap();
    assert!(rem.code.contains("struct __attribute__") || rem.code.contains("CustomerProfile"));
    assert!(rem.code.contains("CustomerProfile"));
    assert!(rem.code.contains("firstName"));
}

// ---------------------------------------------------------------------------
// C++
// ---------------------------------------------------------------------------

#[test]
fn cpp_functions_class_and_poco() {
    let chunks = chunk_file(&sample("sample_cpp.cpp"), "test_repo");
    let swap = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::Cpp(f) if f.function_name.as_deref() == Some("swap")))
        .unwrap();
    let CodeChunkKind::Cpp(f) = &swap.kind else { panic!() };
    assert_eq!(f.class_name.as_deref(), Some("UUID"));
    assert_eq!(f.namespace.as_deref(), Some("Poco"));
    assert!(chunks.iter().any(|c| {
        matches!(&c.kind, CodeChunkKind::Cpp(f) if f.function_name.as_deref() == Some("toString"))
    }));

    let hdr = chunk_file(&sample("sample_cpp.hpp"), "test_repo");
    assert!(hdr.iter().any(|c| {
        matches!(&c.kind, CodeChunkKind::Cpp(f) if f.function_name.as_deref() == Some("flipBytes")
            && f.class_name.as_deref() == Some("ByteOrder")
            && f.namespace.as_deref() == Some("Poco"))
    }));
    assert!(hdr.iter().any(|c| {
        c.r#type == "class_specifier"
            && matches!(&c.kind, CodeChunkKind::Cpp(f) if f.class_name.as_deref() == Some("ByteOrder"))
    }));
    let poco = hdr
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::Cpp(f) if f.class_name.as_deref() == Some("CustomerProfile")))
        .expect("CustomerProfile struct");
    assert_eq!(poco.r#type, "struct_specifier");
    for field in ["CustomerProfile", "firstName", "lastName", "age", "email", "isActive"] {
        assert!(poco.code.contains(field));
    }
}

// ---------------------------------------------------------------------------
// Java
// ---------------------------------------------------------------------------

#[test]
fn java_methods_types_and_poco() {
    let chunks = chunk_file(&sample("sample_java.java"), "test_repo");
    let state = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::Java(f) if f.function_name.as_deref() == Some("state") && f.class_name.as_deref() == Some("Assert")))
        .unwrap();
    let CodeChunkKind::Java(f) = &state.kind else { panic!() };
    assert_eq!(f.namespace.as_deref(), Some("com.example.demo"));
    assert!(chunks.iter().any(|c| c.r#type == "class_declaration"
        && matches!(&c.kind, CodeChunkKind::Java(f) if f.class_name.as_deref() == Some("Assert"))));
    assert!(chunks.iter().any(|c| c.r#type == "interface_declaration"
        && matches!(&c.kind, CodeChunkKind::Java(f) if f.class_name.as_deref() == Some("Validatable"))
        && c.code.contains("isValid")));
    assert!(chunks.iter().any(|c| c.r#type == "enum_declaration"
        && matches!(&c.kind, CodeChunkKind::Java(f) if f.class_name.as_deref() == Some("AssertionSeverity"))
        && c.code.contains("FATAL")));
    let poco = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::Java(f) if f.class_name.as_deref() == Some("CustomerProfile")))
        .unwrap();
    for field in ["CustomerProfile", "firstName", "lastName", "age", "email", "active"] {
        assert!(poco.code.contains(field));
    }
}

// ---------------------------------------------------------------------------
// JavaScript / TypeScript / React
// ---------------------------------------------------------------------------

#[test]
fn js_functions_class_method_and_remainder() {
    let chunks = chunk_file(&sample("sample_js.js"), "test_repo");
    for name in ["isBuffer", "forEach", "findKey", "merge", "getFullName"] {
        assert!(
            chunks.iter().any(|c| matches!(&c.kind, CodeChunkKind::JavaScript(f) if f.function_name.as_deref() == Some(name))),
            "missing {name}"
        );
    }
    let full = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::JavaScript(f) if f.function_name.as_deref() == Some("getFullName")))
        .unwrap();
    let CodeChunkKind::JavaScript(f) = &full.kind else { panic!() };
    assert_eq!(f.class_name.as_deref(), Some("CustomerProfile"));
    let rem = chunks.iter().find(|c| c.r#type == "file_remainder").unwrap();
    assert!(rem.code.contains("const {isArray}") || rem.code.contains("CustomerProfile"));
    assert!(!rem.code.contains("function merge"));
}

#[test]
fn ts_functions_types_and_poco_interface() {
    let chunks = chunk_file(&sample("sample_ts.ts"), "test_repo");
    for name in ["makeIssue", "addIssueToContext", "mergeArray"] {
        assert!(chunks.iter().any(|c| {
            matches!(&c.kind, CodeChunkKind::TypeScript(f) if f.function_name.as_deref() == Some(name))
        }));
    }
    let make = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::TypeScript(f) if f.function_name.as_deref() == Some("makeIssue")))
        .unwrap();
    let CodeChunkKind::TypeScript(f) = &make.kind else { panic!() };
    assert_eq!(f.parameters.len(), 1);
    assert_eq!(f.return_type.as_deref(), Some("ZodIssue"));
    assert!(chunks.iter().any(|c| {
        c.r#type == "interface_declaration"
            && matches!(&c.kind, CodeChunkKind::TypeScript(f) if f.class_name.as_deref() == Some("ParseContext"))
    }));
    assert!(chunks.iter().any(|c| {
        c.r#type == "type_alias_declaration"
            && matches!(&c.kind, CodeChunkKind::TypeScript(f) if f.class_name.as_deref() == Some("ParseParams"))
    }));
    assert!(chunks.iter().any(|c| {
        c.r#type == "enum_declaration"
            && matches!(&c.kind, CodeChunkKind::TypeScript(f) if f.class_name.as_deref() == Some("ParseStatusCode"))
            && c.code.contains("Valid")
    }));
    let iface = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::TypeScript(f) if f.class_name.as_deref() == Some("CustomerProfile")))
        .unwrap();
    for field in ["CustomerProfile", "firstName", "lastName", "age", "email", "isActive"] {
        assert!(iface.code.contains(field));
    }
}

#[test]
fn react_components_hooks_export_and_inner() {
    let chunks = chunk_file(&sample("sample_react.tsx"), "test_repo");
    let app = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::React(f) if f.component_name.as_deref() == Some("App")))
        .unwrap();
    let CodeChunkKind::React(f) = &app.kind else { panic!() };
    assert!(f.is_exported);
    assert_eq!(app.r#type, "functional_component");
    let layout = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::React(f) if f.component_name.as_deref() == Some("Layout")))
        .unwrap();
    let CodeChunkKind::React(lf) = &layout.kind else { panic!() };
    assert!(!lf.is_exported);
    assert!(chunks.iter().any(|c| {
        c.r#type == "hook"
            && matches!(&c.kind, CodeChunkKind::React(f) if f.component_name.as_deref() == Some("useWindowSize") && f.is_exported)
    }));

    let inner = chunk_file(&sample("sample_react_inner.tsx"), "test_repo");
    assert!(inner.iter().any(|c| {
        c.r#type == "function"
            && matches!(&c.kind, CodeChunkKind::React(f) if f.component_name.as_deref() == Some("helper"))
    }));
    assert!(inner.iter().any(|c| {
        c.r#type == "function"
            && matches!(&c.kind, CodeChunkKind::React(f) if f.component_name.as_deref() == Some("helper2"))
    }));
    let rem = inner.iter().find(|c| c.r#type == "file_remainder").unwrap();
    assert!(!rem.code.contains("function Widget"));
    assert!(rem.code.contains("import"));
}

// ---------------------------------------------------------------------------
// C#
// ---------------------------------------------------------------------------

#[test]
fn csharp_records_struct_enum_poco_and_modern() {
    let chunks = chunk_file(&sample("sample_csharp.cs"), "test_repo");
    assert!(chunks.iter().any(|c| matches!(&c.kind, CodeChunkKind::CSharp(f) if f.function_name.as_deref() == Some("Encode"))));
    assert!(chunks.iter().any(|c| matches!(&c.kind, CodeChunkKind::CSharp(f) if f.function_name.as_deref() == Some("Equals"))));

    let record = chunks
        .iter()
        .find(|c| {
            c.r#type == "record_declaration"
                && matches!(&c.kind, CodeChunkKind::CSharp(f) if f.class_name.as_deref() == Some("MaxDiscountLogItemSummary"))
        })
        .unwrap();
    let CodeChunkKind::CSharp(rf) = &record.kind else { panic!() };
    let props: HashSet<_> = rf.properties.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        props,
        HashSet::from([
            "ItemNumber",
            "PcatName",
            "TotalQuantity",
            "TotalDollarDiscount",
            "AvgDiscountPct",
        ])
    );

    let pos = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::CSharp(f) if f.class_name.as_deref() == Some("MaxDicount2")))
        .unwrap();
    let CodeChunkKind::CSharp(pf) = &pos.kind else { panic!() };
    assert!(pf.properties.iter().any(|p| p.r#type == "string"));
    assert!(pf.properties.iter().any(|p| p.r#type == "decimal"));

    assert!(chunks.iter().any(|c| {
        c.r#type == "struct_declaration"
            && matches!(&c.kind, CodeChunkKind::CSharp(f) if f.class_name.as_deref() == Some("JsonEncodedText"))
    }));
    assert!(chunks.iter().any(|c| {
        c.r#type == "enum_declaration"
            && matches!(&c.kind, CodeChunkKind::CSharp(f) if f.class_name.as_deref() == Some("JsonTokenType"))
    }));
    assert!(chunks.iter().any(|c| {
        c.r#type == "delegate_declaration"
            && matches!(&c.kind, CodeChunkKind::CSharp(f) if f.class_name.as_deref() == Some("JsonWriteCallback"))
    }));

    let poco = chunks
        .iter()
        .find(|c| {
            c.r#type == "class_declaration"
                && matches!(&c.kind, CodeChunkKind::CSharp(f) if f.class_name.as_deref() == Some("CustomerProfile"))
        })
        .unwrap();
    let CodeChunkKind::CSharp(cf) = &poco.kind else { panic!() };
    let prop_map: std::collections::HashMap<_, _> = cf
        .properties
        .iter()
        .map(|p| (p.name.as_str(), p.r#type.as_str()))
        .collect();
    assert_eq!(prop_map.get("FirstName"), Some(&"string"));
    assert_eq!(prop_map.get("Age"), Some(&"int"));
    assert_eq!(prop_map.get("IsActive"), Some(&"bool"));

    let modern = chunk_file(&sample("sample_csharp_modern.cs"), "test_repo");
    assert!(!modern.is_empty());
    let types: HashSet<_> = modern.iter().map(|c| c.r#type.as_str()).collect();
    assert!(types.contains("class_declaration"));
    assert!(types.contains("interface_declaration"));
    assert!(types.contains("record_declaration"));
    assert!(types.contains("enum_declaration"));
    assert!(modern.iter().any(|c| matches!(&c.kind, CodeChunkKind::CSharp(f) if f.function_name.as_deref() == Some("CalculateTotal"))));
    assert!(modern.iter().any(|c| matches!(&c.kind, CodeChunkKind::CSharp(f) if f.function_name.as_deref() == Some("IsValid"))));
    assert!(modern.iter().any(|c| matches!(&c.kind, CodeChunkKind::CSharp(f) if f.interface_name.as_deref() == Some("IOrderRepository"))));
}

// ---------------------------------------------------------------------------
// Python
// ---------------------------------------------------------------------------

#[test]
fn python_dependencies_exact_order() {
    let bytes = std::fs::read(sample("sample_python.py")).unwrap();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter::Language::from(tree_sitter_python::LANGUAGE))
        .unwrap();
    let tree = parser.parse(&bytes, None).unwrap();
    let deps = code_chunker::python::extract_dependencies(tree.root_node(), &bytes);
    assert_eq!(
        deps,
        vec![
            "__future__",
            "json",
            "os",
            "package.module",
            "collections.abc",
            "dataclasses",
            "typing",
            ".local_module",
            ".",
            "..sibling",
            "package",
        ]
    );
}

#[test]
fn python_functions_classes_signatures_and_remainder() {
    let chunks = chunk_file(&sample("sample_python.py"), "test_repo");
    assert!(chunks.iter().all(|c| matches!(c.kind, CodeChunkKind::Python(_)) || c.r#type == "file_remainder"));
    for name in [
        "traced", "decorate", "identity", "describe", "fetch_records", "consume", "values",
        "stream_values", "yield_from_values", "callback", "__init__", "create", "parse", "label",
        "outer", "local", "format", "control_flow",
    ] {
        assert!(
            chunks.iter().any(|c| matches!(&c.kind, CodeChunkKind::Python(f) if f.function_name.as_deref() == Some(name))),
            "missing {name}"
        );
    }
    assert!(chunks.iter().any(|c| matches!(&c.kind, CodeChunkKind::Python(f) if f.class_name.as_deref() == Some("Service"))));
    assert!(chunks.iter().any(|c| matches!(&c.kind, CodeChunkKind::Python(f) if f.class_name.as_deref() == Some("LocalFormatter"))));

    let describe = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::Python(f) if f.function_name.as_deref() == Some("describe")))
        .unwrap();
    let CodeChunkKind::Python(df) = &describe.kind else { panic!() };
    assert_eq!(
        df.parameters.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["item", "/", "count", "labels", "enabled", "options"]
    );
    assert_eq!(df.return_type.as_deref(), Some("dict[str, object]"));
    assert_eq!(df.decorators, vec!["@traced(\"sync\")"]);

    let service = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::Python(f) if f.class_name.as_deref() == Some("Service") && f.function_name.is_none() && !f.is_type_alias))
        .unwrap();
    let CodeChunkKind::Python(sf) = &service.kind else { panic!() };
    assert_eq!(sf.type_parameters.as_deref(), Some("[T]"));
    assert_eq!(sf.bases.as_deref(), Some("BaseService, metaclass=type"));
    let props: std::collections::HashMap<_, _> = sf
        .properties
        .iter()
        .map(|p| (p.name.as_str(), p.r#type.as_str()))
        .collect();
    assert_eq!(props.get("identifier"), Some(&"int"));
    assert_eq!(props.get("name"), Some(&"str"));
    assert_eq!(props.get("cache"), Some(&""));
    assert!(!props.contains_key("Alias"));
    assert!(!service.code.contains("def __init__"));

    let values = chunks
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::Python(f) if f.function_name.as_deref() == Some("values")))
        .unwrap();
    let CodeChunkKind::Python(vf) = &values.kind else { panic!() };
    assert!(vf.is_generator);

    let rem = chunks.iter().find(|c| c.r#type == "file_remainder").unwrap();
    assert!(matches!(rem.kind, CodeChunkKind::Python(_)));
    assert!(rem.code.contains("CONSTANT = \"module remainder\"") || rem.code.contains("CONSTANT = 'module remainder'") || rem.code.contains("module remainder"));
    assert!(rem.code.contains("match CONSTANT:"));
}

#[test]
fn python_realworld_sources() {
    let cases = [
        ("realworld_fastapi_api_key.py", "__call__", Some("APIKeyQuery")),
        ("realworld_httpx_auth.py", "async_auth_flow", Some("Auth")),
        ("realworld_pydantic_decorator.py", "validate_arguments", None),
    ];
    for (file, fn_name, class_name) in cases {
        let chunks = chunk_file(&sample(file), "real_world");
        assert!(chunks.iter().any(|c| {
            matches!(&c.kind, CodeChunkKind::Python(f)
                if f.function_name.as_deref() == Some(fn_name)
                && (class_name.is_none() || f.class_name.as_deref() == class_name))
        }), "{file}");
    }
}

// ---------------------------------------------------------------------------
// SQL dialects
// ---------------------------------------------------------------------------

#[test]
fn sql_sample_core_statements() {
    let chunks = sql_chunks(&sample("sample_sql.sql"));
    assert!(chunks.len() >= 16);
    let tables = sql_of_type(&chunks, "CREATE TABLE");
    assert_eq!(tables.len(), 3);
    assert_eq!(
        sql_obj_names(&tables),
        HashSet::from(["customers".into(), "orders".into(), "order_items".into()])
    );
    let customers = tables
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::Sql(f) if f.object_name.as_deref() == Some("customers")))
        .unwrap();
    let CodeChunkKind::Sql(cf) = &customers.kind else { panic!() };
    assert_eq!(cf.schema_name.as_deref(), Some("dbo"));
    assert!(customers.code.contains("PRIMARY KEY"));

    assert_eq!(sql_of_type(&chunks, "CREATE VIEW").len(), 2);
    assert_eq!(sql_of_type(&chunks, "CREATE FUNCTION").len(), 1);
    assert_eq!(sql_of_type(&chunks, "CREATE TRIGGER").len(), 1);
    assert_eq!(sql_of_type(&chunks, "CREATE INDEX").len(), 2);
    assert_eq!(sql_of_type(&chunks, "CREATE SEQUENCE").len(), 1);
    let types: HashSet<_> = chunks
        .iter()
        .filter_map(|c| match &c.kind {
            CodeChunkKind::Sql(f) => f.statement_type.clone(),
            _ => None,
        })
        .collect();
    for t in ["INSERT", "SELECT", "UPDATE", "DELETE"] {
        assert!(types.contains(t), "missing {t}");
    }
}

#[test]
fn sql_sfa_schema_counts_and_metadata() {
    let chunks = sql_chunks(&sample("SFA_Schema.sql"));
    assert_eq!(chunks.len(), 61);
    assert_eq!(sql_of_type(&chunks, "CREATE TABLE").len(), 22);
    assert_eq!(sql_of_type(&chunks, "CREATE VIEW").len(), 24);
    assert_eq!(sql_of_type(&chunks, "CREATE PROCEDURE").len(), 4);
    assert_eq!(sql_of_type(&chunks, "ALTER TABLE").len(), 11);
    assert!(!chunks.iter().any(|c| {
        matches!(&c.kind, CodeChunkKind::Sql(f) if f.statement_type.as_deref().map(|s| s.eq_ignore_ascii_case("MERGE")).unwrap_or(false))
    }));
    for c in &chunks {
        let CodeChunkKind::Sql(f) = &c.kind else { panic!() };
        assert_eq!(f.database_name.as_deref(), Some("SFA"));
        assert_eq!(f.schema_name.as_deref(), Some("dbo"));
        assert!(!c.code.lines().any(|l| l.trim().eq_ignore_ascii_case("GO")));
        assert!(!c.code.to_uppercase().contains("SET ANSI_NULLS"));
    }
}

#[test]
fn sql_postgresql_oracle_sqlite_dialects() {
    let pg = sql_chunks(&sample("sample_postgresql.sql"));
    assert_eq!(pg.len(), 18);
    assert_eq!(sql_of_type(&pg, "CREATE EXTENSION").len(), 2);
    assert_eq!(sql_of_type(&pg, "CREATE TYPE").len(), 2);
    assert_eq!(sql_of_type(&pg, "CREATE TABLE").len(), 3);
    assert_eq!(sql_of_type(&pg, "CREATE POLICY").len(), 2);
    let funcs = sql_of_type(&pg, "CREATE OR REPLACE FUNCTION");
    assert_eq!(funcs.len(), 2);
    let audit = funcs
        .iter()
        .find(|c| matches!(&c.kind, CodeChunkKind::Sql(f) if f.object_name.as_deref() == Some("create_audit_trigger")))
        .unwrap();
    assert!(audit.code.contains("$fn$"));
    assert!(audit.code.contains("$body$"));

    let ora = sql_chunks(&sample("sample_oracle.sql"));
    assert_eq!(ora.len(), 15);
    assert_eq!(sql_of_type(&ora, "CREATE TABLESPACE").len(), 1);
    assert_eq!(sql_of_type(&ora, "CREATE SEQUENCE").len(), 2);
    assert!(ora.iter().any(|c| matches!(&c.kind, CodeChunkKind::Sql(f) if f.statement_type.as_deref() == Some("CREATE OR REPLACE PACKAGE"))));
    assert!(ora.iter().any(|c| matches!(&c.kind, CodeChunkKind::Sql(f) if f.statement_type.as_deref() == Some("CREATE OR REPLACE PACKAGE BODY"))));
    for c in &ora {
        assert!(!c.code.lines().any(|l| l.trim() == "/"));
    }

    let lite = sql_chunks(&sample("sample_sqlite.sql"));
    assert_eq!(lite.len(), 16);
    assert_eq!(sql_of_type(&lite, "CREATE TABLE").len(), 6);
    assert_eq!(sql_of_type(&lite, "CREATE VIRTUAL TABLE").len(), 1);
    assert_eq!(sql_of_type(&lite, "CREATE TRIGGER").len(), 5);
    assert_eq!(sql_of_type(&lite, "CREATE INDEX").len(), 4);
    let analytics: Vec<_> = lite
        .iter()
        .filter(|c| matches!(&c.kind, CodeChunkKind::Sql(f) if f.schema_name.as_deref() == Some("analytics")))
        .collect();
    assert_eq!(analytics.len(), 2);
}

// ---------------------------------------------------------------------------
// Pipeline / oversized / generic
// ---------------------------------------------------------------------------

#[test]
fn chunk_single_sets_repo_and_override() {
    let chunks = chunk_file(&sample("sample_js.js"), "test_repo");
    assert!(chunks.iter().all(|c| c.repo_name == "test_repo" && c.file_name == "sample_js.js"));
    let overridden = code_chunker::chunk_single_code_file(
        &sample("sample_js.js"),
        "test_repo",
        Some("my_original.js"),
        &code_splitter(),
        768,
    );
    assert!(overridden.iter().all(|c| c.file_name == "my_original.js"));
}

#[test]
fn generic_fallback_and_oversized_split() {
    let chunks = chunk_file(&sample("sample_special_token.txt"), "test_repo");
    assert!(!chunks.is_empty());
    assert!(chunks.iter().all(|c| matches!(c.kind, CodeChunkKind::Generic)));

    let big = CodeChunk::generic("orig-1", "my_repo", "big.py", "x = 1\n".repeat(2000));
    let split = split_oversized_code_chunks(vec![big], &code_splitter());
    assert!(split.len() > 1);
    assert!(split.iter().all(|c| c.repo_name == "my_repo" && c.file_name == "big.py"));

    let small = CodeChunk::generic("small-1", "r", "f.py", "x = 1");
    let passthrough = split_oversized_code_chunks(vec![small], &code_splitter());
    assert_eq!(passthrough.len(), 1);
    assert_eq!(passthrough[0].id, "small-1");
}

// ---------------------------------------------------------------------------
// Chart.js fixture that previously hung/panicked mid zip ingest (file 585/1068)
// ---------------------------------------------------------------------------

#[test]
fn chartjs_radar_radius_indexable_chunks_without_panic() {
    let path = sample("chartjs_radar_radius_indexable.js");
    assert!(path.is_file(), "missing fixture {}", path.display());

    let code_chunks = chunk_file(&path, "Chart.js-4.5.1");
    assert!(
        !code_chunks.is_empty(),
        "expected at least one code chunk from chartjs fixture"
    );
    assert!(code_chunks.iter().all(|c| c.repo_name == "Chart.js-4.5.1"));
    assert!(code_chunks
        .iter()
        .all(|c| c.file_name == "chartjs_radar_radius_indexable.js"));

    let docs = code_chunker::chunk_file_to_documents(
        &path,
        "Chart.js-4.5.1",
        None,
        &code_splitter(),
        768,
    );
    assert!(!docs.is_empty(), "chunk_file_to_documents returned no docs");
    let joined: String = docs.iter().map(|d| d.content.as_str()).collect();
    assert!(joined.contains("radar"), "missing radar marker: {joined}");
    assert!(
        joined.contains("pointRadius"),
        "missing pointRadius marker: {joined}"
    );
    assert!(
        joined.contains("module.exports"),
        "missing module.exports marker: {joined}"
    );
}

