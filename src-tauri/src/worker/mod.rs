pub mod cancel;
pub mod progress;
pub mod tasks;

pub use cancel::{is_job_cancelled, JobCancellation, JobCancelled};
pub use progress::{
    emit_job_cancelled, emit_job_event, noop_progress, progress_for_job, progress_for_job_with_app,
    set_job_status, update_job_progress, JobProgressEvent, ProgressCallback, CANCELLED_MESSAGE,
};
pub use tasks::{BoxedTaskFuture, TaskFn, TaskRegistry};

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::models::entities::JobStatus;
use crate::services::QdrantService;

/// Maximum concurrent in-flight jobs. Mirrors the Python
/// `MAX_CONCURRENT_JOBS = 2`.
const MAX_CONCURRENT_JOBS: usize = 2;

/// Poll interval between worker sweeps. Mirrors `POLL_INTERVAL = 2.0` (sec).
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// A RUNNING job whose `updated_at` is older than this is marked FAILED.
/// Mirrors `STALE_TIMEOUT_MINUTES = 15`.
const STALE_TIMEOUT_MINUTES: i64 = 15;

/// Error message stored on jobs reclaimed by the stale timeout.
const STALE_MESSAGE: &str = "Timed out: no progress update for 15 minutes";

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
///
/// `cancellations` is the shared registry of in-flight job tokens. The worker
/// registers a token per job so `cancel_job` can stop a running task, and
/// removes it once the job reaches a terminal state.
pub fn start(
    pool: SqlitePool,
    registry: TaskRegistry,
    cancel: CancellationToken,
    app: Option<AppHandle>,
    cancellations: JobCancellation,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        run_loop(pool, registry, cancel, app, cancellations).await;
    })
}

