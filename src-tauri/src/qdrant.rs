use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::Read,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, RwLock},
    time::Duration,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use qdrant_client::qdrant::{
    vectors_config, CreateCollectionBuilder, CreateFieldIndexCollectionBuilder, Distance,
    FieldType, KeywordIndexParamsBuilder, Modifier, PayloadSchemaType, SparseIndexConfig,
    SparseVectorConfig, SparseVectorParams, VectorParams, VectorParamsMap, VectorsConfig,
};
use qdrant_client::Qdrant;
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info, warn};
use crate::models::response::ModelStatusResponse;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

/// Owns the Qdrant sidecar so it is killed on app exit.
pub struct QdrantChild(pub Mutex<Option<Child>>);

/// Frontend-facing readiness (managed state + `backend_status` events).
#[derive(Debug, Clone, serde::Serialize)]
pub struct BackendStatus {
    pub qdrant_ready: bool,
    pub qdrant_error: Option<String>,
    pub http_port: Option<u16>,
    pub grpc_port: Option<u16>,
    pub db_ready: bool,
    pub embedders_ready: bool,
    pub embedding_device: Option<String>,
    pub model_statuses: Vec<ModelStatusResponse>,
    pub worker_ready: bool,
}

impl Default for BackendStatus {
    fn default() -> Self {
        Self {
            qdrant_ready: false,
            qdrant_error: None,
            http_port: None,
            grpc_port: None,
            db_ready: false,
            embedders_ready: false,
            embedding_device: None,
            model_statuses: Vec::new(),
            worker_ready: false,
        }
    }
}

pub struct BackendStatusState(pub RwLock<BackendStatus>);

// Must match the dense embedder output size (Snowflake/snowflake-arctic-embed-xs).
pub const EMBEDDING_DIM: u64 = 384;

/// Prefetch overfetch multiplier for hybrid RRF (docs recommend prefetch > final limit).
pub const PREFETCH_OVERFETCH: u64 = 4;

const COLLECTIONS: [&str; 2] = ["codebase", "general"];

/// Payload indexes. `is_tenant` marks primary partition keys for filtered HNSW.
const PAYLOAD_INDEXES: [(&str, &str, FieldType, bool); 10] = [
    ("codebase", "repo_name", FieldType::Keyword, true),
    ("codebase", "file_name", FieldType::Keyword, false),
    ("codebase", "created_at", FieldType::Datetime, false),
    ("general", "file_name", FieldType::Keyword, false),
    ("general", "group", FieldType::Keyword, true),
    ("general", "url", FieldType::Keyword, false),
    ("general", "website_key", FieldType::Keyword, false),
    ("general", "zip_filename", FieldType::Keyword, false),
    ("general", "doc_type", FieldType::Keyword, false),
    ("general", "created_at", FieldType::Datetime, false),
];

pub struct QdrantState {
    pub client: Qdrant,
    pub http_port: u16,
    pub grpc_port: u16,
}

/// Shared hybrid collection create config (dense Cosine + sparse BM25 with IDF).
pub fn hybrid_collection_builder(name: &str, dense_dim: u64) -> CreateCollectionBuilder {
    let mut dense_map = HashMap::new();
    dense_map.insert(
        "dense".to_string(),
        VectorParams {
            size: dense_dim,
            distance: Distance::Cosine as i32,
            ..Default::default()
        },
    );

    let mut sparse_map = HashMap::new();
    sparse_map.insert(
        "sparse".to_string(),
        SparseVectorParams {
            index: Some(SparseIndexConfig {
                on_disk: Some(false),
                ..Default::default()
            }),
            modifier: Some(Modifier::Idf as i32),
        },
    );

    CreateCollectionBuilder::new(name)
        .vectors_config(VectorsConfig {
            config: Some(vectors_config::Config::ParamsMap(VectorParamsMap {
                map: dense_map,
            })),
        })
        .sparse_vectors_config(SparseVectorConfig { map: sparse_map })
}

fn available_port() -> Result<u16, String> {
    TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("failed to reserve a Qdrant port: {error}"))?
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| format!("failed to read the reserved Qdrant port: {error}"))
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("failed to resolve application data directory: {error}"))?;
    fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create application data directory: {error}"))?;
    Ok(dir)
}

pub fn storage_path(app: &AppHandle) -> Result<PathBuf, String> {
    let storage = data_dir(app)?.join("qdrant");
    fs::create_dir_all(&storage)
        .map_err(|error| format!("failed to create Qdrant storage directory: {error}"))?;
    Ok(storage)
}

fn pidfile_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("qdrant.pid"))
}

