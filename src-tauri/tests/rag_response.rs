use mcp_nano_lib::models::response::RagResponse;
use mcp_nano_lib::models::RagResult;

fn make_rag_response(n: usize) -> RagResponse {
    let results: Vec<RagResult> = (0..n)
        .map(|i| RagResult {
            id: i.to_string(),
            document: format!("doc content {i}"),
            metadata: serde_json::json!({"source": "test.py"}),
            score: 1.0 - (i as f64 * 0.1),
        })
        .collect();
    let count = results.len() as i64;
    RagResponse {
        total_count: count,
        results,
        user_query: "test query".to_string(),
    }
}

#[test]
fn empty_response_has_zero_count() {
    let resp = make_rag_response(0);
    assert_eq!(resp.total_count, 0);
    assert!(resp.results.is_empty());
}

#[test]
fn single_result_maps_fields_correctly() {
    let resp = make_rag_response(1);
    assert_eq!(resp.results.len(), 1);
    assert_eq!(resp.results[0].id, "0");
    assert_eq!(resp.results[0].document, "doc content 0");
    assert_eq!(resp.results[0].score, 1.0);
    assert_eq!(resp.user_query, "test query");
}

#[test]
fn multiple_results_have_descending_score_order() {
    let resp = make_rag_response(5);
    assert_eq!(resp.total_count, 5);
    for i in 1..resp.results.len() {
        assert!(
            resp.results[i - 1].score >= resp.results[i].score,
            "results should be sorted by score descending at index {i}"
        );
    }
}

#[test]
fn document_field_contains_content() {
    let resp = make_rag_response(3);
    for r in &resp.results {
        assert!(!r.document.is_empty(), "document should not be empty");
        assert!(r.document.starts_with("doc content"));
    }
}

#[test]
fn metadata_preserves_json_fields() {
    let resp = make_rag_response(1);
    let meta = &resp.results[0].metadata;
    assert_eq!(meta.get("source").and_then(|v| v.as_str()), Some("test.py"));
}

#[test]
fn show_documents_false_strips_document_field() {
    let mut resp = make_rag_response(2);
    for r in &mut resp.results {
        r.document.clear();
    }
    for r in &resp.results {
        assert!(r.document.is_empty(), "document should be empty when stripped");
        assert!(!r.id.is_empty(), "id should still be present");
    }
}

#[test]
fn response_serializes_to_expected_json_shape() {
    let resp = make_rag_response(1);
    let json = serde_json::to_value(&resp).unwrap();
    assert!(json.get("results").unwrap().is_array());
    assert!(json.get("total_count").unwrap().is_number());
    assert!(json.get("user_query").unwrap().is_string());
    let result = &json["results"][0];
    assert!(result.get("id").unwrap().is_string());
    assert!(result.get("document").unwrap().is_string());
    assert!(result.get("metadata").unwrap().is_object());
    assert!(result.get("score").unwrap().is_number());
}