async fn run_loop(
    pool: SqlitePool,
    registry: TaskRegistry,
    cancel: CancellationToken,
    app: Option<AppHandle>,
    cancellations: JobCancellation,
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

        // Reclaim stale RUNNING jobs and delete their partial output. A
        // reclaimed job still running in this process is stopped first so it
        // cannot write after the cleanup.
        match reclaim_stale_jobs(&pool).await {
            Ok(stale) => {
                for job in stale {
                    cancellations.cancel(&job.job_id);
                    warn!("worker: job {} timed out; cleaning up", job.job_id);
                    finish_failed_job(&pool, app.as_ref(), &job, &job_params(&job), STALE_MESSAGE)
                        .await;
                }
            }
            Err(e) => error!("stale reclaim failed: {e:#}"),
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
                        let cancellations = cancellations.clone();
                        let task = tokio::spawn(async move {
                            let _permit = sem.acquire_owned().await.expect("sem closed");
                            let job_id = job.job_id.clone();
                            info!(
                                "worker starting job_id={job_id} task={:?} file={:?}",
                                job.task_name, job.file_name
                            );
                            crate::write_ingest_breadcrumb(
                                "job_start",
                                &format!(
                                    "job_id={job_id} task={:?} file={:?}",
                                    job.task_name, job.file_name
                                ),
                            );

                            // Register the token before the task can run so
                            // `cancel_job` always finds it. A cancel that landed
                            // between the claim and this point is only recorded
                            // in the DB, so re-check the row and self-cancel.
                            let job_cancel = CancellationToken::new();
                            cancellations.register(&job_id, job_cancel.clone());
                            if job_row_is_cancelled(&pool, &job_id).await {
                                job_cancel.cancel();
                            }

                            let inner = tokio::spawn({
                                let pool = pool.clone();
                                let app = app.clone();
                                let job = job.clone();
                                async move {
                                    run_job(&pool, &registry, job, app, job_cancel).await;
                                }
                            });
                            let joined = inner.await;
                            cancellations.remove(&job_id);
                            if let Err(join_err) = joined {
                                let msg = if join_err.is_panic() {
                                    let payload = join_err.into_panic();
                                    let detail = crate::panic_payload_to_string(&*payload);
                                    format!("task panicked: {detail}")
                                } else {
                                    format!("task cancelled: {join_err}")
                                };
                                error!("worker task panicked for {job_id}: {msg}");
                                crate::write_ingest_breadcrumb(
                                    "job_panic",
                                    &format!("job_id={job_id} {msg}"),
                                );
                                let _ = set_job_status(&pool, &job_id, "FAILED", None, Some(&msg))
                                    .await;
                                finish_failed_job(
                                    &pool,
                                    app.as_ref(),
                                    &job,
                                    &job_params(&job),
                                    &msg,
                                )
                                .await;
                            }
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
    warn!(
        "worker shutting down: awaiting {} inflight tasks",
        inflight.len()
    );
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
    cancel: CancellationToken,
) {
    let job_id = job.job_id.clone();
    let file_name = job.file_name.clone();
    let task_name = match &job.task_name {
        Some(n) => n.clone(),
        None => {
            let msg = "missing task_name";
            let _ = set_job_status(pool, &job_id, "FAILED", None, Some(msg)).await;
            emit_job_failed(app.as_ref(), &job, msg);
            return;
        }
    };

    let task_fn = match registry.get(&task_name) {
        Some(f) => f.clone(),
        None => {
            let msg = format!("unknown task: {task_name}");
            let _ = set_job_status(pool, &job_id, "FAILED", None, Some(&msg)).await;
            emit_job_failed(app.as_ref(), &job, &msg);
            return;
        }
    };

    let params = job_params(&job);

    if let Some(app) = app.as_ref() {
        progress::emit_job_event(
            app,
            "job_progress",
            &progress::JobProgressEvent::new(
                job_id.clone(),
                0,
                "RUNNING",
                file_name.clone(),
                Some("Starting".to_string()),
            ),
        );
    }

    let progress = match app.clone() {
        Some(app) => progress::progress_for_job_with_app(
            pool.clone(),
            job_id.clone(),
            app,
            file_name.clone(),
        ),
        None => progress_for_job(pool.clone(), job_id.clone()),
    };

    let result = task_fn(params.clone(), progress, cancel.clone()).await;

    // A cancelled job is terminal in its own right: the row was already
    // flipped to CANCELLED by `cancel_job`, so the task's real outcome must
    // not overwrite it.
    if cancel.is_cancelled() || result.as_ref().err().is_some_and(is_job_cancelled) {
        // The stale-job reclaim cancels the token after marking the row
        // FAILED and cleaning up; keep that outcome.
        if job_row_status(pool, &job_id).await.as_deref() == Some("FAILED") {
            return;
        }
        finish_cancelled_job(pool, app.as_ref(), &job_id, &task_name, &file_name, &params).await;
        return;
    }

    match result {
        Ok(msg) => {
            info!("worker: job {job_id} finished: {msg}");
            if let Err(e) = update_job_progress(pool, &job_id, 100, None).await {
                error!("final progress update failed for {job_id}: {e:#}");
            }
            if let Some(app) = app.as_ref() {
                progress::emit_job_event(
                    app,
                    "job_finished",
                    &progress::JobProgressEvent::new(
                        job_id.clone(),
                        100,
                        "FINISHED",
                        file_name.clone(),
                        Some(msg.clone()),
                    ),
                );
            }
            let _ = set_job_status(pool, &job_id, "FINISHED", Some(&msg), None).await;
            if let Err(e) = post_process_file_status(pool, &params, "completed", None).await {
                error!("file_metadata status update failed for {job_id}: {e:#}");
            }
        }
        Err(e) => {
            let msg = format!("{e:#}");
            error!("worker: job {job_id} failed: {msg}");
            let _ = set_job_status(pool, &job_id, "FAILED", None, Some(&msg)).await;
            finish_failed_job(pool, app.as_ref(), &job, &params, &msg).await;
        }
    }
}

/// True when `job_id`'s row is already terminal-`CANCELLED` in SQLite.
///
/// Used by the worker right after registering a job's cancellation token: a
/// cancel that landed between the claim and the registration is only visible
/// in the database, so the job cancels itself instead of running to the end.
async fn job_row_is_cancelled(pool: &SqlitePool, job_id: &str) -> bool {
    job_row_status(pool, job_id).await.as_deref() == Some("CANCELLED")
}

async fn job_row_status(pool: &SqlitePool, job_id: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT status FROM job_status WHERE job_id = ?")
        .bind(job_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

fn qdrant_from_app(app: Option<&AppHandle>) -> Option<QdrantService> {
    app.and_then(|app| app.try_state::<crate::qdrant::QdrantState>())
        .map(|state| QdrantService::new(state.client.clone()))
}

/// The job's `task_params` JSON, or `Null` when absent or unparsable.
fn job_params(job: &JobStatus) -> serde_json::Value {
    job.task_params
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn emit_job_failed(app: Option<&AppHandle>, job: &JobStatus, message: &str) {
    if let Some(app) = app {
        progress::emit_job_event(
            app,
            "job_failed",
            &progress::JobProgressEvent::new(
                job.job_id.clone(),
                100,
                "FAILED",
                job.file_name.clone(),
                Some(message.to_string()),
            ),
        );
    }
}

/// Terminal handling for a failed job whose row is already `FAILED`: mark
/// its `file_metadata` failed, tell the UI, and delete the vectors and
/// uploads it wrote so a partial index is not left searchable.
async fn finish_failed_job(
    pool: &SqlitePool,
    app: Option<&AppHandle>,
    job: &JobStatus,
    params: &serde_json::Value,
    message: &str,
) {
    let job_id = &job.job_id;
    if let Err(e) = post_process_file_status(pool, params, "failed", Some(message)).await {
        error!("file_metadata status update failed for {job_id}: {e:#}");
    }
    emit_job_failed(app, job, message);
    let Some(task_name) = job.task_name.as_deref() else {
        return;
    };
    let qdrant = qdrant_from_app(app);
    match crate::services::job_cleanup::cleanup_failed_job(qdrant.as_ref(), task_name, params).await
    {
        Ok(()) => info!("worker: removed partial output of failed job {job_id}"),
        Err(e) => error!("failed-job cleanup failed for {job_id}: {e:#}"),
    }
}

/// Terminal handling for a cancelled job.
///
/// The row was already flipped to `CANCELLED` by `cancel_job` (or by the
/// self-cancel check), so the status is confirmed here rather than derived
/// from the task result, the UI is told, and everything the run produced is
/// deleted.
async fn finish_cancelled_job(
    pool: &SqlitePool,
    app: Option<&AppHandle>,
    job_id: &str,
    task_name: &str,
    file_name: &Option<String>,
    params: &serde_json::Value,
) {
    let _ = set_job_status(pool, job_id, "CANCELLED", None, Some(CANCELLED_MESSAGE)).await;

    let qdrant = qdrant_from_app(app);
    if let Err(e) = crate::services::job_cleanup::cleanup_cancelled_job(
        qdrant.as_ref(),
        pool,
        task_name,
        params,
    )
    .await
    {
        error!("cancel cleanup failed for {job_id}: {e:#}");
    }

    if let Some(app) = app {
        emit_job_cancelled(app, job_id, file_name.clone());
    }
    info!("worker: job {job_id} cancelled and cleaned up");
    crate::write_ingest_breadcrumb("job_cancelled", &format!("job_id={job_id}"));
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
/// as FAILED and return them. Mirrors the Python `reclaim_stale_jobs`.
async fn reclaim_stale_jobs(pool: &SqlitePool) -> Result<Vec<JobStatus>> {
    let cutoff = chrono::Utc::now() - chrono::Duration::minutes(STALE_TIMEOUT_MINUTES);
    let cutoff_iso = cutoff.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    // SQLite stores datetimes as text in our ISO format; string comparison
    // works because the format is lexicographically ordered.
    let now = progress::now_iso();
    let rows = sqlx::query_as::<_, JobStatus>(
        "UPDATE job_status \
         SET status = 'FAILED', error_message = ?, updated_at = ? \
         WHERE status = 'RUNNING' AND updated_at < ? \
         RETURNING *",
    )
    .bind(STALE_MESSAGE)
    .bind(&now)
    .bind(&cutoff_iso)
    .fetch_all(pool)
    .await?;
    Ok(rows)
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
    use sqlx::Row;

    async fn open_pool(dir: &tempfile::TempDir) -> SqlitePool {
        crate::controllers::debug::open_sqlite_pool(dir.path()).await
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

    /// Poll `job_id` every 100 ms, up to `polls` times, until `done` accepts
    /// its status. Returns that status, or an empty string on timeout.
    async fn wait_for_status(
        pool: &SqlitePool,
        job_id: &str,
        polls: usize,
        done: impl Fn(&str) -> bool,
    ) -> String {
        for _ in 0..polls {
            let (status, _, _) = fetch_job_status(pool, job_id).await;
            if done(&status) {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        String::new()
    }

    async fn insert_running_job(pool: &SqlitePool, updated_at: &str) -> String {
        let job_id = format!("job-{}", uuid::Uuid::new_v4());
        sqlx::query(
            "INSERT INTO job_status (job_id, status, created_at, updated_at, progress_percentage, task_name) \
             VALUES (?, 'RUNNING', ?, ?, 50, 'noop')",
        )
        .bind(&job_id)
        .bind(updated_at)
        .bind(updated_at)
        .execute(pool)
        .await
        .unwrap();
        job_id
    }

    async fn set_row_status(pool: &SqlitePool, job_id: &str, status: &str) {
        sqlx::query("UPDATE job_status SET status = ? WHERE job_id = ?")
            .bind(status)
            .bind(job_id)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn worker_claims_pending_job_and_runs_to_finished() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;

        // Register a noop task that reports progress 0 -> 50 -> 100.
        let mut reg = TaskRegistry::new();
        reg.register("noop", |_params, progress, _cancel| async move {
            progress(0, Some("starting".to_string())).await;
            progress(50, Some("halfway".to_string())).await;
            progress(100, Some("done".to_string())).await;
            Ok("noop completed".to_string())
        });

        let job_id = insert_pending_job(&pool, "noop", "{}").await;
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let handle = start(pool.clone(), reg, cancel, None, JobCancellation::new());

        // Wait for the job to reach FINISHED (poll up to 5s).
        let final_status =
            wait_for_status(&pool, &job_id, 50, |s| s == "FINISHED" || s == "FAILED").await;
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
        let handle = start(pool.clone(), reg, cancel, None, JobCancellation::new());

        let final_status =
            wait_for_status(&pool, &job_id, 50, |s| s == "FAILED" || s == "FINISHED").await;
        cancel_clone.cancel();
        let _ = handle.await;

        assert_eq!(final_status, "FAILED");
    }

    #[tokio::test]
    async fn failed_job_removes_its_upload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;
        let upload = dir.path().join("abc_main.rs");
        std::fs::write(&upload, b"fn main() {}").unwrap();

        let mut reg = TaskRegistry::new();
        reg.register(
            "process_code_file_upload",
            |_params, _progress, _cancel| async move { Err(anyhow::anyhow!("boom")) },
        );
        let params = serde_json::json!({ "path": upload.to_string_lossy() }).to_string();
        let job_id = insert_pending_job(&pool, "process_code_file_upload", &params).await;
        let cancel = CancellationToken::new();
        let handle = start(
            pool.clone(),
            reg,
            cancel.clone(),
            None,
            JobCancellation::new(),
        );

        let final_status =
            wait_for_status(&pool, &job_id, 50, |s| s == "FAILED" && !upload.exists()).await;
        cancel.cancel();
        let _ = handle.await;

        assert_eq!(final_status, "FAILED");
        assert!(!upload.exists(), "failed job's upload should be removed");
    }

    #[tokio::test]
    async fn worker_resets_running_to_pending_on_shutdown() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;

        // Insert a job that is "RUNNING" but never gets claimed (registry empty).
        let job_id = insert_running_job(&pool, &progress::now_iso()).await;

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
        let old = chrono::Utc::now() - chrono::Duration::minutes(20);
        let job_id = insert_running_job(&pool, &old.format("%Y-%m-%dT%H:%M:%SZ").to_string()).await;

        let stale = reclaim_stale_jobs(&pool).await.unwrap();
        assert_eq!(stale.len(), 1, "one stale job should be reclaimed");
        assert_eq!(stale[0].job_id, job_id);

        let row = sqlx::query("SELECT status, error_message FROM job_status WHERE job_id = ?")
            .bind(&job_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>(0), "FAILED");
        let err: String = row.get(1);
        assert!(
            err.contains("Timed out"),
            "error should mention timeout: {err}"
        );
    }

    #[tokio::test]
    async fn reclaim_stale_jobs_skips_fresh_running() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;
        insert_running_job(&pool, &progress::now_iso()).await;

        let stale = reclaim_stale_jobs(&pool).await.unwrap();
        assert!(
            stale.is_empty(),
            "fresh RUNNING job should not be reclaimed"
        );
    }

    #[tokio::test]
    async fn claim_jobs_returns_empty_when_no_pending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;
        let claimed = claim_jobs(&pool, 5).await.unwrap();
        assert!(claimed.is_empty());
    }

    #[tokio::test]
    async fn claim_jobs_skips_cancelled_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;
        let job_id = insert_pending_job(&pool, "noop", "{}").await;
        set_row_status(&pool, &job_id, "CANCELLED").await;

        let claimed = claim_jobs(&pool, 5).await.unwrap();
        assert!(claimed.is_empty(), "a CANCELLED row must never be claimed");
    }

    #[tokio::test]
    async fn reset_running_to_pending_leaves_cancelled_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;
        let cancelled = insert_pending_job(&pool, "noop", "{}").await;
        let running = insert_pending_job(&pool, "noop", "{}").await;
        set_row_status(&pool, &cancelled, "CANCELLED").await;
        set_row_status(&pool, &running, "RUNNING").await;

        reset_running_to_pending(&pool).await.unwrap();
        let (cancelled_status, _, _) = fetch_job_status(&pool, &cancelled).await;
        let (running_status, _, _) = fetch_job_status(&pool, &running).await;
        assert_eq!(cancelled_status, "CANCELLED");
        assert_eq!(running_status, "PENDING");
    }

    #[tokio::test]
    async fn cancelled_running_job_ends_cancelled_not_failed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = open_pool(&dir).await;

        let mut reg = TaskRegistry::new();
        reg.register("long_running", |_params, _progress, cancel| async move {
            tokio::select! {
                _ = cancel.cancelled() => Err(anyhow::Error::new(JobCancelled)),
                _ = tokio::time::sleep(Duration::from_secs(30)) => Ok("finished".to_string()),
            }
        });

        let job_id = insert_pending_job(&pool, "long_running", "{}").await;
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let cancellations = JobCancellation::new();
        let handle = start(pool.clone(), reg, cancel_clone, None, cancellations.clone());

        // Wait for the worker to claim the job and register its token.
        let running = wait_for_status(&pool, &job_id, 80, |s| s == "RUNNING").await;
        assert_eq!(running, "RUNNING", "job should be claimed");

        // Mirror `cancel_job`: mark the row, then fire the token.
        set_row_status(&pool, &job_id, "CANCELLED").await;
        assert!(cancellations.cancel(&job_id), "token should be registered");

        let final_status =
            wait_for_status(&pool, &job_id, 80, |s| s != "RUNNING" && s != "PENDING").await;
        cancel.cancel();
        let _ = handle.await;

        assert_eq!(
            final_status, "CANCELLED",
            "a cancelled job must not be recorded as FAILED"
        );
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
