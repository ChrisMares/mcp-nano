pub mod progress;
pub mod tasks;

pub use progress::{noop_progress, progress_for_job, set_job_status, update_job_progress, ProgressCallback};
pub use tasks::{BoxedTaskFuture, TaskFn, TaskRegistry};

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

use crate::models::entities::JobStatus;

/// Maximum concurrent in-flight jobs. Mirrors the Python
/// `MAX_CONCURRENT_JOBS = 2`.
const MAX_CONCURRENT_JOBS: usize = 2;

/// Poll interval between worker sweeps. Mirrors `POLL_INTERVAL = 2.0` (sec).
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// A RUNNING job whose `updated_at` is older than this is marked FAILED.
/// Mirrors `STALE_TIMEOUT_MINUTES = 15`.
const STALE_TIMEOUT_MINUTES: i64 = 15;

/// Start the worker poll loop. Returns the `JoinHandle` of the loop task so
/// the caller can `.await` it if desired (typically just left running until
/// `cancel` is triggered).
///
/// The loop owns its own `SqlitePool` clone and `TaskRegistry` clone (both
/// are cheap to clone — pool is `Arc` internally, registry is `Arc`-backed).
///
/// When `app` is `Some`, every progress callback emits a `job_progress`
/// Tauri event that the UI listens to live (no polling of `get_job_status`).
/// Tests pass `None` so progress callbacks only update the SQLite row.
pub fn start(
    pool: SqlitePool,
    registry: TaskRegistry,
    cancel: CancellationToken,
    app: Option<AppHandle>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        run_loop(pool, registry, cancel, app).await;
    })
}

async fn run_loop(
    pool: SqlitePool,
    registry: TaskRegistry,
    cancel: CancellationToken,
    app: Option<AppHandle>,
) {
    use tokio::sync::Semaphore;
    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_JOBS));
    let mut inflight: Vec<JoinHandle<()>> = Vec::new();

    loop {
        // Reap completed handles (log any panics). Partition by `is_finished`
        // so we can await the finished ones outside a sync closure.
        let prev = std::mem::take(&mut inflight);
        let (finished, still_running): (Vec<_>, Vec<_>) =
            prev.into_iter().partition(|h| h.is_finished());
        inflight = still_running;
        for h in finished {
            if let Err(panic) = h.await {
                error!("worker task panicked: {panic}");
            }
        }

        // Reclaim stale RUNNING jobs.
        if let Err(e) = reclaim_stale_jobs(&pool).await {
            eprintln!("stale reclaim failed: {e:#}");
        }

        // Claim PENDING jobs up to the available semaphore slots.
        let available = MAX_CONCURRENT_JOBS - inflight.len();
        if available > 0 {
            match claim_jobs(&pool, available).await {
                Ok(jobs) => {
                    for job in jobs {
                        let pool = pool.clone();
                        let registry = registry.clone();
                        let sem = sem.clone();
                        let app = app.clone();
                        let task = tokio::spawn(async move {
                            // Hold the permit for the lifetime of the task.
                            let _permit = sem.acquire_owned().await.expect("sem closed");
                            run_job(&pool, &registry, job, app).await;
                        });
                        inflight.push(task);
                    }
                }
                Err(e) => error!("claim_jobs failed: {e:#}"),
            }
        }

        // Wait for the next poll interval or shutdown.
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(POLL_INTERVAL) => {},
        }
    }

    // Graceful shutdown: reset any still-RUNNING jobs to PENDING so the next
    // launch can pick them up.
    warn!("worker shutting down: awaiting {} inflight tasks", inflight.len());
    for h in inflight {
        let _ = h.await;
    }
    if let Err(e) = reset_running_to_pending(&pool).await {
        error!("reset_running_to_pending failed: {e:#}");
    }
}

