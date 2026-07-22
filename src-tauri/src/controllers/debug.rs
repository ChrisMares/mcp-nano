//! Shared scenario helpers for integration tests.
//!
//! Gated by `cfg(debug_assertions)` so this module is stripped from release
//! builds. The `tests/` folder (separate integration-test crates) imports
//! these via `tests/common/mod.rs`:
//!
//! ```ignore
//! mod common;
//! use common::{spawn_qdrant, load_embedders, ...};
//! ```
//!
//! Keeping a single source of truth here (rather than copy-pasting helpers
//! into each test file) follows the Rust By Example recommendation:
//! <https://doc.rust-lang.org/rust-by-example/testing/integration_testing.html>

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use qdrant_client::Qdrant;
use sqlx::SqlitePool;

use crate::services::embedder_state::EmbedderState;
use crate::services::embedders::{DenseEmbedder, Reranker};

/// Resolve `src-tauri/resources/models/` from `CARGO_MANIFEST_DIR`.
pub fn models_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/models")
}

/// Locate the bundled qdrant binary for the current target triple.
/// Returns `None` if the binary isn't present (skips the test).
pub fn qdrant_binary() -> Option<PathBuf> {
    let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries/qdrant-x86_64-unknown-linux-gnu");
    if bin.exists() {
        Some(bin)
    } else {
        None
    }
}

/// Kills the spawned qdrant child process when dropped.
pub struct ChildGuard {
    child: std::process::Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Spawn qdrant on a free port with storage in a tempdir. Returns
/// `(Qdrant client, ChildGuard)` — the child is killed when the guard drops.
/// Returns `None` if the binary is missing or the server fails to come up.
pub async fn spawn_qdrant() -> Option<(Qdrant, ChildGuard)> {
    let bin = qdrant_binary()?;
    let storage = tempfile::tempdir().ok()?;
    let storage_path = storage.path().to_path_buf();
    // Leak the tempdir so it survives the test (storage_path lives for the
    // test's lifetime; we clean up Qdrant data by killing the child).
    std::mem::forget(storage);

    let http_port = portpicker::pick_unused_port()?;
    let grpc_port = portpicker::pick_unused_port()?;
    let child = std::process::Command::new(&bin)
        .env("QDRANT__SERVICE__HOST", "127.0.0.1")
        .env("QDRANT__SERVICE__HTTP_PORT", http_port.to_string())
        .env("QDRANT__SERVICE__GRPC_PORT", grpc_port.to_string())
        .env(
            "QDRANT__STORAGE__STORAGE_PATH",
            storage_path.to_string_lossy().to_string(),
        )
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let health_url = format!("http://127.0.0.1:{http_port}/healthz");
    for _ in 0..40 {
        if reqwest::get(&health_url).await.is_ok() {
            let client = Qdrant::from_url(&format!("http://127.0.0.1:{grpc_port}"))
                .build()
                .ok()?;
            return Some((client, ChildGuard { child }));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    None
}

/// Open a temp SQLite pool and run migrations. Used by worker E2E tests.
pub async fn open_sqlite_pool(dir: &Path) -> SqlitePool {
    let path = dir.join("app.db");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .expect("connect pool");
    sqlx::migrate!("./migrations").run(&pool).await.expect("migrate");
    pool
}

/// Load the full `EmbedderState` (dense + reranker + BM25) from
/// `resources/models/`. Returns `None` (skip-safe) if the model files
/// are absent — caller should print a skip message and return.
pub fn load_embedders() -> Option<Arc<EmbedderState>> {
    let dir = models_dir();
    if !dir.join("arctic-embed-xs/model.safetensors").exists() {
        tracing::warn!("skipping: arctic-embed-xs not downloaded; run scripts/download-models.sh");
        return None;
    }
    if !dir.join("minilm-l6-v2/model.safetensors").exists() {
        tracing::warn!("skipping: minilm-l6-v2 not downloaded; run scripts/download-models.sh");
        return None;
    }
    Some(Arc::new(
        EmbedderState::load(&dir).expect("load embedders"),
    ))
}

/// Skip-safe dense embedder loader. Returns `None` if arctic-embed-xs
/// isn't downloaded.
pub fn dense_ready() -> Option<DenseEmbedder> {
    let dir = models_dir().join("arctic-embed-xs");
    let weights = dir.join("model.safetensors");
    let tokenizer = dir.join("tokenizer.json");
    if !weights.exists() || !tokenizer.exists() {
        return None;
    }
    DenseEmbedder::load(&dir).ok()
}

/// Skip-safe reranker loader. Returns `None` if minilm-l6-v2
/// isn't downloaded.
pub fn reranker_ready() -> Option<Reranker> {
    let dir = models_dir().join("minilm-l6-v2");
    let weights = dir.join("model.safetensors");
    let tokenizer = dir.join("tokenizer.json");
    if !weights.exists() || !tokenizer.exists() {
        return None;
    }
    match Reranker::load(&dir) {
        Ok(r) => Some(r),
        Err(e) => {
            tracing::warn!("Reranker::load failed: {e:#}");
            None
        }
    }
}

/// Cosine similarity between two equal-length vectors.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot = a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// Create a Qdrant collection configured for hybrid (dense + sparse IDF) vectors.
pub async fn create_test_collection(
    client: &Qdrant,
    collection: &str,
    dense_dim: usize,
) -> Result<()> {
    client
        .create_collection(crate::qdrant::hybrid_collection_builder(
            collection,
            dense_dim as u64,
        ))
        .await
        .context("create_collection")?;
    Ok(())
}
