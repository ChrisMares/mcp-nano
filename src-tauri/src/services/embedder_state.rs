use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::info;

use crate::services::embedders::dense::InferenceDevice;
use crate::services::embedders::{Bm25Embedder, DenseEmbedder, Reranker};
use crate::services::embedders::EncodeQuery;

/// Tauri-managed state holding ONNX embedder + reranker sessions.
///
/// Sessions are loaded once at startup. Register via
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

        let (dense, reranker, device_mode) = match Self::load_models(models_dir, device) {
            Ok((dense, reranker)) if device.is_gpu() => {
                if let Err(error) = dense.encode_query("GPU capability check") {
                    tracing::warn!(
                        "{} embedding cannot run this model; using CPU: {error}",
                        device.label()
                    );
                    let (dense, reranker) = Self::load_models(models_dir, InferenceDevice::Cpu)?;
                    (dense, reranker, device.fallback_label().to_string())
                } else {
                    (dense, reranker, device_mode)
                }
            }
            Ok((dense, reranker)) => (dense, reranker, device_mode),
            Err(error) if device.is_gpu() => {
                tracing::warn!(
                    "{} embedding unavailable; using CPU: {error:#}",
                    device.label()
                );
                let (dense, reranker) = Self::load_models(models_dir, InferenceDevice::Cpu)?;
                (dense, reranker, device.fallback_label().to_string())
            }
            Err(error) => return Err(error),
        };

        Ok(Self {
            dense,
            reranker,
            bm25: Bm25Embedder::new(),
            device_mode,
        })
    }

    fn load_models(
        models_dir: &Path,
        device: InferenceDevice,
    ) -> Result<(DenseEmbedder, Reranker)> {
        let dense = DenseEmbedder::load_with_device(
            &models_dir.join("arctic-embed-xs"),
            device,
        )
        .context("loading dense embedder")?;
        let reranker = Reranker::load_with_device(&models_dir.join("minilm-l6-v2"), device)
            .context("loading reranker")?;
        Ok((dense, reranker))
    }

    pub fn device_mode(&self) -> &str {
        &self.device_mode
    }

    fn embedding_device() -> (InferenceDevice, String) {
        if std::env::var("MCP_NANO_DEVICE")
            .ok()
            .is_some_and(|value| value.eq_ignore_ascii_case("cpu"))
        {
            info!("Embedding device: CPU (requested by MCP_NANO_DEVICE)");
            return (InferenceDevice::Cpu, InferenceDevice::Cpu.label().to_string());
        }

        #[cfg(all(feature = "directml", target_os = "windows"))]
        {
            info!("Embedding device: DirectML (GPU)");
            return (
                InferenceDevice::DirectMl,
                InferenceDevice::DirectMl.label().to_string(),
            );
        }

        #[cfg(all(feature = "cuda", target_os = "linux"))]
        {
            info!("Embedding device: CUDA (GPU 0)");
            return (
                InferenceDevice::Cuda,
                InferenceDevice::Cuda.label().to_string(),
            );
        }

        #[cfg(not(any(
            all(feature = "directml", target_os = "windows"),
            all(feature = "cuda", target_os = "linux"),
        )))]
        {
            info!("Embedding device: CPU (no GPU EP enabled for this OS/build)");
            (InferenceDevice::Cpu, InferenceDevice::Cpu.label().to_string())
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
