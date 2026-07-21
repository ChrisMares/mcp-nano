//! End-to-end integration tests that spawn the bundled Qdrant binary and
//! (for the worker test) drive the full ingestion pipeline.
//!
//! Both tests are `#[ignore]`'d because they require:
//!   - `binaries/qdrant-x86_64-unknown-linux-gnu` (bundled Qdrant binary)
//!   - `resources/models/arctic-embed-xs/` (downloaded by `scripts/download-models.sh`)
//!   - `resources/models/minilm-l6-v2/` (for the worker pipeline test)
//!
//! Run with:
//!
//! ```sh
//! cargo test --tests -- --ignored
//! ```

mod common;

use std::time::Duration;

use common::{create_test_collection, load_embedders, models_dir, open_sqlite_pool, spawn_qdrant};
use mcp_nano_lib::services::embedders::{EncodeDocuments, EncodeQuery};
use mcp_nano_lib::services::ingestion_service::IngestionService;
use mcp_nano_lib::services::qdrant_service::{Include, QdrantService};
use mcp_nano_lib::worker;
use qdrant_client::qdrant::{Condition, Filter};
use sqlx::Row;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Spawn qdrant -> embed 3 texts -> upsert -> RRF query -> verify hits.
#[tokio::test]
#[ignore = "requires bundled qdrant binary + downloaded arctic-embed-xs model"]
async fn end_to_end_upsert_and_hybrid_query() {
    let Some((qdrant_client, _guard)) = spawn_qdrant().await else {
        eprintln!("skipping: could not spawn qdrant (binary missing)");
        return;
    };
    let embedders = match load_embedders() {
        Some(e) => e,
        None => return,
    };

    // Use a unique collection name per run to avoid collisions.
    let collection = format!("itest_{}", std::process::id());
    // Bypass ensure_collection's allow-list (test-only collection).
    create_test_collection(&qdrant_client, &collection, embedders.dense.dim())
        .await
        .expect("create_collection");

    let svc = QdrantService::new(qdrant_client.clone());
    let docs = vec![
        "Rust is a systems programming language focused on safety and speed.".to_string(),
        "Python is popular for machine learning and data science.".to_string(),
        "Qdrant is a vector database for semantic search.".to_string(),
    ];
    let doc_refs: Vec<&str> = docs.iter().map(|s| s.as_str()).collect();
    let embeddings = embedders
        .dense
        .encode_documents(&doc_refs, 4)
        .expect("encode docs");
    let ids: Vec<Uuid> = (0..docs.len()).map(|_| Uuid::new_v4()).collect();
    let metadatas: Vec<serde_json::Value> = docs
        .iter()
        .map(|d| serde_json::json!({"source": "test", "preview": d.chars().take(20).collect::<String>()}))
        .collect();

    svc.upsert_items(
        &collection,
        &ids,
        &docs,
        &embeddings,
        &metadatas,
        &embedders.bm25,
        100,
    )
    .await
    .expect("upsert");

    // Wait for the upsert to be reflected (Qdrant is eventually consistent
    // for counts; the wait=true flag on upsert handles write durability,
    // but a small delay helps the index settle for the very first query).
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Query: "programming language" should match the Rust doc best.
    let q_emb = embedders
        .dense
        .encode_query("programming language")
        .expect("encode query");
    let result = svc
        .query_items(
            &collection,
            &q_emb,
            Some("programming language"),
            3,
            None,
            Include::all(),
            Some(&embedders.bm25),
        )
        .await
        .expect("query");

    assert!(!result.is_empty(), "query should return hits");
    assert_eq!(result.ids[0].len(), 3, "should return 3 hits");
    assert!(
        !result.documents[0].is_empty(),
        "documents should be populated"
    );
    assert!(
        result.documents[0].iter().any(|d| d.contains("Rust")),
        "Rust doc should be among hits: {:?}",
        result.documents[0]
    );

    // Cleanup: delete the test collection.
    let _ = qdrant_client.delete_collection(&collection).await;
}

