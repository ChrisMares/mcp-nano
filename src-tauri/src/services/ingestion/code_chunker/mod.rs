//! Code-aware chunker dispatch. Direct port of
//! `embedding/code_embedder/chunk_code_base.py`.
//!
//! Each language lives in its own submodule (`c.rs`, `cpp.rs`, ...) and
//! exposes `extract_dependencies(root, src) -> Vec<String>` plus
//! `extract_chunks(root, src, file_path, deps) -> Vec<CodeChunk>` names.
//! The [`Language`] enum + `match` in [`extract_for`] maps a file extension
//! to its chunker pair.
//!
//! The shared entry point is [`chunk_single_code_file`]: parse one file,
//! dispatch by extension.

use std::path::Path;

use tree_sitter::{Language as TsLanguage, Node, Parser};

use super::types::{CodeChunk, DocumentChunk};

pub mod c;
pub mod cpp;
pub mod csharp;
pub mod helpers;
pub mod java;
pub mod js;
pub mod oversized;
pub mod python;
pub mod react;
pub mod rust_lang;
pub mod sql;
pub mod ts;

/// Per-language dispatch enum. Avoids the higher-ranked trait-bound
/// explosion that comes with `fn(Node<'_>, &[u8]) -> Vec<String>` function
/// pointers; we dispatch on this enum via a match instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    C,
    Cpp,
    CSharp,
    Java,
    JavaScript,
    TypeScript,
    TypeScriptTsx,
    Rust,
    Python,
    Sql,
}

impl Language {
    pub fn tree_sitter_language(self) -> TsLanguage {
        match self {
            Language::C => TsLanguage::from(tree_sitter_c::LANGUAGE),
            Language::Cpp => TsLanguage::from(tree_sitter_cpp::LANGUAGE),
            Language::CSharp => TsLanguage::from(tree_sitter_c_sharp::LANGUAGE),
            Language::Java => TsLanguage::from(tree_sitter_java::LANGUAGE),
            Language::JavaScript => TsLanguage::from(tree_sitter_javascript::LANGUAGE),
            Language::TypeScript => TsLanguage::from(tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
            Language::TypeScriptTsx => TsLanguage::from(tree_sitter_typescript::LANGUAGE_TSX),
            Language::Rust => TsLanguage::from(tree_sitter_rust::LANGUAGE),
            Language::Python => TsLanguage::from(tree_sitter_python::LANGUAGE),
            Language::Sql => TsLanguage::from(tree_sitter_sequel::LANGUAGE),
        }
    }

    pub fn extract_dependencies<'a>(self, root: Node<'a>, source: &[u8]) -> Vec<String> {
        match self {
            Language::C => c::extract_dependencies(root, source),
            Language::Cpp => cpp::extract_dependencies(root, source),
            Language::CSharp => csharp::extract_dependencies(root, source),
            Language::Java => java::extract_dependencies(root, source),
            Language::JavaScript => js::extract_dependencies(root, source),
            Language::TypeScript => ts::extract_dependencies(root, source),
            Language::TypeScriptTsx => react::extract_dependencies(root, source),
            Language::Rust => rust_lang::extract_dependencies(root, source),
            Language::Python => python::extract_dependencies(root, source),
            Language::Sql => sql::extract_dependencies(root, source),
        }
    }

    pub fn extract_chunks<'a>(
        self,
        root: Node<'a>,
        source: &[u8],
        file_path: &str,
        deps: &[String],
    ) -> Vec<CodeChunk> {
        match self {
            Language::C => c::extract_chunks(root, source, file_path, deps),
            Language::Cpp => cpp::extract_chunks(root, source, file_path, deps),
            Language::CSharp => csharp::extract_chunks(root, source, file_path, deps),
            Language::Java => java::extract_chunks(
                root,
                source,
                file_path,
                deps,
                java::extract_package(&root, source).as_deref(),
            ),
            Language::JavaScript => js::extract_chunks(root, source, file_path, deps),
            Language::TypeScript => ts::extract_chunks(root, source, file_path, deps),
            Language::TypeScriptTsx => react::extract_chunks(root, source, file_path, deps),
            Language::Rust => rust_lang::extract_chunks(root, source, file_path, deps),
            Language::Python => python::extract_chunks(root, source, file_path, deps),
            Language::Sql => sql::extract_chunks(root, source, file_path, deps),
        }
    }
}

