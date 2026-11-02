//! RAG retrieval shared by the Tauri `rag_query` command and MCP tool calls:
//! hybrid dense + BM25 search with cross-encoder reranking, plus code symbol
//! lookups (see [`symbols`]).

mod symbols;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use qdrant_client::qdrant::Filter;
use tracing::{debug, info};

pub use symbols::{distinct_calls, metadata_strings};

use crate::models::request::RagQueryRequest;
use crate::models::response::RagResponse;
use crate::models::RagResult;
use crate::services::embedder_state::EmbedderState;
use crate::services::embedders::EncodeQuery;
use crate::services::file_filters::DATA_EXTENSIONS;
use crate::services::ingestion::code_chunker::language_for_extension;
use crate::services::qdrant_service::{Include, QdrantService};

/// Keep rerank batches modest on GPU; pairs are truncated to 512 tokens but
/// attention still scales with batch × seq².
const RERANK_BATCH_SIZE: usize = 16;
const DEFAULT_LIMIT: i64 = 10;
/// Collection that carries `symbol` / `calls` payload fields.
const CODE_COLLECTION: &str = "codebase";
/// Score subtracted from low-value files in codebase searches. Reranker
/// scores are logits; a changelog paragraph typically outscores the best
/// code hit by 1-3 for feature-name queries.
const LOW_VALUE_PENALTY: f64 = 4.0;
/// Data files longer than this are dumps, not hand-written configs.
const MAX_CONFIG_LINES: u64 = 300;
/// Data files above [`MIN_DUMP_BYTES`] whose average line is longer than
/// this are minified or embed scraped pages: dumps, whatever their line
/// count.
const MAX_CONFIG_LINE_LENGTH: u64 = 500;
/// Smallest data file judged by line length.
const MIN_DUMP_BYTES: u64 = 4 * 1024;
/// File names (without extension) of release notes and legal boilerplate.
const LOW_VALUE_NAMES: [&str; 10] = [
    "changelog",
    "changes",
    "history",
    "news",
    "license",
    "licence",
    "copying",
    "authors",
    "contributors",
    "notice",
];
/// Documentation formats never down-ranked, whatever the file is called.
const DOC_EXTENSIONS: [&str; 6] = ["md", "mdx", "markdown", "rst", "dox", "adoc"];
/// Share of non-empty lines that must be imports/exports for a leftover
/// chunk to be ranked behind real code.
const IMPORT_ONLY_RATIO: f64 = 0.7;

/// Shared RAG pipeline used by the Tauri `rag_query` command and MCP tool calls.
#[derive(Clone)]
pub struct RagService {
    embedders: Arc<EmbedderState>,
    qdrant: QdrantService,
}

impl RagService {
    pub fn new(embedders: Arc<EmbedderState>, qdrant: QdrantService) -> Self {
        Self { embedders, qdrant }
    }

    /// Run one or more RAG requests and merge their results.
    ///
    /// A request with a `symbol` performs a symbol lookup (definition,
    /// references, or callees) in the `codebase` collection instead of a
    /// free-text search; other collections are skipped for such requests.
    /// A text request with `expand: true` also returns, in `related`, the
    /// definitions of functions its hits call.
    pub async fn run_rag_query(&self, requests: &[RagQueryRequest]) -> Result<RagResponse> {
        if requests.is_empty() {
            return Err(anyhow!("at least one query request is required"));
        }

        let device = self.embedders.device_mode();
        let total_start = Instant::now();
        info!(
            "RAG query start: {} request(s) inference_device={device}",
            requests.len()
        );
        let mut all_results: Vec<RagResult> = Vec::new();
        let mut related: Vec<RagResult> = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        let mut combined_query = String::new();
        let mut last_show_documents = true;

        for req in requests {
            let limit = normalize_limit(req.limit) as usize;
            last_show_documents = req.show_documents.unwrap_or(true);
            let query = req.query.trim();
            let symbol = req.symbol.as_deref().map(str::trim).unwrap_or("");
            debug!(
                "  collection={} query_chars={} symbol={symbol:?} relation={:?} limit={limit}",
                req.collection,
                query.len(),
                req.relation
            );

            if !symbol.is_empty() && req.collection != CODE_COLLECTION {
                continue;
            }
            let scope = match &req.where_clause {
                Some(w) if !w.is_null() && w.as_object().is_none_or(|o| !o.is_empty()) => {
                    Some(QdrantService::build_filter(w).context("building filter")?)
                }
                _ => None,
            };

            if !symbol.is_empty() {
                let relation = req.relation.unwrap_or_default();
                let lookup = self
                    .symbol_lookup(symbol, relation, query, scope, limit)
                    .await?;
                all_results.extend(lookup.results);
                notes.extend(lookup.note);
                append_query(&mut combined_query, symbol);
                continue;
            }

            if query.is_empty() {
                return Err(anyhow!("a query or a symbol is required"));
            }
            let results = self
                .semantic_search(&req.collection, query, scope.clone(), limit)
                .await?;
            if req.expand.unwrap_or(false) && req.collection == CODE_COLLECTION {
                let expanded = self.expand_callees(&results, scope).await?;
                related.extend(expanded);
            }
            all_results.extend(results);
            append_query(&mut combined_query, query);
        }

        let mut seen_ids: HashSet<String> = HashSet::new();
        for r in &all_results {
            seen_ids.insert(r.id.clone());
        }
        let mut unique_related: Vec<RagResult> = Vec::new();
        for r in related {
            if seen_ids.insert(r.id.clone()) {
                unique_related.push(r);
            }
        }

        if !last_show_documents {
            for r in all_results.iter_mut().chain(unique_related.iter_mut()) {
                r.document.clear();
            }
        }

        info!(
            "RAG query complete device={device} results={} related={} total_elapsed_ms={}",
            all_results.len(),
            unique_related.len(),
            total_start.elapsed().as_millis()
        );

        Ok(RagResponse {
            total_count: all_results.len() as i64,
            results: all_results,
            related: unique_related,
            notes,
            user_query: combined_query,
        })
    }