/// Execute a single claimed job: dispatch to the registered task, update
/// `job_status` with progress / terminal state, and update `file_metadata`
/// status if `task_params.storage_object_id` is present.
///
/// When `app` is `Some`, the progress callback additionally emits a
/// `job_progress` Tauri event for the UI to consume live (replaces
/// `get_job_status` polling), plus a `job_finished` or `job_failed` event
/// at terminal state.
async fn run_job(
    pool: &SqlitePool,
    registry: &TaskRegistry,
    job: JobStatus,
    app: Option<AppHandle>,
) {
    let job_id = job.job_id.clone();
    let task_name = match &job.task_name {
        Some(n) => n.clone(),
        None => {
            let _ = set_job_status(pool, &job_id, "FAILED", None, Some("missing task_name")).await;
            if let Some(app) = app.as_ref() {
                let _ = app.emit(
                    "job_failed",
                    progress::JobProgressEvent {
                        job_id: job_id.clone(),
                        percentage: 100,
                        message: Some("missing task_name".to_string()),
                    },
                );
            }
            return;
        }
    };

    let task_fn = match registry.get(&task_name) {
        Some(f) => f.clone(),
        None => {
            let msg = format!("unknown task: {task_name}");
            let _ = set_job_status(pool, &job_id, "FAILED", None, Some(&msg)).await;
            if let Some(app) = app.as_ref() {
                let _ = app.emit(
                    "job_failed",
                    progress::JobProgressEvent {
                        job_id: job_id.clone(),
                        percentage: 100,
                        message: Some(msg.clone()),
                    },
                );
            }
            return;
        }
    };

    let params: serde_json::Value = job
        .task_params
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(serde_json::Value::Null);

    let progress = match app.clone() {
        Some(app) => progress::progress_for_job_with_app(pool.clone(), job_id.clone(), app),
        None => progress_for_job(pool.clone(), job_id.clone()),
    };

    let result = task_fn(params.clone(), progress).await;
    match result {
        Ok(msg) => {
            // Set progress to 100 before the terminal status, mirroring the
            // Python `_run_task` which sets `progress_percentage = 100` on
            // FINISHED. Awaits the update so the row reflects 100 before the
            // status flip is observed.
            if let Err(e) = update_job_progress(pool, &job_id, 100, None).await {
                error!("final progress update failed for {job_id}: {e:#}");
            }
            if let Some(app) = app.as_ref() {
                let _ = app.emit(
                    "job_finished",
                    progress::JobProgressEvent {
                        job_id: job_id.clone(),
                        percentage: 100,
                        message: Some(msg.clone()),
                    },
                );
            }
            let _ = set_job_status(pool, &job_id, "FINISHED", Some(&msg), None).await;
            if let Err(e) = post_process_file_status(pool, &params, "completed", None).await {
                error!("file_metadata status update failed for {job_id}: {e:#}");
            }
        }
        Err(e) => {
            let msg = format!("{e:#}");
            if let Some(app) = app.as_ref() {
                let _ = app.emit(
                    "job_failed",
                    progress::JobProgressEvent {
                        job_id: job_id.clone(),
                        percentage: 100,
                        message: Some(msg.clone()),
                    },
                );
            }
            let _ = set_job_status(pool, &job_id, "FAILED", None, Some(&msg)).await;
            if let Err(e) = post_process_file_status(pool, &params, "failed", Some(&msg)).await {
                error!("file_metadata status update failed for {job_id}: {e:#}");
            }
        }
    }
}