/// Map a file extension to its chunker `Language`. Returns `None` for
/// extensions with no dedicated chunker (they fall back to the generic
/// token-based splitter in [`chunk_single_code_file`]).
pub fn language_for_extension(ext: &str) -> Option<Language> {
    Some(match ext {
        "c" | "h" => Language::C,
        "cpp" | "hpp" | "cc" | "cxx" | "hh" => Language::Cpp,
        "cs" => Language::CSharp,
        "java" => Language::Java,
        "ts" => Language::TypeScript,
        "tsx" | "jsx" => Language::TypeScriptTsx,
        "js" | "mjs" | "cjs" => Language::JavaScript,
        "py" | "pyi" => Language::Python,
        "rs" => Language::Rust,
        "sql" => Language::Sql,
        _ => return None,
    })
}

/// True if the extension has a dedicated tree-sitter chunker.
pub fn is_code_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .map(|e| language_for_extension(&e).is_some())
        .unwrap_or(false)
}

/// Chunk a single source file. Mirrors `chunk_single_code_file`.
///
/// Returns a list of language-specific `CodeChunk`s (or generic
/// `CodeChunkGeneric` chunks for unknown extensions). On a parse error
/// for a known extension, falls back to the generic splitter.
pub fn chunk_single_code_file(
    file_path: &Path,
    repo_name: &str,
    file_name_override: Option<&str>,
    splitter: &text_splitter::TextSplitter<tokenizers::Tokenizer>,
    max_chunk_tokens: usize,
) -> Vec<CodeChunk> {
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    let mut chunks: Vec<CodeChunk> = if let Some(lang) = language_for_extension(&ext) {
        chunk_with_language(file_path, lang)
    } else {
        Vec::new()
    };

    if chunks.is_empty() {
        chunks = chunk_generic_file(file_path, repo_name, splitter, max_chunk_tokens);
    }

    let final_name = file_name_override
        .map(|s| s.to_string())
        .or_else(|| {
            file_path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();
    for chunk in &mut chunks {
        chunk.repo_name = repo_name.to_string();
        chunk.file_name = final_name.clone();
        chunk.code = super::types::clean_code(&chunk.code);
    }
    chunks
}

fn chunk_with_language(file_path: &Path, lang: Language) -> Vec<CodeChunk> {
    let Ok(source) = read_file_text(file_path) else {
        return Vec::new();
    };

    let mut database_name: Option<String> = None;
    let mut processed_source = source;
    if file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .map(|s| s == "sql")
        .unwrap_or(false)
    {
        let (clean, db) = helpers::strip_sql_boilerplate(&processed_source);
        processed_source = helpers::strip_sql_comments(&clean);
        database_name = db;
    }

    let source_bytes = processed_source.into_bytes();
    let mut parser = Parser::new();
    if let Err(e) = parser.set_language(&lang.tree_sitter_language()) {
        tracing::error!("tree-sitter parser init failed for {}: {e}", file_path.display());
        return Vec::new();
    }
    let Some(tree) = parser.parse(&source_bytes, None) else {
        return Vec::new();
    };
    let root = tree.root_node();
    let deps = lang.extract_dependencies(root, &source_bytes);
    if lang == Language::Sql {
        sql::extract_chunks_with_db(
            root,
            &source_bytes,
            file_path.to_str().unwrap_or(""),
            &deps,
            database_name.as_deref(),
        )
    } else {
        lang.extract_chunks(root, &source_bytes, file_path.to_str().unwrap_or(""), &deps)
    }
}

/// Generic fallback: token-based chunking (mirrors Python `_parse_generic_file`).
fn chunk_generic_file(
    file_path: &Path,
    repo_name: &str,
    splitter: &text_splitter::TextSplitter<tokenizers::Tokenizer>,
    _max_chunk_tokens: usize,
) -> Vec<CodeChunk> {
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let Ok(code) = read_file_text(file_path) else {
        return Vec::new();
    };
    if code.is_empty() || code.contains('\u{0}') {
        return Vec::new();
    }
    splitter
        .chunks(&code)
        .map(|c| {
            CodeChunk::generic(
                uuid::Uuid::new_v4().to_string(),
                repo_name,
                &file_name,
                c.to_string(),
            )
        })
        .collect()
}

/// Detect a file's encoding from its BOM or first 4 bytes. Mirrors
/// `_detect_encoding`.
pub fn detect_encoding(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] == 0xfe {
        return "utf-16-le";
    }
    if bytes.len() >= 2 && bytes[0] == 0xfe && bytes[1] == 0xff {
        return "utf-16-be";
    }
    if bytes.len() >= 3 && bytes[0] == 0xef && bytes[1] == 0xbb && bytes[2] == 0xbf {
        return "utf-8";
    }
    if bytes.len() >= 4 && bytes[1] == 0x00 && bytes[3] == 0x00 {
        return "utf-16-le";
    }
    "utf-8"
}

/// Read a file with the auto-detected encoding. Mirrors `_read_file_text`.
pub fn read_file_text(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    let enc = detect_encoding(&bytes);
    let text = match enc {
        "utf-16-le" => decode_utf16(&bytes, true),
        "utf-16-be" => decode_utf16(&bytes, false),
        _ => String::from_utf8_lossy(&bytes).into_owned(),
    };
    Ok(text)
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    let pairs: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| {
            if little_endian {
                u16::from_le_bytes([c[0], c[1]])
            } else {
                u16::from_be_bytes([c[0], c[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&pairs)
}

/// Convert a `CodeChunk` list into `DocumentChunk`s for the uniform
/// embed+upsert pipeline. The language-specific metadata is folded into the
/// per-chunk `metadata` map. Used by `IngestionService::ingest_code_file`.
pub fn code_chunks_to_document_chunks(code_chunks: Vec<CodeChunk>) -> Vec<DocumentChunk> {
    code_chunks
        .into_iter()
        .enumerate()
        .map(|(chunk_index, cc)| {
            let metadata = cc.chunk_metadata();
            let ext = Path::new(&cc.file_name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("txt")
                .to_lowercase();
            DocumentChunk {
                id: cc.id,
                file_name: cc.file_name.clone(),
                content: cc.code.clone(),
                doc_type: ext,
                chunk_index: chunk_index as i64,
                page: None,
                metadata,
                created_at: cc.created_at.clone(),
            }
        })
        .collect()
}

/// Chunked + oversized-split path that returns `DocumentChunk`s ready for
/// embedding. Used by `IngestionService::ingest_code_file`.
pub fn chunk_file_to_documents(
    file_path: &Path,
    repo_name: &str,
    file_name_override: Option<&str>,
    splitter: &text_splitter::TextSplitter<tokenizers::Tokenizer>,
    max_chunk_tokens: usize,
) -> Vec<DocumentChunk> {
    // Callers (zip ingest) wrap this in catch_unwind so a language-parser
    // panic becomes a per-file error rather than killing the whole job.
    let path_s = file_path.display().to_string();
    let t0 = std::time::Instant::now();
    crate::write_ingest_breadcrumb("chunk_parse", &path_s);
    let chunks =
        chunk_single_code_file(file_path, repo_name, file_name_override, splitter, max_chunk_tokens);
    let parse_ms = t0.elapsed().as_millis();
    crate::write_ingest_breadcrumb(
        "chunk_split_oversized",
        &format!("path={path_s} raw_chunks={} parse_ms={parse_ms}", chunks.len()),
    );
    let t1 = std::time::Instant::now();
    let split = oversized::split_oversized_code_chunks(chunks, splitter);
    let split_ms = t1.elapsed().as_millis();
    crate::write_ingest_breadcrumb(
        "chunk_to_docs",
        &format!(
            "path={path_s} split_chunks={} parse_ms={parse_ms} split_ms={split_ms}",
            split.len()
        ),
    );
    code_chunks_to_document_chunks(split)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_encoding_handles_bom() {
        assert_eq!(detect_encoding(&[0xef, 0xbb, 0xbf, b'x']), "utf-8");
        assert_eq!(detect_encoding(&[0xff, 0xfe, 0x00, 0x00]), "utf-16-le");
        assert_eq!(detect_encoding(&[0xfe, 0xff, 0x00, 0x00]), "utf-16-be");
        assert_eq!(detect_encoding(b"plain ascii"), "utf-8");
    }

    #[test]
    fn is_code_file_recognizes_supported_extensions() {
        for ext in &["py", "rs", "ts", "tsx", "c", "h", "cpp", "cs", "java", "sql", "js"] {
            assert!(is_code_file(Path::new(&format!("foo.{ext}"))), "{ext}");
        }
        assert!(!is_code_file(Path::new("foo.txt")));
        assert!(!is_code_file(Path::new("foo.md")));
    }

    #[test]
    fn language_for_extension_maps_all_variants() {
        assert_eq!(language_for_extension("c"), Some(Language::C));
        assert_eq!(language_for_extension("h"), Some(Language::C));
        assert_eq!(language_for_extension("ts"), Some(Language::TypeScript));
        assert_eq!(language_for_extension("tsx"), Some(Language::TypeScriptTsx));
        assert_eq!(language_for_extension("sql"), Some(Language::Sql));
        assert_eq!(language_for_extension("java"), Some(Language::Java));
        assert_eq!(language_for_extension("nope"), None);
    }

    #[test]
    fn code_documents_have_sequential_chunk_indexes() {
        let chunks = vec![
            CodeChunk::generic("one", "repo", "file.txt", "first"),
            CodeChunk::generic("two", "repo", "file.txt", "second"),
        ];
        let documents = code_chunks_to_document_chunks(chunks);
        assert_eq!(documents[0].chunk_index, 0);
        assert_eq!(documents[1].chunk_index, 1);
    }
}
