//! Website controller commands.
//!
//! `crawl_website` runs the BFS crawler directly in the controller (no
//! job queue) and returns the URL list. While crawling it emits
//! `crawl_progress` Tauri events so the UI can show the live URL list.
//!
//! `embed_website` enqueues a `process_website_scrape` job row in
//! `job_status` for the worker to claim; the worker dispatches to
//! `IngestionService::process_website_embed`, which scrapes each URL,
//! chunks the result, embeds it via the dense model, and upserts to
//! Qdrant.

use std::sync::Arc;

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::db::DbState;
use crate::models::response::{CrawlResponse, EmbedWebsiteResponse};
use crate::services::ingestion;
use crate::services::ingestion::website::CrawlProgressEvent;
use crate::worker::progress::{emit_job_event, now_iso, JobProgressEvent};

#[derive(Clone, Default)]
pub struct WebsiteCrawlState(pub Arc<tokio::sync::Mutex<Option<CancellationToken>>>);

pub async fn cancel_active_crawl(state: &WebsiteCrawlState) {
    if let Some(token) = state.0.lock().await.as_ref() {
        token.cancel();
    }
}

#[tauri::command]
pub async fn crawl_website(
    app: AppHandle,
    crawl_state: tauri::State<'_, WebsiteCrawlState>,
    url: String,
    depth: Option<i64>,
    same_domain_only: Option<bool>,
    render_javascript: Option<bool>,
) -> Result<CrawlResponse, String> {
    let depth = depth.unwrap_or(1).max(0) as usize;
    let same_domain = same_domain_only.unwrap_or(true);
    let cancellation = CancellationToken::new();
    *crawl_state.0.lock().await = Some(cancellation.clone());
    let app_for_progress = app.clone();
    let on_progress: ingestion::website::CrawlProgressCallback =
        Arc::new(move |ev: CrawlProgressEvent| {
            let _ = app_for_progress.emit("crawl_progress", &ev);
        });
    let result = ingestion::website::crawl_website_with_options_and_cancellation(
        &url,
        depth,
        same_domain,
        render_javascript.unwrap_or(false),
        Some(cancellation.clone()),
        Some(on_progress),
    )
    .await;
    crawl_state.0.lock().await.take();
    let urls = result.map_err(|e| format!("crawling {url}: {e:#}"))?;
    let count = urls.len() as i64;
    Ok(CrawlResponse { urls, count })
}

#[tauri::command]
pub async fn cancel_website_crawl(
    crawl_state: tauri::State<'_, WebsiteCrawlState>,
) -> Result<(), String> {
    cancel_active_crawl(&crawl_state).await;
    Ok(())
}

#[tauri::command]
pub async fn embed_website(
    app: AppHandle,
    urls: Vec<String>,
    group: Option<String>,
    render_javascript: Option<bool>,
) -> Result<EmbedWebsiteResponse, String> {
    let urls: Vec<String> = urls
        .iter()
        .map(|url| ingestion::website::normalize_website_url(url))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("invalid website URL: {e:#}"))?;
    let group_str = group.unwrap_or_else(|| "default".to_string());
    let pool = match app.try_state::<DbState>() {
        Some(state) => state.pool.clone(),
        None => return Err("SQLite not initialized yet".to_string()),
    };
    let job_id = Uuid::new_v4().to_string();
    let now = now_iso();
    let display_name = urls
        .first()
        .cloned()
        .unwrap_or_else(|| "website".to_string());
    let task_params = json!({
        "urls": urls,
        "group": group_str,
        "render_javascript": render_javascript.unwrap_or(false),
    });
    let params_str = serde_json::to_string(&task_params).unwrap_or_else(|_| "{}".to_string());
    sqlx::query(
        "INSERT INTO job_status (job_id, status, created_at, updated_at, progress_percentage, task_name, task_params, file_name) \
         VALUES (?, 'PENDING', ?, ?, 0, ?, ?, ?)",
    )
    .bind(&job_id)
    .bind(&now)
    .bind(&now)
    .bind("process_website_scrape")
    .bind(&params_str)
    .bind(&display_name)
    .execute(&pool)
    .await
    .map_err(|e| format!("inserting job_status: {e}"))?;
    emit_job_event(
        &app,
        "job_queued",
        &JobProgressEvent::new(
            job_id.clone(),
            0,
            "PENDING",
            Some(display_name),
            Some("Queued".to_string()),
        ),
    );
    Ok(EmbedWebsiteResponse {
        job_id,
        url_count: urls.len() as i64,
    })
}
