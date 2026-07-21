use crate::models::response::RagResponse;

/// Format a RAG response for MCP tool output. Mirrors Python `dynamic_call_tool`.
pub fn format_rag_response(result: &RagResponse, query: &str, limit: usize) -> String {
    let total = result.total_count;
    if total == 0 || result.results.is_empty() {
        return format!("No results found for query: '{query}'");
    }

    let display_query = if result.user_query.is_empty() {
        query
    } else {
        result.user_query.as_str()
    };

    let mut formatted = Vec::new();
    for (idx, item) in result.results.iter().take(limit).enumerate() {
        let meta = &item.metadata;
        let source = meta
            .get("source")
            .and_then(|v| v.as_str())
            .or_else(|| meta.get("repo_name").and_then(|v| v.as_str()))
            .unwrap_or("");
        let label = if source.is_empty() {
            String::new()
        } else {
            format!(" [{source}]")
        };
        formatted.push(format!(
            "Result {}{label} (score: {:.3}):\nMetadata: {meta}",
            idx + 1,
            item.score
        ));
    }

    format!(
        "Found {total} results for '{display_query}'\n\n{}",
        formatted.join("\n---\n\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RagResult;

    #[test]
    fn empty_results_message() {
        let r = RagResponse::default();
        assert!(format_rag_response(&r, "foo", 5).contains("No results found"));
    }

    #[test]
    fn formats_hits() {
        let r = RagResponse {
            total_count: 1,
            user_query: "rust".into(),
            results: vec![RagResult {
                id: "1".into(),
                document: "".into(),
                metadata: serde_json::json!({"repo_name": "mcp-nano"}),
                score: 0.9,
            }],
        };
        let text = format_rag_response(&r, "rust", 5);
        assert!(text.contains("Found 1 results"));
        assert!(text.contains("[mcp-nano]"));
        assert!(text.contains("0.900"));
    }
}
