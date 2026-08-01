//! Data management integration tests.
//!
//! Tests the SQLite file_metadata query paths used by the data controller.
//! Qdrant-backed tests (get_files facet, get_websites facet) are marked
//! `#[ignore]` and require the bundled Qdrant binary.


use sqlx::SqlitePool;
use uuid::Uuid;

use mcp_nano_lib::models::entities::FileMetadata;
use mcp_nano_lib::models::response::{FileMetadataDto, UserFilesResponse};
use mcp_nano_lib::models::RepoItem;

async fn seed_file_metadata(pool: &SqlitePool) {
    let now = "2026-07-21T12:00:00Z";
    // A repo file in codebase collection
    sqlx::query(
        "INSERT INTO file_metadata (storage_object_id, full_path, file_type, size_bytes, repo_name, group_id, status, created_at, collection) \
         VALUES (?, ?, ?, ?, ?, ?, 'completed', ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind("/uploads/mcp-nano/src/main.rs")
    .bind("rs")
    .bind(1024i64)
    .bind("mcp-nano")
    .bind("rust-repos")
    .bind(now)
    .bind("codebase")
    .execute(pool)
    .await
    .expect("seed codebase file");

    // A doc in general collection
    sqlx::query(
        "INSERT INTO file_metadata (storage_object_id, full_path, file_type, size_bytes, repo_name, group_id, status, created_at, collection) \
         VALUES (?, ?, ?, ?, ?, ?, 'completed', ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind("/uploads/guide.pdf")
    .bind("pdf")
    .bind(2048i64)
    .bind("")
    .bind("guides")
    .bind(now)
    .bind("general")
    .execute(pool)
    .await
    .expect("seed general file");
}

async fn test_pool() -> SqlitePool {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("app.db");
    std::mem::forget(dir);
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .expect("connect");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrate");
    pool
}

#[tokio::test]
async fn query_codebase_files_returns_only_codebase_records() {
    let pool = test_pool().await;
    seed_file_metadata(&pool).await;

    let rows = sqlx::query_as::<_, FileMetadata>(
        "SELECT * FROM file_metadata WHERE status = 'completed' AND collection = 'codebase'",
    )
    .fetch_all(&pool)
    .await
    .expect("query codebase");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].repo_name.as_deref(), Some("mcp-nano"));
    assert_eq!(rows[0].collection.as_deref(), Some("codebase"));
}

#[tokio::test]
async fn query_general_files_returns_only_general_records() {
    let pool = test_pool().await;
    seed_file_metadata(&pool).await;

    let rows = sqlx::query_as::<_, FileMetadata>(
        "SELECT * FROM file_metadata WHERE status = 'completed' AND collection = 'general'",
    )
    .fetch_all(&pool)
    .await
    .expect("query general");

    assert_eq!(rows.len(), 1);
    let file = &rows[0];
    assert_eq!(file.filename(), Some("guide.pdf"));
    assert_eq!(file.group_id.as_deref(), Some("guides"));
    assert_eq!(file.collection.as_deref(), Some("general"));
}

#[tokio::test]
async fn delete_file_metadata_by_storage_object_id() {
    let pool = test_pool().await;
    seed_file_metadata(&pool).await;

    let rows = sqlx::query_as::<_, FileMetadata>(
        "SELECT * FROM file_metadata WHERE status = 'completed'",
    )
    .fetch_all(&pool)
    .await
    .expect("list before delete");

    let id = rows[0].storage_object_id.clone();
    sqlx::query("DELETE FROM file_metadata WHERE storage_object_id = ?")
        .bind(&id)
        .execute(&pool)
        .await
        .expect("delete");

    let remaining = sqlx::query_as::<_, FileMetadata>(
        "SELECT * FROM file_metadata WHERE status = 'completed'",
    )
    .fetch_all(&pool)
    .await
    .expect("list after delete");

    assert_eq!(
        remaining.len(),
        rows.len() - 1,
        "should have one fewer row"
    );
}

#[test]
fn file_metadata_dto_from_entity_maps_correctly() {
    let row = FileMetadata {
        id: 1,
        storage_object_id: "abc-123".to_string(),
        full_path: "/uploads/reports/annual.pdf".to_string(),
        file_type: Some("pdf".to_string()),
        size_bytes: Some(50000),
        repo_name: None,
        group_id: Some("finance".to_string()),
        status: "completed".to_string(),
        error_message: None,
        created_at: Some("2026-07-21T00:00:00Z".to_string()),
        collection: Some("general".to_string()),
    };
    let dto = FileMetadataDto::from_entity(&row);
    assert_eq!(dto.filename, "annual.pdf");
    assert_eq!(dto.file_type.as_deref(), Some("pdf"));
    assert_eq!(dto.size_bytes, Some(50000));
    assert!(dto.created_at.is_some());
    assert_eq!(dto.group, "finance");
}

#[test]
fn repo_item_default_has_empty_fields() {
    let item = RepoItem::default();
    assert_eq!(item.repo_name, "");
    assert!(item.created_at.is_none());
    assert!(item.storage_object_id.is_none());
}

#[test]
fn user_files_response_default_is_empty() {
    let resp = UserFilesResponse::default();
    assert!(resp.repos.is_empty());
    assert!(resp.documents.is_empty());
}