    /// Hybrid dense + BM25 retrieval followed by cross-encoder reranking.
    async fn semantic_search(
        &self,
        collection: &str,
        query: &str,
        filter: Option<Filter>,
        limit: usize,
    ) -> Result<Vec<RagResult>> {
        let device = self.embedders.device_mode();
        let encode_start = Instant::now();
        let dense = self
            .embedders
            .dense
            .encode_query(query)
            .context("encoding query")?;
        info!(
            "RAG dense encode_query done device={device} dim={} elapsed_ms={}",
            dense.len(),
            encode_start.elapsed().as_millis()
        );

        // Over-fetch so the reranker has candidates to reorder.
        let multiple = match limit {
            0..=10 => 5,
            11..=25 => 4,
            26..=50 => 3,
            _ => 2,
        };
        let retrieve_n = limit * multiple;
        let retrieve_start = Instant::now();
        let vec_result = self
            .qdrant
            .query_items(
                collection,
                &dense,
                Some(query),
                retrieve_n,
                filter,
                Include::all(),
                Some(&self.embedders.bm25),
            )
            .await
            .with_context(|| format!("querying collection '{collection}'"))?;
        let results = vec_db_to_rag_results(&vec_result);
        info!(
            "RAG qdrant retrieve done collection={collection} candidates={} elapsed_ms={}",
            results.len(),
            retrieve_start.elapsed().as_millis()
        );

        self.rerank_results(query, results, limit, collection == CODE_COLLECTION)
    }

    /// Cross-encoder rerank of `results` against `query`, keeping `top`.
    /// In the codebase collection (`penalize_low_value`), release notes,
    /// license files, and large data files lose [`LOW_VALUE_PENALTY`] so code
    /// and docs win unless those files are clearly the better match.
    fn rerank_results(
        &self,
        query: &str,
        mut results: Vec<RagResult>,
        top: usize,
        penalize_low_value: bool,
    ) -> Result<Vec<RagResult>> {
        if results.is_empty() || top == 0 {
            return Ok(results);
        }

        let device = self.embedders.device_mode();
        let docs: Vec<&str> = results.iter().map(|r| r.document.as_str()).collect();
        let total_chars: usize = docs.iter().map(|d| d.len()).sum();
        let max_chars = docs.iter().map(|d| d.len()).max().unwrap_or(0);
        let batches = docs.len().div_ceil(RERANK_BATCH_SIZE.max(1));
        let start = Instant::now();
        info!(
            "RAG rerank begin device={device} docs={} batch_size={RERANK_BATCH_SIZE} batches={batches} total_chars={total_chars} max_doc_chars={max_chars} top={top}",
            docs.len()
        );

        let scores = self
            .embedders
            .reranker
            .rerank(query, &docs, RERANK_BATCH_SIZE)
            .context("reranking")?;

        info!(
            "RAG rerank done device={device} scores={} elapsed_ms={}",
            scores.len(),
            start.elapsed().as_millis()
        );

        for (result, score) in results.iter_mut().zip(scores.iter()) {
            result.score = f64::from(*score);
            if penalize_low_value && is_low_value_file(&result.metadata) {
                result.score -= LOW_VALUE_PENALTY;
            }
        }
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut results = demote_import_only(results);
        results.truncate(top);
        Ok(results)
    }
}

