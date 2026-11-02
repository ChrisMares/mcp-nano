//! Cleanup for a cancelled or failed job.
//!
//! Each ingestion task deletes the previous version of its data *before* it
//! writes the new one, so a run that does not finish leaves a partial index
//! behind. Rather than keep that, cancellation and failure remove everything
//! the run could have written, keyed by the same metadata the task uses:
//!
//! * `process_zip` — vectors by `repo_name` (codebase) or `zip_filename`
//!   (general), the uploaded archive and its extraction directory.
//! * `process_documents_upload` — vectors by `file_name` (+ `group`, + `user_id`)
//!   and the uploaded temp file.
//! * `process_code_file_upload` — vectors by `file_name` and the temp file.
//! * `process_website_scrape` — vectors by `doc_type=website` + each `url`.
//!
//! A cancelled job also drops its `file_metadata` row (keyed by
//! `storage_object_id`); a failed job keeps it so the UI can show the error.
//!
//! [`cleanup_cancelled_job`] is called from `cancel_job` for a job that was
//! still `PENDING` and from the worker for a running one.
//! [`cleanup_failed_job`] is called by the worker when a task returns an
//! error, panics, or is reclaimed after the stale-progress timeout.

use anyhow::{Context, Result};
use qdrant_client::qdrant::{Condition, Filter};
use sqlx::SqlitePool;
use tracing::{info, warn};

use super::ingestion_service::{
    metadata_user_id, repo_name_from_zip_filename, strip_zip_ext, upload_conditions, UploadScope,
};
use super::qdrant_service::QdrantService;

/// Delete everything a cancelled job produced, including its
/// `file_metadata` row. Best-effort: a failure to remove one artifact is
/// logged by the caller and does not stop the rest.
pub async fn cleanup_cancelled_job(
    qdrant: Option<&QdrantService>,
    pool: &SqlitePool,
    task_name: &str,
    params: &serde_json::Value,
) -> Result<()> {
    cleanup_failed_job(qdrant, task_name, params).await?;

    if let Some(storage_object_id) = params.get("storage_object_id").and_then(|v| v.as_str()) {
        sqlx::query("DELETE FROM file_metadata WHERE storage_object_id = ?")
            .bind(storage_object_id)
            .execute(pool)
            .await
            .context("deleting file_metadata for cancelled job")?;
    }

    Ok(())
}

/// Delete the vectors and uploaded files of a failed job so a partial index
/// is not left searchable. The `file_metadata` row is kept (the worker marks
/// it `failed` with the error message).
pub async fn cleanup_failed_job(
    qdrant: Option<&QdrantService>,
    task_name: &str,
    params: &serde_json::Value,
) -> Result<()> {
    match task_name {
        "process_zip" => cleanup_zip(qdrant, params).await?,
        "process_documents_upload" => cleanup_single_file(qdrant, params, "general", true).await?,
        "process_code_file_upload" => {
            cleanup_single_file(qdrant, params, "codebase", false).await?
        }
        "process_website_scrape" => cleanup_website(qdrant, params).await?,
        other => warn!("cleanup: no cleanup rule for task {other}"),
    }
    Ok(())
}

async fn delete_by(qdrant: Option<&QdrantService>, collection: &str, filter: Filter) -> Result<()> {
    let Some(qdrant) = qdrant else {
        info!("cleanup: Qdrant unavailable, skipping vector delete in {collection}");
        return Ok(());
    };
    qdrant
        .delete_items(collection, None, Some(filter))
        .await
        .with_context(|| format!("deleting job vectors from {collection}"))
}