fn write_pidfile(app: &AppHandle, pid: u32) -> Result<(), String> {
    let path = pidfile_path(app)?;
    fs::write(&path, pid.to_string()).map_err(|e| format!("failed to write qdrant pidfile: {e}"))
}

fn clear_pidfile(app: &AppHandle) {
    if let Ok(path) = pidfile_path(app) {
        let _ = fs::remove_file(path);
    }
}

fn kill_from_pidfile(app: &AppHandle) {
    let Ok(path) = pidfile_path(app) else {
        return;
    };
    let Ok(contents) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(pid) = contents.trim().parse::<u32>() else {
        let _ = fs::remove_file(&path);
        return;
    };
    if pid > 1 {
        warn!("killing Qdrant from pidfile pid={pid}");
        #[cfg(unix)]
        {
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
            std::thread::sleep(Duration::from_millis(300));
            if Path::new(&format!("/proc/{pid}")).exists() {
                let _ = Command::new("kill")
                    .args(["-KILL", &pid.to_string()])
                    .status();
            }
        }
        #[cfg(windows)]
        {
            let mut command = Command::new("taskkill");
            hide_console(&mut command);
            let _ = command.args(["/PID", &pid.to_string(), "/F"]).status();
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = fs::remove_file(&path);
}

/// Resolve the bundled qdrant binary next to the running executable.
fn qdrant_binary() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "current executable has no parent directory".to_string())?;
    let dir = if dir.ends_with("deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };

    #[cfg(windows)]
    let candidates = [
        dir.join("qdrant.exe"),
        dir.join("qdrant-x86_64-pc-windows-msvc.exe"),
    ];
    #[cfg(not(windows))]
    let candidates = [
        dir.join("qdrant"),
        dir.join("qdrant-x86_64-unknown-linux-gnu"),
        dir.join("../binaries/qdrant-x86_64-unknown-linux-gnu"),
    ];

    for path in candidates {
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(format!(
        "bundled Qdrant binary not found next to {} (run npm run ensure:qdrant)",
        dir.display()
    ))
}

pub fn start(app: &AppHandle) -> Result<(u16, u16, Child), String> {
    let storage = storage_path(app)?;
    kill_from_pidfile(app);
    kill_orphaned_sidecars();

    let bin = qdrant_binary()?;
    let snapshots = storage.join("snapshots");
    fs::create_dir_all(&snapshots)
        .map_err(|error| format!("failed to create Qdrant snapshots directory: {error}"))?;

    let log_path = data_dir(app)?.join("logs").join("qdrant-sidecar.log");
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|error| format!("failed to open Qdrant log {}: {error}", log_path.display()))?;

    let mut last_err = String::new();
    for attempt in 1..=5 {
        let http_port = available_port()?;
        let grpc_port = available_port()?;

        let stdout = log_file
            .try_clone()
            .map_err(|error| format!("failed to clone Qdrant log handle: {error}"))?;
        let stderr = log_file
            .try_clone()
            .map_err(|error| format!("failed to clone Qdrant log handle: {error}"))?;

        let mut qdrant_command = Command::new(&bin);
        hide_console(&mut qdrant_command);
        let mut spawned = qdrant_command
            .env("QDRANT__SERVICE__HOST", "127.0.0.1")
            .env("QDRANT__SERVICE__HTTP_PORT", http_port.to_string())
            .env("QDRANT__SERVICE__GRPC_PORT", grpc_port.to_string())
            .env(
                "QDRANT__STORAGE__STORAGE_PATH",
                storage.to_string_lossy().as_ref(),
            )
            .env(
                "QDRANT__STORAGE__SNAPSHOTS_PATH",
                snapshots.to_string_lossy().as_ref(),
            )
            .env("QDRANT__LOG_LEVEL", "INFO")
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|error| format!("failed to start Qdrant ({}): {error}", bin.display()))?;

        let pid = spawned.id();
        write_pidfile(app, pid)?;
        info!(
            "Qdrant started (pid {pid}, attempt {attempt}) binary={} http://127.0.0.1:{http_port} grpc={grpc_port}; storage={} log={}",
            bin.display(),
            storage.display(),
            log_path.display()
        );

        std::thread::sleep(Duration::from_millis(500));
        match spawned.try_wait() {
            Ok(Some(status)) => {
                clear_pidfile(app);
                let tail = tail_log(&log_path, 40);
                last_err = format!(
                    "Qdrant exited immediately with {status} (attempt {attempt}). Last log lines:\n{tail}"
                );
                warn!("{last_err}");
                kill_from_pidfile(app);
                kill_orphaned_sidecars();
            }
            Ok(None) => {
                let watch_pid = pid;
                let watch_log = log_path.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        #[cfg(unix)]
                        let alive = Path::new(&format!("/proc/{watch_pid}")).exists();
                        #[cfg(windows)]
                        let alive = {
                            let mut tasklist = Command::new("tasklist");
                            hide_console(&mut tasklist);
                            tasklist
                                .args(["/FI", &format!("PID eq {watch_pid}"), "/NH"])
                                .output()
                                .map(|o| String::from_utf8_lossy(&o.stdout).contains(&watch_pid.to_string()))
                                .unwrap_or(false)
                        };
                        #[cfg(not(any(unix, windows)))]
                        let alive = true;
                        if !alive {
                            let tail = tail_log(&watch_log, 20);
                            error!("qdrant process {watch_pid} is gone. Last log lines:\n{tail}");
                            break;
                        }
                    }
                });
                return Ok((http_port, grpc_port, spawned));
            }
            Err(error) => {
                warn!("could not poll Qdrant child status: {error}");
                let _ = spawned.kill();
                let _ = spawned.wait();
                clear_pidfile(app);
            }
        }
    }

    Err(if last_err.is_empty() {
        "failed to start Qdrant after retries".into()
    } else {
        last_err
    })
}

