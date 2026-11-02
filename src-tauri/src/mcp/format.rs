//! Plain-text rendering of RAG results for MCP tool output.

use crate::models::response::RagResponse;
use crate::models::RagResult;
use crate::services::rag::{distinct_calls, metadata_strings};

/// Payload keys printed on their own lines (or as the body) rather than
/// inside the metadata JSON.
const SEPARATE_KEYS: [&str; 7] = [
    "content",
    "code",
    "path",
    "symbol",
    "qualified_symbol",
    "calls",
    "members",
];

/// Format a RAG response for MCP tool output: one block per hit with a short
/// header, the remaining metadata as compact JSON, and the chunk text verbatim
/// so the client does not have to unescape it from JSON.
///
/// `ranked` is false for symbol lookups without query text; their results
/// (and `related` definitions, which are never ranked) are labelled
/// "file order" instead of showing a meaningless zero score.
pub fn format_rag_response(
    result: &RagResponse,
    query: &str,
    limit: usize,
    ranked: bool,
) -> String {
    let total = result.total_count;
    let mut notes = String::new();
    for note in &result.notes {
        notes.push_str(&format!("Note: {note}\n"));
    }
    if total == 0 || result.results.is_empty() {
        let text = format!("No results found for query: '{query}'\n{notes}");
        return text.trim_end().to_string();
    }

    let display_query = if result.user_query.is_empty() {
        query
    } else {
        result.user_query.as_str()
    };

    let limited = &result.results[..result.results.len().min(limit)];
    let mut text = format!(
        "Found {total} results for '{display_query}'\n{notes}\n{}",
        format_results(limited, ranked)
    );
    if !result.related.is_empty() {
        text.push_str(&format!(
            "\n\n===\nRelated definitions (functions called by the results above):\n\n{}",
            format_results(&result.related, false)
        ));
    }
    text
}

fn format_results(items: &[RagResult], ranked: bool) -> String {
    let mut formatted = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        formatted.push(format_result(idx + 1, item, ranked));
    }
    formatted.join("\n---\n\n")
}

fn format_result(number: usize, item: &RagResult, ranked: bool) -> String {
    let source = source_label(&item.metadata);
    let label = if source.is_empty() {
        String::new()
    } else {
        format!(" [{source}]")
    };

    let mut text = item.document.clone();
    let mut metadata = serde_json::Map::new();
    if let Some(map) = item.metadata.as_object() {
        for (key, value) in map {
            if SEPARATE_KEYS.contains(&key.as_str()) {
                if text.is_empty() && (key == "content" || key == "code") {
                    if let Some(s) = value.as_str() {
                        text = s.to_string();
                    }
                }
                continue;
            }
            metadata.insert(key.clone(), value.clone());
        }
    }

    let order = if ranked {
        format!("score: {:.3}", item.score)
    } else {
        "file order".to_string()
    };
    let mut out = format!(
        "Result {number}{label} ({order}):\nMetadata: {}",
        serde_json::Value::Object(metadata)
    );
    if let Some(path) = item.metadata.get("path").and_then(|v| v.as_str()) {
        out.push_str(&format!("\nFile: {path}"));
    }
    let symbol = item
        .metadata
        .get("qualified_symbol")
        .or_else(|| item.metadata.get("symbol"))
        .and_then(|v| v.as_str());
    if let Some(symbol) = symbol {
        out.push_str(&format!("\nSymbol: {symbol}"));
    }
    let members = metadata_strings(item, "members");
    if !members.is_empty() {
        out.push_str(&format!("\nMembers: {}", members.join(", ")));
    }
    let calls = distinct_calls(&metadata_strings(item, "calls"));
    if !calls.is_empty() {
        out.push_str(&format!("\nCalls: {}", calls.join(", ")));
    }
    out.push_str(&format!("\nContent:\n{text}"));
    out
}