async fn cleanup_zip(qdrant: Option<&QdrantService>, params: &serde_json::Value) -> Result<()> {
    let collection = params
        .pointer("/embedding_options/collection")
        .and_then(|v| v.as_str())
        .unwrap_or("general");
    let repo_name = params
        .pointer("/embedding_options/repo_name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let zip_filename = params
        .get("zip_filename")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    // Mirrors process_zip_upload: codebase is keyed by repo_name (defaulted
    // from zip_filename when omitted), general by the original zip filename.
    let condition = if collection == "codebase" {
        repo_name
            .map(str::to_string)
            .or_else(|| zip_filename.map(repo_name_from_zip_filename))
            .filter(|name| !name.is_empty())
            .map(|name| Condition::matches("repo_name", name))
            .or_else(|| {
                zip_filename.map(|name| Condition::matches("zip_filename", name.to_string()))
            })
    } else {
        zip_filename
            .map(|name| Condition::matches("zip_filename", name.to_string()))
            .or_else(|| repo_name.map(|name| Condition::matches("repo_name", name.to_string())))
    };
    if let Some(condition) = condition {
        delete_by(qdrant, collection, Filter::must([condition])).await?;
    }

    if let Some(zip_path) = params.get("zip_path").and_then(|v| v.as_str()) {
        warn_unless_missing(zip_path, std::fs::remove_file(zip_path));
        let extract_dir = format!("{}_extracted", strip_zip_ext(zip_path));
        warn_unless_missing(&extract_dir, std::fs::remove_dir_all(&extract_dir));
    }
    Ok(())
}

fn warn_unless_missing(path: &str, result: std::io::Result<()>) {
    if let Err(e) = result {
        if e.kind() != std::io::ErrorKind::NotFound {
            warn!("cleanup: removing {path}: {e}");
        }
    }
}

/// Conditions shared by the two single-file upload tasks. Documents are
/// scoped by `group`, code files by `repo_name`.
fn file_conditions(params: &serde_json::Value, is_document: bool) -> Vec<Condition> {
    let str_param = |key: &str| params.get(key).and_then(|v| v.as_str());
    let path = str_param("path").unwrap_or("");
    let scope = if is_document {
        UploadScope::Group(str_param("group"))
    } else {
        UploadScope::Repo(str_param("repo_name"))
    };
    upload_conditions(path, scope, metadata_user_id(params.get("metadata")))
}

async fn cleanup_single_file(
    qdrant: Option<&QdrantService>,
    params: &serde_json::Value,
    default_collection: &str,
    is_document: bool,
) -> Result<()> {
    let collection = params
        .get("collection")
        .and_then(|v| v.as_str())
        .unwrap_or(default_collection);
    delete_by(
        qdrant,
        collection,
        Filter::must(file_conditions(params, is_document)),
    )
    .await?;
    if let Some(path) = params.get("path").and_then(|v| v.as_str()) {
        warn_unless_missing(path, std::fs::remove_file(path));
    }
    Ok(())
}

async fn cleanup_website(qdrant: Option<&QdrantService>, params: &serde_json::Value) -> Result<()> {
    let urls: Vec<&str> = params
        .get("urls")
        .and_then(|v| v.as_array())
        .map(|urls| urls.iter().filter_map(|u| u.as_str()).collect())
        .unwrap_or_default();
    if urls.is_empty() {
        return Ok(());
    }
    // `should` is OR: any of the job's URLs, restricted to website chunks.
    let filter = Filter {
        must: vec![Condition::matches("doc_type", "website".to_string())],
        should: urls
            .iter()
            .map(|url| Condition::matches("url", url.to_string()))
            .collect(),
        ..Default::default()
    };
    delete_by(qdrant, "general", filter).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::debug::open_sqlite_pool;

    #[test]
    fn file_conditions_strips_job_prefix() {
        let params = serde_json::json!({
            "path": "/tmp/uploads/2bc57ac7-1234-5678-9abc-def012345678_notes.md",
            "group": "team",
        });
        let conditions = file_conditions(&params, true);
        // file_name + group
        assert_eq!(conditions.len(), 2);
    }

    #[test]
    fn file_conditions_omits_empty_group_and_user_id() {
        let params = serde_json::json!({
            "path": "/tmp/uploads/abc_notes.md",
            "group": "",
            "metadata": {"user_id": "u-1"},
        });
        let conditions = file_conditions(&params, true);
        // file_name + user_id (empty group skipped)
        assert_eq!(conditions.len(), 2);
    }

    #[test]
    fn code_file_conditions_ignore_group() {
        let params = serde_json::json!({
            "path": "/tmp/uploads/abc_main.rs",
            "group": "ignored",
        });
        assert_eq!(file_conditions(&params, false).len(), 1);
    }

    #[test]
    fn code_file_conditions_are_scoped_to_the_repo() {
        let params = serde_json::json!({
            "path": "/tmp/uploads/abc_main.rs",
            "repo_name": "my_repo",
        });
        // file_name + repo_name
        assert_eq!(file_conditions(&params, false).len(), 2);
    }

    #[test]
    fn strip_zip_ext_is_shared_with_ingestion() {
        assert_eq!(strip_zip_ext("/tmp/x.zip"), "/tmp/x");
        assert_eq!(strip_zip_ext("/tmp/x.tar"), "/tmp/x.tar");
    }

    #[tokio::test]
    async fn cleanup_removes_upload_file_and_metadata_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc_notes.md");
        std::fs::write(&path, b"hello").unwrap();

        let pool = open_sqlite_pool(dir.path()).await;
        sqlx::query(
            "INSERT INTO file_metadata (storage_object_id, full_path, status, created_at) \
             VALUES ('job-1', 'notes.md', 'pending', '2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let params = serde_json::json!({
            "path": path.to_string_lossy(),
            "collection": "general",
            "storage_object_id": "job-1",
        });
        cleanup_cancelled_job(None, &pool, "process_documents_upload", &params)
            .await
            .unwrap();

        assert!(!path.exists(), "upload should be removed");
        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM file_metadata WHERE storage_object_id = 'job-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, 0, "file_metadata row should be deleted");
    }

    #[tokio::test]
    async fn failed_cleanup_removes_upload_but_keeps_metadata_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc_main.rs");
        std::fs::write(&path, b"fn main() {}").unwrap();

        let pool = open_sqlite_pool(dir.path()).await;
        sqlx::query(
            "INSERT INTO file_metadata (storage_object_id, full_path, status, created_at) \
             VALUES ('job-2', 'main.rs', 'failed', '2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let params = serde_json::json!({
            "path": path.to_string_lossy(),
            "storage_object_id": "job-2",
        });
        cleanup_failed_job(None, "process_code_file_upload", &params)
            .await
            .unwrap();

        assert!(!path.exists(), "upload should be removed");
        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM file_metadata WHERE storage_object_id = 'job-2'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, 1, "failed job keeps its metadata row");
    }

    #[tokio::test]
    async fn cleanup_zip_removes_archive_and_extract_dir() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("abc_repo.zip");
        std::fs::write(&zip_path, b"PK").unwrap();
        let extract_dir = dir.path().join("abc_repo_extracted");
        std::fs::create_dir_all(&extract_dir).unwrap();
        std::fs::write(extract_dir.join("file.rs"), b"fn main() {}").unwrap();

        let pool = open_sqlite_pool(dir.path()).await;
        let params = serde_json::json!({
            "zip_path": zip_path.to_string_lossy(),
            "zip_filename": "repo.zip",
            "storage_object_id": "job-1",
            "embedding_options": {
                "collection": "codebase",
                "repo_name": "repo",
            },
        });
        cleanup_cancelled_job(None, &pool, "process_zip", &params)
            .await
            .unwrap();

        assert!(!zip_path.exists(), "archive should be removed");
        assert!(!extract_dir.exists(), "extraction dir should be removed");
    }
}