/// Update `file_metadata.status` (and optionally `error_message`) for the
/// `storage_object_id` in `params`. Mirrors the Python
/// `_post_process_file_status`.
async fn post_process_file_status(
    pool: &SqlitePool,
    params: &serde_json::Value,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    let storage_object_id = match params.get("storage_object_id").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return Ok(()),
    };
    if let Some(err) = error {
        sqlx::query(
            "UPDATE file_metadata SET status = ?, error_message = ? WHERE storage_object_id = ?",
        )
        .bind(status)
        .bind(err)
        .bind(storage_object_id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query("UPDATE file_metadata SET status = ? WHERE storage_object_id = ?")
            .bind(status)
            .bind(storage_object_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Atomically claim up to `limit` PENDING jobs using `BEGIN IMMEDIATE` +
/// `UPDATE ... RETURNING`. SQLite (3.35+) supports the `RETURNING` clause.
/// Claimed jobs are set to RUNNING with `progress_percentage = 0` and
/// `updated_at = now`.
async fn claim_jobs(pool: &SqlitePool, limit: usize) -> Result<Vec<JobStatus>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let now = progress::now_iso();
    let rows = sqlx::query_as::<_, JobStatus>(
        "UPDATE job_status \
         SET status = 'RUNNING', progress_percentage = 0, updated_at = ? \
         WHERE id IN (\
             SELECT id FROM job_status \
             WHERE status = 'PENDING' AND task_name IS NOT NULL \
             ORDER BY created_at ASC LIMIT ?\
         ) \
         RETURNING *",
    )
    .bind(&now)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
    .context("claim_jobs UPDATE RETURNING")?;
    Ok(rows)
}

/// Mark RUNNING jobs whose `updated_at` is older than `STALE_TIMEOUT_MINUTES`
/// as FAILED. Mirrors the Python `reclaim_stale_jobs`.
async fn reclaim_stale_jobs(pool: &SqlitePool) -> Result<usize> {
    let cutoff = chrono::Utc::now() - chrono::Duration::minutes(STALE_TIMEOUT_MINUTES);
    let cutoff_iso = cutoff.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    // SQLite stores datetimes as text in our ISO format; string comparison
    // works because the format is lexicographically ordered.
    let now = progress::now_iso();
    let result = sqlx::query(
        "UPDATE job_status \
         SET status = 'FAILED', error_message = 'Timed out: no progress update for 15 minutes', updated_at = ? \
         WHERE status = 'RUNNING' AND updated_at < ?",
    )
    .bind(&now)
    .bind(&cutoff_iso)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() as usize)
}

/// On shutdown, reset any RUNNING jobs back to PENDING so the next launch
/// can pick them up. Mirrors the Python `stop()` cleanup.
pub async fn reset_running_to_pending(pool: &SqlitePool) -> Result<usize> {
    let now = progress::now_iso();
    let result = sqlx::query(
        "UPDATE job_status \
         SET status = 'PENDING', progress_percentage = 0, error_message = NULL, updated_at = ? \
         WHERE status = 'RUNNING'",
    )
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::progress::noop_progress;
    use sqlx::Row;

    /// Open a SqlitePool against a fresh tempdir + run migrations.
    async fn open_pool(dir: &tempfile::TempDir) -> SqlitePool {
        let path = dir.path().join("app.db");
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .expect("connect pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        pool
    }

    async fn insert_pending_job(pool: &SqlitePool, task_name: &str, params: &str) -> String {
        let job_id = format!("job-{}", uuid::Uuid::new_v4());
        let now = progress::now_iso();
        sqlx::query(
            "INSERT INTO job_status (job_id, status, created_at, updated_at, progress_percentage, task_name, task_params) \
             VALUES (?, 'PENDING', ?, ?, 0, ?, ?)",
        )
        .bind(&job_id)
        .bind(&now)
        .bind(&now)
        .bind(task_name)
        .bind(params)
        .execute(pool)
        .await
        .expect("insert pending job");
        job_id
    }

    async fn fetch_job_status(pool: &SqlitePool, job_id: &str) -> (String, i64, Option<String>) {
        let row = sqlx::query(
            "SELECT status, progress_percentage, result FROM job_status WHERE job_id = ?",
        )
        .bind(job_id)
        .fetch_one(pool)
        .await
        .expect("fetch job");
        (
            row.get::<String, _>(0),
            row.get::<i64, _>(1),
            row.get::<Option<String>, _>(2),
        )
    }

    #[tokio::test]
    async fn worker_claims_pending_job_and_runs_to_finished() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;

        // Register a noop task that reports progress 0 -> 50 -> 100.
        let mut reg = TaskRegistry::new();
        reg.register("noop", |_params, progress| {
            async move {
                progress(0, Some("starting".to_string())).await;
                progress(50, Some("halfway".to_string())).await;
                progress(100, Some("done".to_string())).await;
                Ok("noop completed".to_string())
            }
        });

        let job_id = insert_pending_job(&pool, "noop", "{}").await;
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let handle = start(pool.clone(), reg, cancel, None);

        // Wait for the job to reach FINISHED (poll up to 5s).
        let mut final_status = String::new();
        for _ in 0..50 {
            let (status, _pct, _result) = fetch_job_status(&pool, &job_id).await;
            if status == "FINISHED" || status == "FAILED" {
                final_status = status;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        cancel_clone.cancel();
        let _ = handle.await;

        assert_eq!(final_status, "FINISHED", "job should reach FINISHED");
        let (status, pct, result) = fetch_job_status(&pool, &job_id).await;
        assert_eq!(status, "FINISHED");
        assert_eq!(pct, 100);
        assert_eq!(result.as_deref(), Some("noop completed"));
    }

    #[tokio::test]
    async fn worker_marks_unknown_task_as_failed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;
        let reg = TaskRegistry::new(); // empty registry

        let job_id = insert_pending_job(&pool, "unknown_task", "{}").await;
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let handle = start(pool.clone(), reg, cancel, None);

        let mut final_status = String::new();
        for _ in 0..50 {
            let (status, _, _) = fetch_job_status(&pool, &job_id).await;
            if status == "FAILED" || status == "FINISHED" {
                final_status = status;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        cancel_clone.cancel();
        let _ = handle.await;

        assert_eq!(final_status, "FAILED");
    }

    #[tokio::test]
    async fn worker_resets_running_to_pending_on_shutdown() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;

        // Insert a job that is "RUNNING" but never gets claimed (registry empty).
        let job_id = format!("job-{}", uuid::Uuid::new_v4());
        let now = progress::now_iso();
        sqlx::query(
            "INSERT INTO job_status (job_id, status, created_at, updated_at, progress_percentage, task_name) \
             VALUES (?, 'RUNNING', ?, ?, 50, 'noop')",
        )
        .bind(&job_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let n = reset_running_to_pending(&pool).await.unwrap();
        assert_eq!(n, 1);
        let (status, pct, _) = fetch_job_status(&pool, &job_id).await;
        assert_eq!(status, "PENDING");
        assert_eq!(pct, 0);
    }

    #[tokio::test]
    async fn claim_jobs_returns_running_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;
        let _jid1 = insert_pending_job(&pool, "noop", "{}").await;
        let _jid2 = insert_pending_job(&pool, "noop", "{}").await;

        let claimed = claim_jobs(&pool, 2).await.unwrap();
        assert_eq!(claimed.len(), 2);
        for job in &claimed {
            assert_eq!(job.status, "RUNNING");
            assert_eq!(job.progress_percentage, 0);
        }
    }

    #[tokio::test]
    async fn reclaim_stale_jobs_marks_old_running_as_failed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;

        // Insert a RUNNING job with an old updated_at (20 minutes ago).
        let job_id = format!("job-{}", uuid::Uuid::new_v4());
        let old = chrono::Utc::now() - chrono::Duration::minutes(20);
        let old_iso = old.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        sqlx::query(
            "INSERT INTO job_status (job_id, status, created_at, updated_at, progress_percentage, task_name) \
             VALUES (?, 'RUNNING', ?, ?, 50, 'noop')",
        )
        .bind(&job_id)
        .bind(&old_iso)
        .bind(&old_iso)
        .execute(&pool)
        .await
        .unwrap();

        let n = reclaim_stale_jobs(&pool).await.unwrap();
        assert_eq!(n, 1, "one stale job should be reclaimed");

        use sqlx::Row;
        let row =
            sqlx::query("SELECT status, error_message FROM job_status WHERE job_id = ?")
                .bind(&job_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row.get::<String, _>(0), "FAILED");
        let err: String = row.get(1);
        assert!(err.contains("Timed out"), "error should mention timeout: {err}");
    }

    #[tokio::test]
    async fn reclaim_stale_jobs_skips_fresh_running() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;
        let job_id = format!("job-{}", uuid::Uuid::new_v4());
        let now = progress::now_iso();
        sqlx::query(
            "INSERT INTO job_status (job_id, status, created_at, updated_at, progress_percentage, task_name) \
             VALUES (?, 'RUNNING', ?, ?, 50, 'noop')",
        )
        .bind(&job_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let n = reclaim_stale_jobs(&pool).await.unwrap();
        assert_eq!(n, 0, "fresh RUNNING job should not be reclaimed");
    }

    #[tokio::test]
    async fn claim_jobs_returns_empty_when_no_pending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;
        let claimed = claim_jobs(&pool, 5).await.unwrap();
        assert!(claimed.is_empty());
    }

    #[tokio::test]
    async fn post_process_file_status_updates_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;
        let now = progress::now_iso();
        sqlx::query(
            "INSERT INTO file_metadata (storage_object_id, full_path, status, created_at) \
             VALUES (?, '/tmp/x.txt', 'pending', ?)",
        )
        .bind("so-1")
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let params = serde_json::json!({"storage_object_id": "so-1"});
        post_process_file_status(&pool, &params, "completed", None)
            .await
            .unwrap();
        let row = sqlx::query("SELECT status FROM file_metadata WHERE storage_object_id = ?")
            .bind("so-1")
            .fetch_one(&pool)
            .await
            .unwrap();
        let status: String = row.get(0);
        assert_eq!(status, "completed");
    }
}
