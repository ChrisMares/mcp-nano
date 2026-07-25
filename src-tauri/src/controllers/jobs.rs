//! Job-upload controller commands.
//!
//! `upload_repo_zip`, `upload_documents`, and `upload_code_files` insert a
//! `PENDING` row into `job_status` per uploaded file. Each uploaded path is
//! copied into `app_local_data_dir()/uploads/<job_uuid>_<orig_filename>` so
//! the worker can read it via the on-disk path (no bytes over IPC — large
//! zips aren't serialized through `invoke`). The worker poll loop then
//! claims the row, dispatches via the registered task name, and runs the
//! actual chunk→embed→upsert pipeline.
//!
//! `get_active_jobs` and `get_job_status` query `job_status` directly (the
//! UI also receives live `job_progress` / `job_finished` / `job_failed`
//! Tauri events from the worker, replacing the previous polling loop).

use std::path::{Path, PathBuf};

use serde_json::json;
use tauri::{AppHandle, Manager};
use tracing::info;
use uuid::Uuid;

use crate::db::DbState;
use crate::models::entities::JobStatus;
use crate::models::request::EmbeddingOptions;
use crate::models::response::{ActiveJobsResponse, UploadJobEntry, UploadResponse};
use crate::worker::progress::{emit_job_event, now_iso, JobProgressEvent};

#[tauri::command]
pub async fn upload_repo_zip(
    app: AppHandle,
    paths: Vec<String>,
    embedding_options: EmbeddingOptions,
) -> Result<UploadResponse, String> {
    info!(
        "upload_repo_zip: {} file(s), collection={:?}",
        paths.len(),
        embedding_options.collection
    );
    enqueue_upload_jobs(
        &app,
        &paths,
        &embedding_options,
        "process_zip",
        |orig_name, dest_path, job_id, options| {
            let mut opts = options.clone();
            if opts.collection.as_deref().unwrap_or("codebase") == "codebase" {
                let needs_default = opts
                    .repo_name
                    .as_ref()
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true);
                if needs_default {
                    let default_name =
                        crate::services::ingestion_service::repo_name_from_zip_filename(orig_name);
                    if !default_name.is_empty() {
                        opts.repo_name = Some(default_name);
                    }
                }
            }
            json!({
                "zip_path": dest_path.to_string_lossy(),
                "zip_filename": orig_name,
                "storage_object_id": job_id,
                "embedding_options": opts,
            })
        },
        true,
    )
    .await
}

#[tauri::command]
pub async fn upload_documents(
    app: AppHandle,
    paths: Vec<String>,
    embedding_options: EmbeddingOptions,
) -> Result<UploadResponse, String> {
    info!(
        "upload_documents: {} file(s), group={:?}",
        paths.len(),
        embedding_options.group
    );
    enqueue_upload_jobs(
        &app,
        &paths,
        &embedding_options,
        "process_documents_upload",
        |_orig_name, dest_path, job_id, options| {
            json!({
                "path": dest_path.to_string_lossy(),
                "storage_object_id": job_id,
                "collection": options.collection.clone(),
                "group": options.group.clone(),
                "metadata": options.metadata.clone(),
            })
        },
        true,
    )
    .await
}

#[tauri::command]
pub async fn upload_code_files(
    app: AppHandle,
    paths: Vec<String>,
    embedding_options: EmbeddingOptions,
) -> Result<UploadResponse, String> {
    info!(
        "upload_code_files: {} file(s), repo={:?}",
        paths.len(),
        embedding_options.repo_name
    );
    enqueue_upload_jobs(
        &app,
        &paths,
        &embedding_options,
        "process_code_file_upload",
        |_orig_name, dest_path, job_id, options| {
            json!({
                "path": dest_path.to_string_lossy(),
                "storage_object_id": job_id,
                "collection": options.collection.clone(),
                "repo_name": options.repo_name.clone(),
                "metadata": options.metadata.clone(),
            })
        },
        true,
    )
    .await
}