fn tail_log(path: &Path, lines: usize) -> String {
    let Ok(mut f) = File::open(path) else {
        return String::new();
    };
    let mut content = String::new();
    let _ = f.read_to_string(&mut content);
    content
        .lines()
        .rev()
        .take(lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn shutdown(app: &AppHandle) {
    if let Some(state) = app.try_state::<QdrantChild>() {
        if let Ok(mut guard) = state.0.lock() {
            if let Some(mut child) = guard.take() {
                let pid = child.id();
                match child.kill() {
                    Ok(()) => {
                        let _ = child.wait();
                        info!("Qdrant sidecar stopped (pid {pid})");
                    }
                    Err(error) => warn!("failed to kill Qdrant sidecar (pid {pid}): {error}"),
                }
            }
        }
    }
    clear_pidfile(app);
}

#[cfg(unix)]
fn is_our_qdrant_cmdline(cmd: &str) -> bool {
    cmd.contains("target/debug/qdrant")
        || cmd.contains("target/release/qdrant")
        || cmd.contains("qdrant-x86_64-unknown-linux-gnu")
        || cmd.contains("qdrant-x86_64-pc-windows-msvc")
        || cmd.ends_with("\\qdrant.exe")
        || cmd.ends_with("/qdrant")
}

#[cfg(unix)]
fn list_our_qdrant_pids() -> Vec<i32> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids = Vec::new();
    for entry in entries.flatten() {
        let pid: i32 = match entry.file_name().to_string_lossy().parse() {
            Ok(p) if p > 1 => p,
            _ => continue,
        };
        let Ok(cmdline) = fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        if is_our_qdrant_cmdline(&String::from_utf8_lossy(&cmdline)) {
            pids.push(pid);
        }
    }
    pids
}

#[cfg(not(unix))]
fn list_our_qdrant_pids() -> Vec<i32> {
    Vec::new()
}

fn kill_orphaned_sidecars() {
    let pids = list_our_qdrant_pids();
    if pids.is_empty() {
        return;
    }
    for pid in &pids {
        warn!("killing orphaned Qdrant sidecar pid={pid}");
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
    for _ in 0..20 {
        if list_our_qdrant_pids().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    for pid in list_our_qdrant_pids() {
        warn!("force-killing Qdrant sidecar pid={pid}");
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
    for _ in 0..20 {
        if list_our_qdrant_pids().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(200));
}

pub fn publish_status(app: &AppHandle, update: impl FnOnce(&mut BackendStatus)) {
    if let Some(state) = app.try_state::<BackendStatusState>() {
        if let Ok(mut guard) = state.0.write() {
            update(&mut guard);
            let snapshot = guard.clone();
            drop(guard);
            let _ = app.emit("backend_status", snapshot);
        }
    }
}

/// Connect, wait for HTTP readiness, ensure collections/indexes, register state.
pub async fn init(app: AppHandle, http_port: u16, grpc_port: u16) -> Result<(), String> {
    wait_for_readyz(http_port).await?;
    let client = connect_grpc(grpc_port).await?;
    info!("Qdrant connected on gRPC port {grpc_port} (HTTP {http_port})");

    ensure_collections(&client).await?;

    app.manage(QdrantState {
        client,
        http_port,
        grpc_port,
    });
    publish_status(&app, |s| {
        s.qdrant_ready = true;
        s.qdrant_error = None;
        s.http_port = Some(http_port);
        s.grpc_port = Some(grpc_port);
    });
    info!("Qdrant initialization complete");
    Ok(())
}

pub async fn wait_for_readyz(http_port: u16) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{http_port}/readyz");
    const MAX_RETRIES: usize = 30;
    for attempt in 0..MAX_RETRIES {
        match reqwest::get(&url).await {
            Ok(resp) if resp.status().is_success() => {
                info!("Qdrant /readyz ok on port {http_port}");
                return Ok(());
            }
            Ok(resp) => {
                warn!(
                    "Qdrant /readyz status {} (attempt {}/{MAX_RETRIES})",
                    resp.status(),
                    attempt + 1
                );
            }
            Err(error) => {
                warn!(
                    "Qdrant /readyz not ready (attempt {}/{MAX_RETRIES}): {error}",
                    attempt + 1
                );
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(format!(
        "Qdrant /readyz failed after {MAX_RETRIES} attempts on port {http_port}"
    ))
}

pub async fn connect_grpc(grpc_port: u16) -> Result<Qdrant, String> {
    let url = format!("http://127.0.0.1:{grpc_port}");
    // Bundled sidecar forever — skip version negotiation noise.
    Qdrant::from_url(&url)
        .skip_compatibility_check()
        .build()
        .map_err(|error| format!("failed to build Qdrant client: {error}"))
}

pub async fn ensure_collections(client: &Qdrant) -> Result<(), String> {
    let existing: Vec<String> = client
        .list_collections()
        .await
        .map_err(|error| format!("failed to list collections: {error}"))?
        .collections
        .into_iter()
        .map(|collection| collection.name)
        .collect();
    info!("Qdrant collections found: {existing:?}");

    for name in COLLECTIONS {
        if existing.iter().any(|collection| collection == name) {
            info!("Collection '{name}' exists.");
            continue;
        }

        client
            .create_collection(hybrid_collection_builder(name, EMBEDDING_DIM))
            .await
            .map_err(|error| format!("failed to create collection '{name}': {error}"))?;
        info!("Created collection '{name}' with dimension {EMBEDDING_DIM} (sparse IDF)");
    }

    for (collection, field, field_type, is_tenant) in PAYLOAD_INDEXES {
        let info = client
            .collection_info(collection)
            .await
            .map_err(|error| format!("failed to inspect collection '{collection}': {error}"))?
            .result
            .ok_or_else(|| format!("collection '{collection}' returned no metadata"))?;

        if let Some(existing) = info.payload_schema.get(field) {
            let expected = payload_schema_type(field_type);
            if existing.data_type != expected as i32 {
                return Err(format!(
                    "payload index '{field}' on '{collection}' has type {:?}, expected {:?}",
                    PayloadSchemaType::try_from(existing.data_type).ok(),
                    expected
                ));
            }
            continue;
        }

        let mut builder =
            CreateFieldIndexCollectionBuilder::new(collection, field, field_type);
        if is_tenant && field_type == FieldType::Keyword {
            builder = builder.field_index_params(
                KeywordIndexParamsBuilder::default().is_tenant(true),
            );
        }
        client
            .create_field_index(builder)
            .await
            .map_err(|error| format!("failed to create index '{field}' on '{collection}': {error}"))?;
        info!("Created index '{field}' on '{collection}' (tenant={is_tenant})");
    }

    Ok(())
}

fn payload_schema_type(field_type: FieldType) -> PayloadSchemaType {
    match field_type {
        FieldType::Keyword => PayloadSchemaType::Keyword,
        FieldType::Integer => PayloadSchemaType::Integer,
        FieldType::Float => PayloadSchemaType::Float,
        FieldType::Geo => PayloadSchemaType::Geo,
        FieldType::Text => PayloadSchemaType::Text,
        FieldType::Bool => PayloadSchemaType::Bool,
        FieldType::Datetime => PayloadSchemaType::Datetime,
        FieldType::Uuid => PayloadSchemaType::Uuid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_index_types_match_qdrant_schema_types() {
        for (field_type, schema_type) in [
            (FieldType::Keyword, PayloadSchemaType::Keyword),
            (FieldType::Integer, PayloadSchemaType::Integer),
            (FieldType::Float, PayloadSchemaType::Float),
            (FieldType::Geo, PayloadSchemaType::Geo),
            (FieldType::Text, PayloadSchemaType::Text),
            (FieldType::Bool, PayloadSchemaType::Bool),
            (FieldType::Datetime, PayloadSchemaType::Datetime),
            (FieldType::Uuid, PayloadSchemaType::Uuid),
        ] {
            assert_eq!(payload_schema_type(field_type), schema_type);
        }
    }
}
