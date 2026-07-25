pub mod controllers;
pub mod db;
pub mod mcp;
pub mod models;
pub mod qdrant;
pub mod services;
pub mod worker;

use std::sync::Arc;

use services::{EmbedderState, IngestionService, QdrantService, RagService};
use tauri::{Manager, RunEvent};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use controllers::{data, jobs, mcpconfig, rag, website};
use mcp::{McpAppState, McpState};

// ---------------------------------------------------------------------------
// Logging: debug builds → synchronous stderr (visible in terminal);
//          release builds → async flexi_logger to file (unchanged).
// ---------------------------------------------------------------------------

#[cfg(debug_assertions)]
fn init_logging(_app: &tauri::AppHandle) {
    use std::sync::OnceLock;
    use tracing_subscriber::{fmt, EnvFilter};

    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let log_level =
            std::env::var("RUST_LOG").unwrap_or_else(|_| "debug".to_string());
        fmt()
            .with_env_filter(EnvFilter::new(&log_level))
            .with_timer(fmt::time::SystemTime)
            .with_writer(std::io::stderr)
            .init();
        let _ = eprintln!("[mcp-nano] logging initialized (level={log_level})");
    });
}

#[cfg(not(debug_assertions))]
mod prod_logging {
    use std::sync::OnceLock;

    use flexi_logger::{
        Cleanup, Criterion, Duplicate, FileSpec, Logger, LoggerHandle, Naming, WriteMode,
    };
    use tauri::Manager;
    use tracing::info;

    /// Must outlive the process — dropping it shuts down `WriteMode::Async`.
    static LOGGER_HANDLE: OnceLock<LoggerHandle> = OnceLock::new();

    pub fn init(app: &tauri::AppHandle) {
        if LOGGER_HANDLE.get().is_some() {
            return;
        }
        if let Ok(data_dir) = app.path().app_local_data_dir() {
            let log_dir = data_dir.join("logs");
            if std::fs::create_dir_all(&log_dir).is_ok() {
                match Logger::try_with_str("info")
                    .unwrap()
                    .log_to_file(
                        FileSpec::default()
                            .directory(&log_dir)
                            .basename("mcp-nano")
                            .suffix("log"),
                    )
                    .append()
                    .rotate(
                        Criterion::Size(5_000_000),
                        Naming::Numbers,
                        Cleanup::KeepLogFiles(3),
                    )
                    .write_mode(WriteMode::Async)
                    .format_for_files(flexi_logger::detailed_format)
                    .format_for_stderr(flexi_logger::detailed_format)
                    .duplicate_to_stderr(Duplicate::Warn)
                    .start()
                {
                    Ok(handle) => {
                        let _ = LOGGER_HANDLE.set(handle);
                        info!("Log directory: {}", log_dir.display());
                    }
                    Err(e) => eprintln!("Logger init failed: {e}"),
                }
                return;
            }
        }
        if let Ok(handle) = Logger::try_with_str("info")
            .unwrap()
            .format_for_stderr(flexi_logger::detailed_format)
            .duplicate_to_stderr(Duplicate::Info)
            .start()
        {
            let _ = LOGGER_HANDLE.set(handle);
        }
    }
}