fn append_query(combined: &mut String, text: &str) {
    if !combined.is_empty() {
        combined.push(' ');
    }
    combined.push_str(text);
}

/// Move leftover chunks that are mostly import/export lines behind every
/// other result, keeping relative order. Import lists are full of matching
/// words, so they otherwise outrank the code an agent is looking for; they
/// are still returned when nothing better matches.
fn demote_import_only(results: Vec<RagResult>) -> Vec<RagResult> {
    let mut kept = Vec::with_capacity(results.len());
    let mut demoted = Vec::new();
    for r in results {
        let is_remainder =
            r.metadata.get("type").and_then(|v| v.as_str()) == Some("file_remainder");
        if is_remainder && is_import_only(&r.document) {
            demoted.push(r);
        } else {
            kept.push(r);
        }
    }
    kept.extend(demoted);
    kept
}

/// True when at least [`IMPORT_ONLY_RATIO`] of the non-empty, non-comment
/// lines of `text` are import or export statements (or their continuation
/// lines inside a multi-line import).
fn is_import_only(text: &str) -> bool {
    let mut lines = 0usize;
    let mut imports = 0usize;
    let mut in_block = false;
    for raw in text.lines() {
        let line = raw.trim();
        let comment = line.starts_with("//")
            || line.starts_with("/*")
            || line.starts_with('*')
            || (line.starts_with('#') && !line.starts_with("#include"));
        if line.is_empty() || comment {
            continue;
        }
        lines += 1;
        if in_block {
            imports += 1;
            if line.contains('}') || line.contains(')') {
                in_block = false;
            }
            continue;
        }
        if is_import_line(line) {
            imports += 1;
            let opens = line.ends_with('{') || line.ends_with('(') || line.ends_with(',');
            if opens {
                in_block = true;
            }
        }
    }
    lines > 0 && imports as f64 / lines as f64 >= IMPORT_ONLY_RATIO
}

fn is_import_line(line: &str) -> bool {
    const PREFIXES: [&str; 10] = [
        "import ",
        "import{",
        "from ",
        "export {",
        "export *",
        "export type {",
        "#include",
        "using ",
        "use ",
        "extern crate ",
    ];
    for prefix in PREFIXES {
        if line.starts_with(prefix) {
            return true;
        }
    }
    (line.starts_with("const ") && line.contains("require("))
        || line == "export default"
        || (line.starts_with("package ") && line.ends_with(';'))
}

/// True for release notes (`Changelog`, `NEWS`), license and authors files,
/// and data dumps: data files over [`MAX_CONFIG_LINES`] lines, or with very
/// long lines. Markdown and other doc
/// formats, READMEs, small configs, and code are never low-value.
fn is_low_value_file(metadata: &serde_json::Value) -> bool {
    let field = |key: &str| metadata.get(key).and_then(|v| v.as_str()).unwrap_or("");
    let path = if field("path").is_empty() {
        field("file_name")
    } else {
        field("path")
    };
    let file = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    let (stem, ext) = match file.split_once('.') {
        Some((stem, ext)) => (stem, ext.rsplit('.').next().unwrap_or(ext)),
        None => (file.as_str(), ""),
    };
    if DOC_EXTENSIONS.contains(&ext) || language_for_extension(ext).is_some() {
        return false;
    }
    for name in LOW_VALUE_NAMES {
        let rest = stem.strip_prefix(name);
        if rest.is_some_and(|r| r.is_empty() || r.starts_with(['-', '_', ' '])) {
            return true;
        }
    }
    if !DATA_EXTENSIONS.contains(&ext) {
        return false;
    }
    let number = |key: &str| metadata.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
    let lines = number("file_lines");
    let bytes = number("file_bytes");
    let long_lines = bytes > MIN_DUMP_BYTES && bytes / lines.max(1) > MAX_CONFIG_LINE_LENGTH;
    lines > MAX_CONFIG_LINES || long_lines
}

fn normalize_limit(limit: Option<i64>) -> i64 {
    let n = limit.unwrap_or(DEFAULT_LIMIT);
    n.clamp(1, 100)
}