#[tauri::command]
pub async fn get_active_jobs(app: AppHandle) -> Result<ActiveJobsResponse, String> {
    let pool = pool_from_state(&app)?;
    let rows = sqlx::query_as::<_, JobStatus>(
        "SELECT * FROM job_status WHERE status IN ('PENDING', 'RUNNING') ORDER BY created_at ASC",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("querying active jobs: {e}"))?;
    let total = rows.len() as i64;
    Ok(ActiveJobsResponse {
        jobs: rows,
        total_count: total,
    })
}

#[tauri::command]
pub async fn get_job_status(app: AppHandle, job_id: String) -> Result<JobStatus, String> {
    let pool = pool_from_state(&app)?;
    sqlx::query_as::<_, JobStatus>("SELECT * FROM job_status WHERE job_id = ?")
        .bind(job_id)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("querying job_status: {e}"))
}

#[tauri::command]
pub async fn get_all_jobs(app: AppHandle) -> Result<Vec<JobStatus>, String> {
    let pool = pool_from_state(&app)?;
    sqlx::query_as::<_, JobStatus>("SELECT * FROM job_status ORDER BY created_at DESC")
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("querying all jobs: {e}"))
}

#[tauri::command]
pub async fn retry_job(app: AppHandle, job_id: String) -> Result<(), String> {
    let pool = pool_from_state(&app)?;
    let now = now_iso();
    let result = sqlx::query(
        "UPDATE job_status SET status = 'PENDING', updated_at = ?, error_message = NULL, result = NULL WHERE job_id = ? AND (status = 'FAILED' OR status = 'PENDING')",
    )
    .bind(&now)
    .bind(&job_id)
    .execute(&pool)
    .await
    .map_err(|e| format!("retrying job: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("Job not found or not in a retryable state".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_pending_jobs(app: AppHandle) -> Result<(), String> {
    let pool = pool_from_state(&app)?;
    sqlx::query("DELETE FROM job_status WHERE status = 'PENDING'")
        .execute(&pool)
        .await
        .map_err(|e| format!("deleting pending jobs: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn delete_all_jobs(app: AppHandle) -> Result<(), String> {
    let pool = pool_from_state(&app)?;
    sqlx::query("DELETE FROM job_status")
        .execute(&pool)
        .await
        .map_err(|e| format!("deleting all jobs: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn get_worker_status(app: AppHandle) -> Result<String, String> {
    let pool = pool_from_state(&app)?;
    let pending: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM job_status WHERE status = 'PENDING'")
            .fetch_one(&pool)
            .await
            .map_err(|e| format!("count pending: {e}"))?;
    let running: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM job_status WHERE status = 'RUNNING'")
            .fetch_one(&pool)
            .await
            .map_err(|e| format!("count running: {e}"))?;
    Ok(format!(
        "{{ \"pending\": {pending}, \"running\": {running} }}"
    ))
}

/// Resolve the SqlitePool from managed state. Returns a useful error
/// message if the app hasn't finished `db::init` yet (the worker also
/// waits on this; the UI shouldn't normally call uploads before
/// initialization completes).
fn pool_from_state(app: &AppHandle) -> Result<sqlx::SqlitePool, String> {
    let state = app
        .try_state::<DbState>()
        .ok_or_else(|| "SQLite not initialized yet".to_string())?;
    Ok(state.pool.clone())
}

/// Resolve `app_local_data_dir()/uploads/`, creating it if missing.
fn uploads_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("resolving app data dir: {e}"))?;
    let uploads = dir.join("uploads");
    std::fs::create_dir_all(&uploads)
        .map_err(|e| format!("creating uploads dir {}: {e}", uploads.display()))?;
    Ok(uploads)
}

/// Shared enqueue path:
/// 1. Generate a job UUID.
/// 2. Copy each path into `uploads/<uuid>_<orig_name>`.
/// 3. Insert a PENDING `job_status` row with `task_name` and a
///    `task_params` JSON derived from the per-task closure.
/// 4. Optionally insert a `file_metadata` row (status `pending`) so Data
///    Management can track the upload; the worker marks it `completed`.
///
/// `task_params_for(orig_name, dest_path, job_id, options)` is per-task.
async fn enqueue_upload_jobs(
    app: &AppHandle,
    paths: &[String],
    embedding_options: &EmbeddingOptions,
    task_name: &str,
    task_params_for: impl Fn(&str, &Path, &str, &EmbeddingOptions) -> serde_json::Value,
    register_file_metadata: bool,
) -> Result<UploadResponse, String> {
    let pool = pool_from_state(app)?;
    let now = now_iso();
    let collection = embedding_options
        .collection
        .clone()
        .unwrap_or_else(|| "general".to_string());

    let mut entries: Vec<UploadJobEntry> = Vec::with_capacity(paths.len());
    let mut errors: Vec<String> = Vec::new();
    for p in paths {
        let uploads_dir = uploads_dir(app)?;
        let source = PathBuf::from(p);
        let orig_name = source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("upload.bin")
            .to_string();
        let job_id = Uuid::new_v4();
        let dest_name = format!("{}_{orig_name}", job_id.simple());
        let dest_path = uploads_dir.join(&dest_name);
        if let Err(e) = std::fs::copy(&source, &dest_path) {
            errors.push(format!("copy {p} to {}: {e}", dest_path.display()));
            continue;
        }
        let job_id_str = job_id.to_string();
        let task_params = task_params_for(&orig_name, &dest_path, &job_id_str, embedding_options);
        let task_params_str =
            serde_json::to_string(&task_params).unwrap_or_else(|_| "{}".to_string());
        if let Err(e) = sqlx::query(
            "INSERT INTO job_status (job_id, status, created_at, updated_at, progress_percentage, task_name, task_params, storage_object_id, file_name) \
             VALUES (?, 'PENDING', ?, ?, 0, ?, ?, ?, ?)",
        )
        .bind(&job_id_str)
        .bind(&now)
        .bind(&now)
        .bind(task_name)
        .bind(&task_params_str)
        .bind(&job_id_str)
        .bind(&orig_name)
        .execute(&pool)
        .await
        {
            errors.push(format!("insert job_status: {e}"));
            let _ = std::fs::remove_file(&dest_path);
            continue;
        }

        if register_file_metadata {
            let repo_name = task_params
                .pointer("/embedding_options/repo_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    task_params
                        .get("repo_name")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .or_else(|| {
                    if task_name == "process_zip" {
                        Some(
                            crate::services::ingestion_service::repo_name_from_zip_filename(
                                &orig_name,
                            ),
                        )
                    } else {
                        embedding_options.repo_name.clone()
                    }
                });
            let group_id = embedding_options.group.clone();
            let size_bytes = std::fs::metadata(&source).ok().map(|m| m.len() as i64);
            let file_type = source
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("bin")
                .to_string();
            if let Err(e) = sqlx::query(
                "INSERT INTO file_metadata \
                 (storage_object_id, full_path, file_type, size_bytes, repo_name, group_id, status, created_at, collection) \
                 VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?)",
            )
            .bind(&job_id_str)
            .bind(&orig_name)
            .bind(&file_type)
            .bind(size_bytes)
            .bind(repo_name)
            .bind(group_id)
            .bind(&now)
            .bind(&collection)
            .execute(&pool)
            .await
            {
                errors.push(format!("insert file_metadata: {e}"));
            }
        }

        emit_job_event(
            app,
            "job_queued",
            &JobProgressEvent::new(
                job_id_str.clone(),
                0,
                "PENDING",
                Some(orig_name.clone()),
                Some("Queued".to_string()),
            ),
        );

        entries.push(UploadJobEntry {
            filename: orig_name,
            job_id: job_id_str,
            collection: collection.clone(),
            status: "PENDING".to_string(),
        });
    }

    let message = if errors.is_empty() {
        format!("Queued {} job(s)", entries.len())
    } else {
        format!(
            "Queued {} job(s); {} error(s): {}",
            entries.len(),
            errors.len(),
            errors.join("; ")
        )
    };
    Ok(UploadResponse {
        message,
        jobs: entries,
        errors,
    })
}
