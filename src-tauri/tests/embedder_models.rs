//! Integration tests that load real model weights from
//! `src-tauri/resources/models/`. These tests silently skip if the model
//! files are absent (run `scripts/download-models.sh` to fetch them).
//!
//! Unlike the Qdrant E2E tests in `qdrant_e2e.rs`, these do not spawn any
//! external process — they only need the safetensors files on disk — so
//! they are NOT marked `#[ignore]`. Run with:
//!
//! ```sh
//! cargo test --tests
//! ```

mod common;

use common::{cosine, dense_ready, reranker_ready};
use mcp_nano_lib::services::embedders::{EncodeDocuments, EncodeQuery};

#[test]
fn encode_query_returns_expected_dimension_vector() {
    let Some(emb) = dense_ready() else {
        eprintln!("skipping: arctic-embed-xs model not downloaded; run scripts/download-models.sh");
        return;
    };
    let v = emb.encode_query("hello world").expect("encode_query");
    assert_eq!(v.len(), emb.dim(), "vector dimension must match model hidden_size");
    assert_eq!(v.len(), 384, "arctic-embed-xs is 384-dim");
    assert!(v.iter().all(|x| x.is_finite()), "no NaN/inf in embedding");
    assert!(
        v.iter().any(|x| *x != 0.0),
        "embedding should not be all zeros"
    );
}

#[test]
fn encode_documents_batch_preserves_count_and_dimension() {
    let Some(emb) = dense_ready() else {
        eprintln!("skipping: arctic-embed-xs model not downloaded; run scripts/download-models.sh");
        return;
    };
    let docs = [
        "Rust is a systems programming language.",
        "Python is great for machine learning.",
        "Vector databases enable semantic search.",
    ];
    let vecs = emb
        .encode_documents(&docs, 2)
        .expect("encode_documents");
    assert_eq!(vecs.len(), docs.len());
    for v in &vecs {
        assert_eq!(v.len(), emb.dim());
        assert!(v.iter().all(|x| x.is_finite()));
    }
}

#[test]
fn similar_texts_produce_higher_similarity_than_unrelated() {
    let Some(emb) = dense_ready() else {
        eprintln!("skipping: arctic-embed-xs model not downloaded; run scripts/download-models.sh");
        return;
    };
    let a = emb.encode_query("rust programming language").unwrap();
    let b = emb.encode_query("rust programming language").unwrap();
    let c = emb.encode_query("chocolate cake recipe").unwrap();

    let sim_same = cosine(&a, &b);
    let sim_diff = cosine(&a, &c);
    assert!(sim_same > 0.95, "same text should be near-identical: {sim_same}");
    assert!(
        sim_same > sim_diff,
        "same text should be more similar than unrelated text: same={sim_same}, diff={sim_diff}"
    );
}

#[test]
fn rerank_returns_one_score_per_document() {
    let Some(r) = reranker_ready() else {
        eprintln!("skipping: minilm-l6-v2 model not downloaded; run scripts/download-models.sh");
        return;
    };
    let docs = [
        "Rust is a systems programming language.",
        "Chocolate cake is delicious.",
        "Tokyo is the capital of Japan.",
    ];
    let scores = r.rerank("programming language", &docs, 4).expect("rerank");
    assert_eq!(scores.len(), docs.len());
    assert!(scores.iter().all(|x| x.is_finite()));
}

#[test]
fn relevant_doc_scores_higher_than_irrelevant() {
    let Some(r) = reranker_ready() else {
        eprintln!("skipping: minilm-l6-v2 model not downloaded; run scripts/download-models.sh");
        return;
    };
    let docs = [
        "Rust is a systems programming language.",
        "Chocolate cake is delicious.",
    ];
    let scores = r.rerank("programming language", &docs, 4).expect("rerank");
    assert!(
        scores[0] > scores[1],
        "relevant doc should score higher: prog={}, cake={}",
        scores[0],
        scores[1]
    );
}

#[test]
fn empty_documents_returns_empty_scores() {
    let Some(r) = reranker_ready() else {
        eprintln!("skipping: minilm-l6-v2 model not downloaded; run scripts/download-models.sh");
        return;
    };
    let scores = r.rerank("anything", &[], 4).expect("rerank empty");
    assert!(scores.is_empty());
}