fn vec_db_to_rag_results(vec: &crate::models::VecDbResult) -> Vec<RagResult> {
    let ids = vec.ids.first().map(Vec::as_slice).unwrap_or_default();
    let docs = vec.documents.first().map(Vec::as_slice).unwrap_or_default();
    let metas = vec.metadatas.first().map(Vec::as_slice).unwrap_or_default();
    let dists = vec.distances.first().map(Vec::as_slice).unwrap_or_default();

    let mut out = Vec::with_capacity(ids.len());
    for (i, id) in ids.iter().enumerate() {
        out.push(RagResult {
            id: id.clone(),
            document: docs.get(i).cloned().unwrap_or_default(),
            metadata: metas
                .get(i)
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
            score: dists.get(i).copied().unwrap_or(0.0),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_limit_clamps() {
        assert_eq!(normalize_limit(None), 10);
        assert_eq!(normalize_limit(Some(0)), 1);
        assert_eq!(normalize_limit(Some(5)), 5);
        assert_eq!(normalize_limit(Some(200)), 100);
    }

    fn remainder(text: &str) -> RagResult {
        RagResult {
            id: text.to_string(),
            document: text.to_string(),
            metadata: serde_json::json!({ "type": "file_remainder" }),
            ..Default::default()
        }
    }

    #[test]
    fn detects_import_only_text_across_languages() {
        assert!(is_import_only(
            "import React, { useState } from \"react\";\nimport {\n  Dropzone,\n  DropzoneContent,\n} from \"@/components/ui/dropzone\";\n\nexport default"
        ));
        assert!(is_import_only("from fastapi import APIRouter\nimport os\n"));
        assert!(is_import_only("using System;\nusing System.Linq;\n"));
        assert!(is_import_only("#include <stdio.h>\n#include \"fl.h\"\n"));
    }

    #[test]
    fn real_code_with_some_imports_is_not_import_only() {
        let text = "import os\n\nROUTES = {}\n\ndef register(name):\n    ROUTES[name] = True\n    return name\n";
        assert!(!is_import_only(text));
        assert!(!is_import_only(""));
    }

    #[test]
    fn import_only_remainders_move_behind_other_results() {
        let code = RagResult {
            id: "code".into(),
            document: "fn a() {}".into(),
            ..Default::default()
        };
        let results = vec![
            remainder("import a from \"a\";"),
            code,
            remainder("const x = 1;"),
        ];
        let ids: Vec<String> = demote_import_only(results)
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, vec!["code", "const x = 1;", "import a from \"a\";"]);
    }

    #[test]
    fn release_notes_license_and_large_data_files_are_low_value() {
        let low = |meta: serde_json::Value| is_low_value_file(&meta);
        assert!(low(serde_json::json!({"path": "Changelog"})));
        assert!(low(serde_json::json!({"path": "doc/NEWS"})));
        assert!(low(serde_json::json!({"path": "LICENSE-MIT.txt"})));
        assert!(low(
            serde_json::json!({"path": "raw/golfleague_raw.json", "file_lines": 900})
        ));
        assert!(low(
            serde_json::json!({"file_name": "dump.csv", "file_lines": 301})
        ));
        assert!(low(
            serde_json::json!({"path": "raw/page.json", "file_lines": 3, "file_bytes": 21937})
        ));
    }

    #[test]
    fn docs_readmes_small_configs_and_code_are_not_low_value() {
        let low = |meta: serde_json::Value| is_low_value_file(&meta);
        assert!(!low(serde_json::json!({"path": "CHANGELOG.md"})));
        assert!(!low(serde_json::json!({"path": "README"})));
        assert!(!low(serde_json::json!({"path": "docs/guide.md"})));
        assert!(!low(
            serde_json::json!({"path": "package.json", "file_lines": 60, "file_bytes": 2400})
        ));
        assert!(!low(
            serde_json::json!({"path": "tiny.json", "file_lines": 1, "file_bytes": 900})
        ));
        assert!(!low(serde_json::json!({"path": "appsettings.json"})));
        assert!(!low(serde_json::json!({"path": "src/newsfeed.cpp"})));
        assert!(!low(serde_json::json!({"path": "src/license.rs"})));
        assert!(!low(serde_json::json!({"path": "NewsFeed"})));
    }

    #[test]
    fn empty_vec_db_maps_to_empty() {
        assert!(vec_db_to_rag_results(&crate::models::VecDbResult::default()).is_empty());
    }
}
