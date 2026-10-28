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
use tracing::{error, info};
use uuid::Uuid;

use crate::db::DbState;
use crate::models::entities::JobStatus;
use crate::models::request::EmbeddingOptions;
use crate::models::response::{ActiveJobsResponse, UploadJobEntry, UploadResponse};
use crate::qdrant::QdrantState;
use crate::services::job_cleanup::cleanup_cancelled_job;
use crate::services::qdrant_service::QdrantService;
use crate::worker::progress::{
    emit_job_cancelled, emit_job_event, now_iso, JobProgressEvent, CANCELLED_MESSAGE,
};
use crate::worker::JobCancellation;

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

/// Cancel a queued or running job and delete everything it produced.
///
/// A `PENDING` job is cancelled with a conditional `UPDATE` that races safely
/// with `claim_jobs`: SQLite serializes writers, so exactly one of the two
/// statements can match the row. A `RUNNING` job is marked `CANCELLED` first
/// and then stopped through its cancellation token; the worker deletes the
/// partial data and emits `job_cancelled`.
#[tauri::command]
pub async fn cancel_job(app: AppHandle, job_id: String) -> Result<(), String> {
    let pool = pool_from_state(&app)?;
    let now = now_iso();

    let pending = sqlx::query(
        "UPDATE job_status SET status = 'CANCELLED', error_message = ?, updated_at = ? \
         WHERE job_id = ? AND status = 'PENDING'",
    )
    .bind(CANCELLED_MESSAGE)
    .bind(&now)
    .bind(&job_id)
    .execute(&pool)
    .await
    .map_err(|e| format!("cancelling pending job: {e}"))?;

    let job = sqlx::query_as::<_, JobStatus>("SELECT * FROM job_status WHERE job_id = ?")
        .bind(&job_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("querying job_status: {e}"))?
        .ok_or_else(|| format!("job {job_id} not found"))?;

    if pending.rows_affected() == 1 {
        // The job never ran, but the copied upload and its metadata row exist.
        let params = parse_params(job.task_params.as_deref());
        let qdrant = qdrant_from_state(&app);
        if let Err(e) = cleanup_cancelled_job(
            qdrant.as_ref(),
            &pool,
            job.task_name.as_deref().unwrap_or_default(),
            &params,
        )
        .await
        {
            error!("cancel cleanup failed for {job_id}: {e:#}");
        }
        emit_job_cancelled(&app, &job_id, job.file_name.clone());
        info!("cancel_job: {job_id} was PENDING and is now CANCELLED");
        return Ok(());
    }

    if job.status == "CANCELLED" {
        // Idempotent: a second click while the worker is tearing down.
        return Ok(());
    }

    if job.status != "RUNNING" {
        return Err(format!(
            "job {job_id} is not active (status: {})",
            job.status
        ));
    }

    // Mark the row before firing the token. The worker re-reads the row right
    // after registering its token, so a cancel landing in that window is not
    // lost.
    let running = sqlx::query(
        "UPDATE job_status SET status = 'CANCELLED', error_message = ?, updated_at = ? \
         WHERE job_id = ? AND status = 'RUNNING'",
    )
    .bind(CANCELLED_MESSAGE)
    .bind(&now)
    .bind(&job_id)
    .execute(&pool)
    .await
    .map_err(|e| format!("cancelling running job: {e}"))?;

    if running.rows_affected() == 0 {
        // The worker reached a terminal state between our SELECT and UPDATE.
        let current: Option<String> =
            sqlx::query_scalar("SELECT status FROM job_status WHERE job_id = ?")
                .bind(&job_id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| format!("querying job_status: {e}"))?;
        return match current.as_deref() {
            Some("CANCELLED") => Ok(()),
            Some(status) => Err(format!("job {job_id} already {status}")),
            None => Err(format!("job {job_id} not found")),
        };
    }

    let fired = app
        .try_state::<JobCancellation>()
        .map(|registry| registry.cancel(&job_id))
        .unwrap_or(false);
    if fired {
        info!("cancel_job: {job_id} RUNNING, token fired");
    } else {
        // Registered a moment later; the worker sees the CANCELLED row and
        // stops itself.
        info!("cancel_job: {job_id} marked CANCELLED before its token was registered");
    }
    Ok(())
}

fn parse_params(raw: Option<&str>) -> serde_json::Value {
    raw.and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn qdrant_from_state(app: &AppHandle) -> Option<QdrantService> {
    app.try_state::<QdrantState>()
        .map(|state| QdrantService::new(state.client.clone()))
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
/// 4. Insert a `file_metadata` row (status `pending`) so Data Management
///    can track the upload; the worker marks it `completed`.
///
/// `task_params_for(orig_name, dest_path, job_id, options)` is per-task.
async fn enqueue_upload_jobs(
    app: &AppHandle,
    paths: &[String],
    embedding_options: &EmbeddingOptions,
    task_name: &str,
    task_params_for: impl Fn(&str, &Path, &str, &EmbeddingOptions) -> serde_json::Value,
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
                        crate::services::ingestion_service::repo_name_from_zip_filename(&orig_name),
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
