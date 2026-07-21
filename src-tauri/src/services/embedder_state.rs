use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::services::embedders::{Bm25Embedder, DenseEmbedder, Reranker};

/// Tauri-managed state holding mmap'd embedder + reranker models.
///
/// Heavy tensors are mmap'd once at startup. Register via
/// `app.manage(Arc::new(state))` so controllers and worker tasks can share
/// a single cheap reference (`State<'_, Arc<EmbedderState>>`).
pub struct EmbedderState {
    pub dense: DenseEmbedder,
    pub reranker: Reranker,
    pub bm25: Bm25Embedder,
}

impl EmbedderState {
    /// Load all three embedders from `resources/models/{arctic-embed-xs,minilm-l6-v2}`.
    pub fn load(models_dir: &Path) -> Result<Self> {
        let dense = DenseEmbedder::load(&models_dir.join("arctic-embed-xs"))
            .context("loading dense embedder")?;
        let reranker = Reranker::load(&models_dir.join("minilm-l6-v2"))
            .context("loading reranker")?;
        let bm25 = Bm25Embedder::new();
        Ok(Self {
            dense,
            reranker,
            bm25,
        })
    }

    /// Resolve the models directory from the app environment.
    /// In dev (`cfg!(debug_assertions)`) reads from the repo's
    /// `src-tauri/resources/models/`; in release uses Tauri's
    /// `resource_dir()/models/`.
    pub fn models_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
        if cfg!(debug_assertions) {
            Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/models"))
        } else {
            use tauri::Manager;
            app.path()
                .resource_dir()
                .map(|d| d.join("models"))
                .map_err(|e| format!("failed to resolve resource dir: {e}"))
        }
    }
}
