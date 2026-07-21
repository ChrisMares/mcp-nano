pub mod controllers;
pub mod db;
pub mod models;
pub mod qdrant;
pub mod services;
pub mod worker;

use services::{EmbedderState, IngestionService, QdrantService};
use tauri::Manager;
use tokio_util::sync::CancellationToken;

use controllers::{data, jobs, mcpconfig, rag, website};

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let (http_port, grpc_port) = qdrant::start(app.handle())?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = qdrant::init(handle, http_port, grpc_port).await {
                    eprintln!("Qdrant initialization failed: {error}");
                }
            });
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = db::init(handle).await {
                    eprintln!("SQLite initialization failed: {error}");
                }
            });
            // Load embedder + reranker models (mmap'd, ~174 MB) and register
            // them as managed state (wrapped in Arc for cheap sharing across
            // worker tasks and command handlers). The worker spawn below
            // reads this back via `app.try_state::<Arc<EmbedderState>>()`.
            match EmbedderState::models_dir(app.handle()) {
                Ok(dir) => match EmbedderState::load(&dir) {
                    Ok(state) => {
                        println!("Embedder models loaded from {}", dir.display());
                        app.manage(std::sync::Arc::new(state));
                    }
                    Err(error) => {
                        eprintln!(
                            "Embedder model load failed from {}: {error:#}",
                            dir.display()
                        );
                    }
                },
                Err(error) => eprintln!("Embedder models_dir resolution failed: {error}"),
            }

            // Spawn the background worker poll loop. The worker needs:
            //   - SQLite pool (for claiming jobs + updating status)
            //   - Task registry (binding ingestion_service methods)
            //   - QdrantService + EmbedderState (via IngestionService)
            // DB and Qdrant init are async; we spawn a follow-up task that
            // polls for them to be registered, then starts the worker.
            let cancel = CancellationToken::new();
            app.manage(cancel.clone());
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let pool = wait_for_pool(&app_handle).await;
                let qdrant_client = wait_for_qdrant_client(&app_handle).await;
                let (pool, qdrant_client) = match (pool, qdrant_client) {
                    (Some(p), Some(q)) => (p, q),
                    _ => {
                        eprintln!(
                            "Worker not started: missing required state (db/qdrant)"
                        );
                        return;
                    }
                };
                let embedders = match app_handle.try_state::<std::sync::Arc<EmbedderState>>() {
                    Some(s) => s.inner().clone(),
                    None => {
                        eprintln!("Worker not started: EmbedderState not registered");
                        return;
                    }
                };
                let models_dir = match EmbedderState::models_dir(&app_handle) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("Worker not started: models_dir resolution failed: {e}");
                        return;
                    }
                };
                let qdrant_service = QdrantService::new(qdrant_client);
                let ingestion = match IngestionService::new(embedders, qdrant_service, &models_dir) {
                    Ok(s) => std::sync::Arc::new(s),
                    Err(e) => {
                        eprintln!("Worker not started: IngestionService init failed: {e:#}");
                        return;
                    }
                };
                let registry = ingestion.build_task_registry();
                println!(
                    "Starting background worker: tasks={} concurrency=2",
                    registry.names().join(", "),
                );
                let _join = worker::start(pool, registry, cancel, Some(app_handle.clone()));
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            rag::rag_query,
            rag::get_metadata_values,
            jobs::upload_repo_zip,
            jobs::upload_documents,
            jobs::upload_code_files,
            jobs::get_active_jobs,
            jobs::get_job_status,
            data::get_files,
            data::delete_repo,
            data::delete_document,
            data::delete_group,
            data::clear_user_collection,
            data::get_websites,
            data::delete_website,
            data::delete_website_group,
            data::clear_websites,
            mcpconfig::get_mcp_servers,
            mcpconfig::create_mcp_server,
            mcpconfig::get_mcp_server,
            mcpconfig::delete_mcp_server,
            mcpconfig::create_mcp_tool,
            mcpconfig::update_mcp_tool,
            mcpconfig::delete_mcp_tool,
            mcpconfig::toggle_mcp_tool,
            mcpconfig::get_mcp_connection_info,
            website::crawl_website,
            website::embed_website,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Poll `app.try_state::<DbState>()` up to 6 seconds, returning the pool
/// once DB init completes. Returns `None` if the DB never comes online.
async fn wait_for_pool(app: &tauri::AppHandle) -> Option<sqlx::SqlitePool> {
    for _ in 0..60 {
        if let Some(state) = app.try_state::<db::DbState>() {
            return Some(state.pool.clone());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    None
}

/// Poll `app.try_state::<QdrantState>()` up to 30 seconds, returning the
/// Qdrant client once init completes. Returns `None` if Qdrant never comes
/// online.
async fn wait_for_qdrant_client(app: &tauri::AppHandle) -> Option<qdrant_client::Qdrant> {
    for _ in 0..300 {
        if let Some(state) = app.try_state::<qdrant::QdrantState>() {
            return Some(state.client.clone());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    None
}
