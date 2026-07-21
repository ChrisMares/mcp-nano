use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tracing::error;

/// A future returned by a [`ProgressCallback`] that, when awaited, persists
/// the progress update to `job_status`.
pub type ProgressFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Progress callback invoked by tasks to report 0..=100 percent completion.
///
/// The callback is **async**: the task must `.await` the returned future to
/// persist the update. This serializes progress writes with task execution
/// (mirroring the Python sync `update_progress` behavior) and eliminates
/// races with the terminal status update in `run_job`.
///
/// Held inside an `Arc` so it's cheap to clone per task invocation.
pub type ProgressCallback =
    Arc<dyn Fn(i32, Option<String>) -> ProgressFuture + Send + Sync>;

/// Event payload emitted alongside each row update when an `AppHandle` is
/// attached. The UI subscribes to `job_progress` once at startup and reacts
/// to these events instead of polling `get_job_status`.
#[derive(Debug, Clone, Serialize)]
pub struct JobProgressEvent {
    pub job_id: String,
    pub percentage: i32,
    pub message: Option<String>,
}

/// A no-op progress callback for tests / tasks that don't report progress.
pub fn noop_progress() -> ProgressCallback {
    Arc::new(|_pct, _msg| Box::pin(async {}))
}

/// Build a progress callback bound to a specific `job_id` that updates the
/// `job_status` row on every invocation. No UI event emission — used by tests
/// and any task that runs without a Tauri `AppHandle`.
pub fn progress_for_job(pool: SqlitePool, job_id: String) -> ProgressCallback {
    Arc::new(move |pct, msg| {
        let pool = pool.clone();
        let job_id = job_id.clone();
        Box::pin(async move {
            if let Err(e) = update_job_progress(&pool, &job_id, pct, msg.as_deref()).await {
                error!("progress update failed for job {job_id}: {e:#}");
            }
        })
    })
}

/// Build a progress callback bound to a specific `job_id` that also pushes
/// a `job_progress` Tauri event via the supplied `AppHandle`. The UI
/// listens for this event once at startup and updates progress bars live
/// without polling — the user-facing replacement for `get_job_status`
/// polling.
pub fn progress_for_job_with_app(
    pool: SqlitePool,
    job_id: String,
    app: AppHandle,
) -> ProgressCallback {
    Arc::new(move |pct, msg| {
        let pool = pool.clone();
        let job_id = job_id.clone();
        let app = app.clone();
        Box::pin(async move {
            if let Err(e) = update_job_progress(&pool, &job_id, pct, msg.as_deref()).await {
                error!("progress update failed for job {job_id}: {e:#}");
            }
            let event = JobProgressEvent {
                job_id: job_id.clone(),
                percentage: pct,
                message: msg.clone(),
            };
            if let Err(e) = app.emit("job_progress", event) {
                error!("emit job_progress failed for job {job_id}: {e:#}");
            }
        })
    })
}

/// Update `job_status.progress_percentage` (clamped to 0..=100) and
/// `updated_at`. Mirrors the Python `update_progress`, which only updates
/// the percentage (not the result/message columns).
pub async fn update_job_progress(
    pool: &SqlitePool,
    job_id: &str,
    pct: i32,
    _msg: Option<&str>,
) -> Result<()> {
    let clamped = pct.clamp(0, 100);
    let now = now_iso();
    sqlx::query(
        "UPDATE job_status SET progress_percentage = ?, updated_at = ? WHERE job_id = ?",
    )
    .bind(clamped)
    .bind(&now)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Set the terminal status of a job. `result` is stored when status is
/// `FINISHED`; `error` is stored when status is `FAILED`. Terminal updates
/// always overwrite the previous values (no COALESCE) so progress callbacks
/// can't race with the final state.
pub async fn set_job_status(
    pool: &SqlitePool,
    job_id: &str,
    status: &str,
    result: Option<&str>,
    error: Option<&str>,
) -> Result<()> {
    let now = now_iso();
    sqlx::query(
        "UPDATE job_status SET status = ?, updated_at = ?, result = ?, error_message = ? WHERE job_id = ?",
    )
    .bind(status)
    .bind(&now)
    .bind(result)
    .bind(error)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// ISO 8601 UTC timestamp. SQLite stores datetimes as text in this format
/// (matches the migration's `DATETIME` columns).
pub fn now_iso() -> String {
    use chrono::Utc;
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_progress_does_not_panic() {
        let p = noop_progress();
        p(50, Some("hello".to_string())).await;
        p(100, None).await;
    }

    #[tokio::test]
    async fn update_job_progress_clamps_to_range() -> Result<()> {
        use sqlx::Row;
        let dir = tempfile::tempdir()?;
        let pool = open_in_memory_pool(&dir).await?;

        sqlx::query(
            "INSERT INTO job_status (job_id, status, progress_percentage) VALUES (?, 'RUNNING', 0)",
        )
        .bind("job-1")
        .execute(&pool)
        .await?;

        update_job_progress(&pool, "job-1", 150, Some("past 100")).await?;
        let row = sqlx::query("SELECT progress_percentage FROM job_status WHERE job_id = ?")
            .bind("job-1")
            .fetch_one(&pool)
            .await?;
        assert_eq!(row.get::<i64, _>(0), 100);

        update_job_progress(&pool, "job-1", -10, None).await?;
        let row = sqlx::query("SELECT progress_percentage FROM job_status WHERE job_id = ?")
            .bind("job-1")
            .fetch_one(&pool)
            .await?;
        assert_eq!(row.get::<i64, _>(0), 0);

        Ok(())
    }

    #[tokio::test]
    async fn set_job_status_records_terminal_state() -> Result<()> {
        use sqlx::Row;
        let dir = tempfile::tempdir()?;
        let pool = open_in_memory_pool(&dir).await?;

        sqlx::query(
            "INSERT INTO job_status (job_id, status, progress_percentage, result) VALUES (?, 'RUNNING', 50, 'in-progress')",
        )
        .bind("job-2")
        .execute(&pool)
        .await?;

        // FINISHED: result overwritten with the new message.
        set_job_status(&pool, "job-2", "FINISHED", Some("done"), None).await?;
        let row =
            sqlx::query("SELECT status, result, error_message FROM job_status WHERE job_id = ?")
                .bind("job-2")
                .fetch_one(&pool)
                .await?;
        assert_eq!(row.get::<String, _>(0), "FINISHED");
        assert_eq!(row.get::<String, _>(1), "done");
        assert!(row.get::<Option<String>, _>(2).is_none());

        // FAILED: error_message set, result cleared (terminal overwrites).
        set_job_status(&pool, "job-2", "FAILED", None, Some("boom")).await?;
        let row =
            sqlx::query("SELECT status, result, error_message FROM job_status WHERE job_id = ?")
                .bind("job-2")
                .fetch_one(&pool)
                .await?;
        assert_eq!(row.get::<String, _>(0), "FAILED");
        assert!(row.get::<Option<String>, _>(1).is_none());
        assert_eq!(row.get::<Option<String>, _>(2).unwrap(), "boom");
        Ok(())
    }

    /// Open a SqlitePool against a fresh tempdir + run migrations.
    async fn open_in_memory_pool(dir: &tempfile::TempDir) -> Result<SqlitePool> {
        let path = dir.path().join("app.db");
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(pool)
    }
}