fn source_label(meta: &serde_json::Value) -> &str {
    for key in ["source", "repo_name", "file_name"] {
        if let Some(value) = meta.get(key).and_then(|v| v.as_str()) {
            if !value.is_empty() {
                return value;
            }
        }
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response_with(metadata: serde_json::Value, document: &str) -> RagResponse {
        RagResponse {
            total_count: 1,
            user_query: "rust".into(),
            results: vec![RagResult {
                id: "1".into(),
                document: document.into(),
                metadata,
                score: 0.9,
            }],
            related: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn prints_notes_under_the_header_and_with_empty_results() {
        let mut r = response_with(serde_json::json!({"code": "fn main() {}"}), "");
        r.notes.push("'main' is defined in 2 files".into());
        let text = format_rag_response(&r, "q", 5, false);
        assert!(text.starts_with(
            "Found 1 results for 'rust'\nNote: 'main' is defined in 2 files\n\nResult 1"
        ));

        let mut empty = RagResponse::default();
        empty.notes.push("hint".into());
        assert_eq!(
            format_rag_response(&empty, "x", 5, false),
            "No results found for query: 'x'\nNote: hint"
        );
    }

    #[test]
    fn empty_results_message() {
        let r = RagResponse::default();
        assert!(format_rag_response(&r, "foo", 5, true).contains("No results found"));
    }

    #[test]
    fn formats_header_and_score() {
        let r = response_with(serde_json::json!({"repo_name": "mcp-nano"}), "");
        let text = format_rag_response(&r, "rust", 5, true);
        assert!(text.contains("Found 1 results"));
        assert!(text.contains("[mcp-nano]"));
        assert!(text.contains("0.900"));
    }

    #[test]
    fn prints_document_content_verbatim_outside_metadata() {
        let r = response_with(
            serde_json::json!({"file_name": "a.md", "content": "line 1\nline 2"}),
            "",
        );
        let text = format_rag_response(&r, "q", 5, true);
        assert!(text.contains("Content:\nline 1\nline 2"));
        assert!(text.contains(r#"Metadata: {"file_name":"a.md"}"#));
    }

    #[test]
    fn prints_code_content() {
        let r = response_with(
            serde_json::json!({"repo_name": "r", "code": "fn main() {}"}),
            "",
        );
        let text = format_rag_response(&r, "q", 5, true);
        assert!(text.contains("Content:\nfn main() {}"));
        assert!(!text.contains(r#""code""#));
    }

    #[test]
    fn prints_symbol_and_calls_lines() {
        let r = response_with(
            serde_json::json!({
                "symbol": "run",
                "qualified_symbol": "Svc::run",
                "path": "src/svc.rs",
                "calls": ["fetch", "rank", "Store::get", "get"],
                "code": "fn run() {}"
            }),
            "",
        );
        let text = format_rag_response(&r, "q", 5, true);
        assert!(text.contains("Symbol: Svc::run"));
        assert!(text.contains("File: src/svc.rs"));
        assert!(text.contains("Calls: fetch, rank, Store::get\n"));
        assert!(!text.contains(r#""path""#));
    }

    #[test]
    fn prints_member_list() {
        let r = response_with(
            serde_json::json!({"symbol": "Svc", "members": ["new", "run"], "code": "struct Svc;"}),
            "",
        );
        let text = format_rag_response(&r, "q", 5, false);
        assert!(text.contains("Members: new, run"));
        assert!(!text.contains(r#""calls""#));
    }

    #[test]
    fn prints_related_definitions_section() {
        let mut r = response_with(serde_json::json!({"code": "a()"}), "");
        r.related.push(RagResult {
            id: "2".into(),
            metadata: serde_json::json!({"symbol": "a", "code": "fn a() {}"}),
            ..Default::default()
        });
        let text = format_rag_response(&r, "q", 5, true);
        assert!(text.contains("Related definitions"));
        assert!(text.contains("(score: 0.900)"));
        assert!(text.contains("(file order)"));
        assert!(text.contains("Content:\nfn a() {}"));
    }

    #[test]
    fn unranked_lookup_shows_file_order_instead_of_score() {
        let r = response_with(serde_json::json!({"code": "fn a() {}"}), "");
        let text = format_rag_response(&r, "q", 5, false);
        assert!(text.contains("(file order)"));
        assert!(!text.contains("score:"));
    }

    #[test]
    fn falls_back_to_document_field() {
        let r = response_with(serde_json::json!({"file_name": "a.md"}), "legacy text");
        let text = format_rag_response(&r, "q", 5, true);
        assert!(text.contains("Content:\nlegacy text"));
    }
}
