//! Cleanup for a cancelled job.
//!
//! Each ingestion task deletes the previous version of its data *before* it
//! writes the new one, so a cancelled re-run leaves a partial index behind.
//! Rather than keep that, cancellation removes everything the run could have
//! written, keyed by the same metadata the task uses:
//!
//! * `process_zip` — vectors by `repo_name` (codebase) or `zip_filename`
//!   (general), the uploaded archive and its extraction directory.
//! * `process_documents_upload` — vectors by `file_name` (+ `group`, + `user_id`)
//!   and the uploaded temp file.
//! * `process_code_file_upload` — vectors by `file_name` and the temp file.
//! * `process_website_scrape` — vectors by `doc_type=website` + each `url`.
//!
//! All paths also drop the `file_metadata` row keyed by `storage_object_id`.
//!
//! Called from two places: `cancel_job` for a job that was still `PENDING`
//! (nothing has run yet, but the upload and metadata row exist) and the
//! worker's `finish_cancelled_job` for a running one.

use anyhow::{Context, Result};
use qdrant_client::qdrant::{Condition, Filter};
use sqlx::SqlitePool;
use tracing::{info, warn};

use super::ingestion_service::{repo_name_from_zip_filename, strip_job_prefix, strip_zip_ext};
use super::qdrant_service::QdrantService;

/// Delete everything a cancelled job produced. Best-effort: a failure to
/// remove one artifact is logged by the caller and does not stop the rest.
pub async fn cleanup_cancelled_job(
    qdrant: Option<&QdrantService>,
    pool: &SqlitePool,
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

    if let Some(storage_object_id) = params.get("storage_object_id").and_then(|v| v.as_str()) {
        sqlx::query("DELETE FROM file_metadata WHERE storage_object_id = ?")
            .bind(storage_object_id)
            .execute(pool)
            .await
            .context("deleting file_metadata for cancelled job")?;
    }

    Ok(())
}

async fn delete_by(
    qdrant: Option<&QdrantService>,
    collection: &str,
    filter: Filter,
) -> Result<()> {
    let Some(qdrant) = qdrant else {
        info!("cleanup: Qdrant unavailable, skipping vector delete in {collection}");
        return Ok(());
    };
    qdrant
        .delete_items(collection, None, Some(filter))
        .await
        .with_context(|| format!("deleting cancelled vectors from {collection}"))
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
            .or_else(|| zip_filename.map(|name| Condition::matches("zip_filename", name.to_string())))
    } else {
        zip_filename
            .map(|name| Condition::matches("zip_filename", name.to_string()))
            .or_else(|| repo_name.map(|name| Condition::matches("repo_name", name.to_string())))
    };
    if let Some(condition) = condition {
        delete_by(qdrant, collection, Filter::must([condition])).await?;
    }

    if let Some(zip_path) = params.get("zip_path").and_then(|v| v.as_str()) {
        if let Err(e) = std::fs::remove_file(zip_path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("cleanup: removing {zip_path}: {e}");
            }
        }
        let extract_dir = format!("{}_extracted", strip_zip_ext(zip_path));
        if let Err(e) = std::fs::remove_dir_all(&extract_dir) {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("cleanup: removing {extract_dir}: {e}");
            }
        }
    }
    Ok(())
}

/// Conditions shared by the two single-file upload tasks.
fn file_conditions(params: &serde_json::Value, include_group: bool) -> Vec<Condition> {
    let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let raw_name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let file_name = strip_job_prefix(raw_name).unwrap_or_else(|| raw_name.to_string());

    let mut conditions = vec![Condition::matches("file_name", file_name)];
    if include_group {
        if let Some(group) = params.get("group").and_then(|v| v.as_str()) {
            if !group.is_empty() {
                conditions.push(Condition::matches("group", group.to_string()));
            }
        }
    }
    if let Some(user_id) = params
        .get("metadata")
        .and_then(|m| m.pointer("/user_id"))
        .and_then(|v| v.as_str())
    {
        conditions.push(Condition::matches("user_id", user_id.to_string()));
    }
    conditions
}

async fn cleanup_single_file(
    qdrant: Option<&QdrantService>,
    params: &serde_json::Value,
    default_collection: &str,
    include_group: bool,
) -> Result<()> {
    let collection = params
        .get("collection")
        .and_then(|v| v.as_str())
        .unwrap_or(default_collection);
    delete_by(
        qdrant,
        collection,
        Filter::must(file_conditions(params, include_group)),
    )
    .await?;
    remove_upload(params);
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

fn remove_upload(params: &serde_json::Value) {
    if let Some(path) = params.get("path").and_then(|v| v.as_str()) {
        if let Err(e) = std::fs::remove_file(path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("cleanup: removing {path}: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn strip_zip_ext_is_shared_with_ingestion() {
        assert_eq!(strip_zip_ext("/tmp/x.zip"), "/tmp/x");
        assert_eq!(strip_zip_ext("/tmp/x.tar"), "/tmp/x.tar");
    }

    #[tokio::test]
    async fn cleanup_removes_upload_file_and_metadata_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc_notes.md");
        std::fs::write(&path, b"hello").unwrap();

        let pool = temp_pool(&dir).await;
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
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM file_metadata WHERE storage_object_id = 'job-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, 0, "file_metadata row should be deleted");
    }

    #[tokio::test]
    async fn cleanup_zip_removes_archive_and_extract_dir() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("abc_repo.zip");
        std::fs::write(&zip_path, b"PK").unwrap();
        let extract_dir = dir.path().join("abc_repo_extracted");
        std::fs::create_dir_all(&extract_dir).unwrap();
        std::fs::write(extract_dir.join("file.rs"), b"fn main() {}").unwrap();

        let pool = temp_pool(&dir).await;
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

    async fn temp_pool(dir: &tempfile::TempDir) -> SqlitePool {
        let path = dir.path().join("app.db");
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }
}