/// End-to-end: insert PENDING job -> worker claims -> ingestion runs ->
/// Qdrant has the upserted point. Requires the bundled Qdrant binary +
/// downloaded arctic-embed-xs model.
#[tokio::test]
#[ignore = "requires bundled qdrant binary + downloaded arctic-embed-xs model"]
async fn end_to_end_job_pipeline_upserts_to_qdrant() {
    let embedders = match load_embedders() {
        Some(e) => e,
        None => return,
    };
    let Some((qdrant_client, _guard)) = spawn_qdrant().await else {
        eprintln!("skipping: could not spawn qdrant (binary missing)");
        return;
    };

    // Use the "general" collection (auto-create allow-list) with a unique
    // group tag so we can clean up by filter without affecting other tests.
    let collection = "general";
    let test_group = format!("itest_worker_{}", std::process::id());
    create_test_collection(&qdrant_client, collection, 384)
        .await
        .ok(); // already-exists is fine

    // Build the ingestion service + task registry.
    let qdrant_service = QdrantService::new(qdrant_client.clone());
    let ingestion = std::sync::Arc::new(
        IngestionService::new(embedders.clone(), qdrant_service, &models_dir())
            .expect("build ingestion service"),
    );
    let registry = ingestion.build_task_registry();

    // Open SQLite + insert a PENDING job.
    let db_dir = tempfile::tempdir().expect("db tempdir");
    let pool = open_sqlite_pool(db_dir.path()).await;

    // Create a temp text file to ingest.
    let doc_dir = tempfile::tempdir().expect("doc tempdir");
    let doc_path = doc_dir.path().join("notes.txt");
    std::fs::write(
        &doc_path,
        "Rust is a systems programming language focused on safety and performance.",
    )
    .unwrap();

    let job_id = format!("job-{}", uuid::Uuid::new_v4());
    let now = mcp_nano_lib::worker::progress::now_iso();
    let params = serde_json::json!({
        "path": doc_path.to_string_lossy(),
        "collection": collection,
        "group": test_group,
    });
    sqlx::query(
        "INSERT INTO job_status (job_id, status, created_at, updated_at, progress_percentage, task_name, task_params) \
         VALUES (?, 'PENDING', ?, ?, 0, 'process_documents_upload', ?)",
    )
    .bind(&job_id)
    .bind(&now)
    .bind(&now)
    .bind(params.to_string())
    .execute(&pool)
    .await
    .unwrap();

    // Start the worker.
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    let handle = worker::start(pool.clone(), registry, cancel, None);

    // Wait for FINISHED (up to 15s — ingestion involves a model forward pass).
    let mut final_status = String::new();
    for _ in 0..150 {
        let row = sqlx::query("SELECT status FROM job_status WHERE job_id = ?")
            .bind(&job_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        let status: String = row.get(0);
        if status == "FINISHED" || status == "FAILED" {
            final_status = status;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    cancel_clone.cancel();
    let _ = handle.await;

    if final_status != "FINISHED" {
        let row =
            sqlx::query("SELECT error_message, result FROM job_status WHERE job_id = ?")
                .bind(&job_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let err: Option<String> = row.get(0);
        let res: Option<String> = row.get(1);
        panic!(
            "job should reach FINISHED, got {final_status}\nerror: {err:?}\nresult: {res:?}"
        );
    }
    assert_eq!(final_status, "FINISHED");

    // Verify the Qdrant collection now has at least one point.
    tokio::time::sleep(Duration::from_millis(300)).await; // let the index settle
    let svc = QdrantService::new(qdrant_client.clone());
    let count = svc.count_items(collection).await.expect("count");
    assert!(
        count > 0,
        "Qdrant collection should have at least 1 point, got {count}"
    );

    // Query to verify the content is searchable, filtered to our test group.
    let q_emb = embedders.dense.encode_query("systems programming language").expect("encode query");
    let filter = Filter::must([Condition::matches("group", test_group.clone())]);
    let result = svc
        .query_items(
            collection,
            &q_emb,
            Some("systems programming language"),
            5,
            Some(filter),
            Include::all(),
            Some(&embedders.bm25),
        )
        .await
        .expect("query");
    assert!(!result.is_empty(), "query should return hits");
    assert!(
        result.documents[0].iter().any(|d| d.contains("Rust")),
        "Rust doc should be in hits: {:?}",
        result.documents[0]
    );

    // Cleanup: delete all points tagged with our test group.
    let cleanup_filter = Filter::must([Condition::matches("group", test_group)]);
    let _ = svc.delete_items(collection, None, Some(cleanup_filter)).await;
}
