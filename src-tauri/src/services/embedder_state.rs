use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use candle_core::Device;
use tracing::info;

use crate::services::embedders::{Bm25Embedder, DenseEmbedder, Reranker};
use crate::services::embedders::EncodeQuery;

/// Tauri-managed state holding mmap'd embedder + reranker models.
///
/// Heavy tensors are mmap'd once at startup. Register via
/// `app.manage(Arc::new(state))` so controllers and worker tasks can share
/// a single cheap reference (`State<'_, Arc<EmbedderState>>`).
pub struct EmbedderState {
    pub dense: DenseEmbedder,
    pub reranker: Reranker,
    pub bm25: Bm25Embedder,
    device_mode: String,
}

impl EmbedderState {
    /// Load all three embedders from `resources/models/{arctic-embed-xs,minilm-l6-v2}`.
    pub fn load(models_dir: &Path) -> Result<Self> {
        let (device, device_mode) = Self::embedding_device();
        let (dense, reranker) = Self::load_models(models_dir, device.clone())?;

        if device.is_cuda() {
            if let Err(error) = dense.encode_query("CUDA capability check") {
                tracing::warn!("CUDA embedding cannot run this model; using CPU: {error}");
                let (dense, reranker) = Self::load_models(models_dir, Device::Cpu)?;
                return Ok(Self {
                    dense,
                    reranker,
                    bm25: Bm25Embedder::new(),
                    device_mode: "CPU (CUDA fallback)".to_string(),
                });
            }
        }

        let bm25 = Bm25Embedder::new();
        Ok(Self {
            dense,
            reranker,
            bm25,
            device_mode,
        })
    }

    fn load_models(models_dir: &Path, device: Device) -> Result<(DenseEmbedder, Reranker)> {
        let dense = DenseEmbedder::load_with_device(
            &models_dir.join("arctic-embed-xs"),
            device.clone(),
        )
        .context("loading dense embedder")?;
        let reranker = Reranker::load_with_device(&models_dir.join("minilm-l6-v2"), device)
            .context("loading reranker")?;
        Ok((dense, reranker))
    }

    pub fn device_mode(&self) -> &str {
        &self.device_mode
    }

    fn embedding_device() -> (Device, String) {
        if std::env::var("MCP_NANO_DEVICE")
            .ok()
            .is_some_and(|value| value.eq_ignore_ascii_case("cpu"))
        {
            info!("Embedding device: CPU (requested by MCP_NANO_DEVICE)");
            return (Device::Cpu, "CPU".to_string());
        }

        #[cfg(all(feature = "cuda", any(target_os = "linux", target_os = "windows")))]
        match Device::new_cuda(0) {
            Ok(device) => {
                info!("Embedding device: CUDA (GPU 0)");
                (device, "CUDA (GPU)".to_string())
            }
            Err(error) => {
                tracing::warn!("CUDA embedding unavailable; using CPU: {error}");
                (Device::Cpu, "CPU".to_string())
            }
        }

        #[cfg(not(all(feature = "cuda", any(target_os = "linux", target_os = "windows"))))]
        {
            info!("Embedding device: CPU (CUDA support was not included in this build)");
            (Device::Cpu, "CPU".to_string())
        }
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