#[cfg(not(debug_assertions))]
fn init_logging(app: &tauri::AppHandle) {
    prod_logging::init(app);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            init_logging(app.handle());
            app.manage(qdrant::BackendStatusState(std::sync::RwLock::new(
                qdrant::BackendStatus::default(),
            )));

            let (http_port, grpc_port, qdrant_child) = qdrant::start(app.handle())?;
            app.manage(qdrant::QdrantChild(std::sync::Mutex::new(Some(
                qdrant_child,
            ))));
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = qdrant::init(handle.clone(), http_port, grpc_port).await {
                    error!("Qdrant initialization failed: {error}");
                    qdrant::publish_status(&handle, |s| {
                        s.qdrant_ready = false;
                        s.qdrant_error = Some(error);
                    });
                }
            });
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match db::init(handle.clone()).await {
                    Ok(()) => qdrant::publish_status(&handle, |s| s.db_ready = true),
                    Err(error) => error!("SQLite initialization failed: {error}"),
                }
            });
            match EmbedderState::models_dir(app.handle()) {
                Ok(dir) => match EmbedderState::load(&dir) {
                    Ok(state) => {
                        let device_mode = state.device_mode().to_string();
                        info!("Embedder models loaded from {} using {device_mode}", dir.display());
                        app.manage(Arc::new(state));
                        qdrant::publish_status(app.handle(), |s| {
                            s.embedders_ready = true;
                            s.embedding_device = Some(device_mode);
                        });
                    }
                    Err(error) => {
                        error!(
                            "Embedder model load failed from {}: {error:#}",
                            dir.display()
                        );
                    }
                },
                Err(error) => error!("Embedder models_dir resolution failed: {error}"),
            }

            let cancel = CancellationToken::new();
            app.manage(cancel.clone());
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let pool = wait_for_pool(&app_handle).await;
                let qdrant_client = wait_for_qdrant_client(&app_handle).await;
                let (pool, qdrant_client) = match (pool, qdrant_client) {
                    (Some(p), Some(q)) => (p, q),
                    _ => {
                        error!("Worker/MCP not started: missing required state (db/qdrant)");
                        return;
                    }
                };
                let embedders = match app_handle.try_state::<Arc<EmbedderState>>() {
                    Some(s) => s.inner().clone(),
                    None => {
                        error!("Worker/MCP not started: EmbedderState not registered");
                        return;
                    }
                };
                let models_dir = match EmbedderState::models_dir(&app_handle) {
                    Ok(d) => d,
                    Err(e) => {
                        error!("Worker/MCP not started: models_dir resolution failed: {e}");
                        return;
                    }
                };
                let qdrant_service = QdrantService::new(qdrant_client);
                let rag = Arc::new(RagService::new(embedders.clone(), qdrant_service.clone()));
                app_handle.manage(rag.clone());

                let mcp_state = Arc::new(McpAppState::new(pool.clone(), rag));
                match mcp::start(mcp_state, cancel.child_token()).await {
                    Ok((port, _handle)) => {
                        app_handle.manage(McpState {
                            port,
                            cancel: cancel.child_token(),
                        });
                        info!("MCP endpoint started on port {port}");
                    }
                    Err(e) => error!("MCP endpoint failed to start: {e}"),
                }

                let ingestion = match IngestionService::new(embedders, qdrant_service, &models_dir)
                {
                    Ok(s) => Arc::new(s),
                    Err(e) => {
                        error!("Worker not started: IngestionService init failed: {e:#}");
                        return;
                    }
                };
                let registry = ingestion.build_task_registry();
                info!(
                    "Starting background worker: tasks={} concurrency=2",
                    registry.names().join(", "),
                );
                let _join = worker::start(pool, registry, cancel, Some(app_handle.clone()));
                qdrant::publish_status(&app_handle, |s| s.worker_ready = true);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            rag::rag_query,
            rag::get_metadata_values,
            rag::get_collections,
            rag::get_metadata_keys,
            rag::get_embedders_status,
            rag::get_backend_status,
            jobs::upload_repo_zip,
            jobs::upload_documents,
            jobs::upload_code_files,
            jobs::get_active_jobs,
            jobs::get_job_status,
            jobs::get_all_jobs,
            jobs::retry_job,
            jobs::delete_pending_jobs,
            jobs::delete_all_jobs,
            jobs::get_worker_status,
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
            mcpconfig::update_mcp_server,
            mcpconfig::toggle_mcp_server,
            mcpconfig::delete_mcp_server,
            mcpconfig::create_mcp_tool,
            mcpconfig::update_mcp_tool,
            mcpconfig::delete_mcp_tool,
            mcpconfig::toggle_mcp_tool,
            mcpconfig::get_mcp_connection_info,
            website::crawl_website,
            website::embed_website,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                if let Some(cancel) = app.try_state::<CancellationToken>() {
                    cancel.cancel();
                }
                qdrant::shutdown(app);
            }
        });
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
