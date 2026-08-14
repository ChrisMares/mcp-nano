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
use tracing::{error, info, warn};

use controllers::{data, jobs, mcpconfig, rag, website};
use mcp::{McpAppState, McpState};

// ---------------------------------------------------------------------------
// Logging: debug builds → stderr + rotating file under app data `logs/`;
//          release builds → async flexi_logger to file (unchanged).
// ---------------------------------------------------------------------------

/// Directory used for app logs and crash breadcrumbs. Set during `init_logging`.
static LOG_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

pub fn log_dir() -> Option<&'static std::path::PathBuf> {
    LOG_DIR.get()
}

pub fn log_directory(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("logs"))
        .map_err(|error| format!("failed to resolve log directory: {error}"))
}

pub fn log_size_bytes(app: &tauri::AppHandle) -> Option<u64> {
    let entries = std::fs::read_dir(log_directory(app).ok()?).ok()?;
    Some(
        entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .sum(),
    )
}

/// Write a single-line breadcrumb that survives panics (flushed). Overwrites
/// `logs/ingest-current.txt` so the last in-flight stage is always visible.
pub fn write_ingest_breadcrumb(stage: &str, detail: &str) {
    let Some(dir) = LOG_DIR.get() else {
        return;
    };
    let path = dir.join("ingest-current.txt");
    let line = format!(
        "{}\t{}\t{}\n",
        chrono_lite_now(),
        stage,
        detail.replace('\n', " ")
    );
    if let Ok(mut f) = std::fs::File::create(&path) {
        use std::io::Write;
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
}

fn chrono_lite_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

/// Extract a human-readable message from a `catch_unwind` panic payload.
pub fn panic_payload_to_string(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".into());
        let payload = panic_payload_to_string(info.payload());
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");
        let breadcrumb = LOG_DIR
            .get()
            .and_then(|d| std::fs::read_to_string(d.join("ingest-current.txt")).ok())
            .unwrap_or_default();
        let bt = std::backtrace::Backtrace::force_capture();
        let dump = format!(
            "=== mcp-nano panic ===\n\
             time_unix={}\n\
             thread={thread_name}\n\
             location={location}\n\
             payload={payload}\n\
             ingest_breadcrumb:\n{breadcrumb}\n\
             backtrace:\n{bt}\n",
            chrono_lite_now()
        );
        eprintln!("{dump}");
        let _ = std::io::Write::flush(&mut std::io::stderr());
        if let Some(dir) = LOG_DIR.get() {
            let path = dir.join("last-panic.log");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                use std::io::Write;
                let _ = writeln!(f, "{dump}");
                let _ = f.flush();
            }
            // Also stamp the live breadcrumb so a paused debugger still leaves
            // a clear "this was a panic" marker even before catch_unwind runs.
            let panic_crumb = format!(
                "{}\tpanic\tthread={thread_name} loc={location} payload={}\n",
                chrono_lite_now(),
                payload.replace('\n', " ")
            );
            if let Ok(mut f) = std::fs::File::create(dir.join("ingest-current.txt")) {
                use std::io::Write;
                let _ = f.write_all(panic_crumb.as_bytes());
                let _ = f.flush();
            }
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("mcp-nano-debug.log"))
            {
                use std::io::Write;
                let _ = writeln!(f, "ERROR panic_hook: {location} | {payload} | crumb={breadcrumb}");
                let _ = f.flush();
            }
        }
        default_hook(info);
    }));
}

#[cfg(debug_assertions)]
fn init_logging(app: &tauri::AppHandle) {
    use std::fs::OpenOptions;
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex, OnceLock};
    use tracing_subscriber::{fmt, EnvFilter};

    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let log_level =
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,mcp_nano_lib=debug".to_string());

        let mut file_writer: Option<Arc<Mutex<std::fs::File>>> = None;
        if let Ok(data_dir) = app.path().app_local_data_dir() {
            let log_dir = data_dir.join("logs");
            if std::fs::create_dir_all(&log_dir).is_ok() {
                let _ = LOG_DIR.set(log_dir.clone());
                let log_path = log_dir.join("mcp-nano-debug.log");
                match OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_path)
                {
                    Ok(f) => {
                        file_writer = Some(Arc::new(Mutex::new(f)));
                        eprintln!(
                            "[mcp-nano] debug log file: {} (level={log_level})",
                            log_path.display()
                        );
                    }
                    Err(e) => eprintln!("[mcp-nano] failed to open debug log file: {e}"),
                }
            }
        }

        install_panic_hook();

        #[derive(Clone)]
        struct TeeWriter {
            file: Option<Arc<Mutex<std::fs::File>>>,
        }

        impl Write for TeeWriter {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                let _ = io::stderr().write_all(buf);
                if let Some(f) = &self.file {
                    if let Ok(mut g) = f.lock() {
                        let _ = g.write_all(buf);
                    }
                }
                Ok(buf.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                let _ = io::stderr().flush();
                if let Some(f) = &self.file {
                    if let Ok(mut g) = f.lock() {
                        let _ = g.flush();
                    }
                }
                Ok(())
            }
        }

        let make = TeeWriter { file: file_writer };
        fmt()
            .with_env_filter(EnvFilter::new(&log_level))
            .with_timer(fmt::time::SystemTime)
            .with_writer(move || make.clone())
            .with_ansi(false)
            .init();
        info!("logging initialized (debug, dual stderr+file)");
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
                let _ = crate::LOG_DIR.set(log_dir.clone());
                crate::install_panic_hook();
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
        crate::install_panic_hook();
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
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))
                {
                    let _ = window.set_icon(icon);
                }
            }
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
                        let model_statuses = state.model_statuses();
                        info!("Embedder models loaded from {} using {device_mode}", dir.display());
                        app.manage(Arc::new(state));
                        qdrant::publish_status(app.handle(), |s| {
                            s.embedders_ready = true;
                            s.embedding_device = Some(device_mode);
                            s.model_statuses = model_statuses;
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

            // Splashscreen: watch for backend readiness (qdrant + db + embedders), then
            // swap the splashscreen out for the real window. A fallback timeout ensures
            // the user is never stuck on the splash if something never becomes ready.
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                const SPLASH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
                const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(120);
                let deadline = tokio::time::Instant::now() + SPLASH_TIMEOUT;

                loop {
                    let ready = app_handle
                        .try_state::<qdrant::BackendStatusState>()
                        .and_then(|state| state.0.read().ok().map(|s| s.qdrant_ready && s.db_ready && s.embedders_ready))
                        .unwrap_or(false);

                    let timed_out = tokio::time::Instant::now() >= deadline;
                    if ready || timed_out {
                        if timed_out && !ready {
                            warn!("Splashscreen timeout reached before backend was ready; showing main window anyway");
                        }
                        if let Some(splash) = app_handle.get_webview_window("splashscreen") {
                            let _ = splash.close();
                        }
                        if let Some(main) = app_handle.get_webview_window("main") {
                            let _ = main.show();
                            let _ = main.set_focus();
                        }
                        break;
                    }
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            rag::rag_query,
            rag::get_metadata_values,
            rag::get_backend_status,
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
