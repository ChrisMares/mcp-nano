use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;
use ort::inputs;
use tokenizers::{Tokenizer, TruncationParams, TruncationStrategy};
use tracing::info;

use super::traits::{EncodeDocuments, EncodeQuery};

/// BERT / MiniLM positional limit used by the bundled ONNX models.
pub(crate) const MAX_SEQ_LEN: usize = 512;
/// Cap text fed to the HF tokenizer. ~4 chars/token → plenty for 512 tokens.
/// Prevents multi‑MB source maps / minified blobs from burning seconds on CPU.
pub(crate) const MAX_TOKENIZE_CHARS: usize = 4_000;

pub(crate) fn clamp_text_for_tokenize(text: &str) -> String {
    if text.chars().count() <= MAX_TOKENIZE_CHARS {
        return text.to_string();
    }
    text.chars().take(MAX_TOKENIZE_CHARS).collect()
}

/// Dense text embedder wrapping an ONNX BERT model via ONNX Runtime.
///
/// Loads `model.onnx` and `tokenizer.json`. Forward pass produces token-level
/// hidden states (`last_hidden_state`); we mean-pool over non-pad tokens
/// (mask-aware) to get one vector per input.
///
/// The bundled model is `Snowflake/snowflake-arctic-embed-xs` (384-dim, 22M
/// params) exported as ONNX.
pub struct DenseEmbedder {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    dim: usize,
    device: InferenceDevice,
    /// Optional instruction prepended to queries (Snowflake/arctic-embed
    /// convention). Empty for models that don't use query prefixes.
    query_instruction: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InferenceDevice {
    Cpu,
    Cuda,
    DirectMl,
}

impl InferenceDevice {
    pub fn is_gpu(self) -> bool {
        matches!(self, Self::Cuda | Self::DirectMl)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Cuda => "CUDA (GPU)",
            Self::DirectMl => "DirectML (GPU)",
        }
    }

    pub fn fallback_label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Cuda => "CPU (CUDA fallback)",
            Self::DirectMl => "CPU (DirectML fallback)",
        }
    }
}

impl DenseEmbedder {
    /// Load the dense embedder from a directory containing `model.onnx` and
    /// `tokenizer.json`.
    pub fn load(model_dir: &Path) -> Result<Self> {
        Self::load_with_device(model_dir, InferenceDevice::Cpu)
    }

    pub fn load_with_device(model_dir: &Path, device: InferenceDevice) -> Result<Self> {
        let weights = model_dir.join("model.onnx");
        let tokenizer_path = model_dir.join("tokenizer.json");
        let config_path = model_dir.join("config.json");

        let tokenizer = load_tokenizer_with_truncation(&tokenizer_path)?;

        let dim = read_hidden_size(&config_path).unwrap_or(384);

        let session = build_session(&weights, device)
            .with_context(|| format!("loading ONNX session {}", weights.display()))?;

        // Snowflake/arctic-embed-xs does not require a query prefix per its
        // model card. The field is kept as an extension point for models that
        // do (e.g. some BGE/E5 variants prefix queries with "query: ").
        let query_instruction = String::new();

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            dim,
            device,
            query_instruction,
        })
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn device(&self) -> InferenceDevice {
        self.device
    }

    fn encode_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let clipped: Vec<String> = texts.iter().map(|t| clamp_text_for_tokenize(t)).collect();
        let encodings = self
            .tokenizer
            .encode_batch(clipped, true)
            .map_err(|e| anyhow!("tokenizer encode failed: {e}"))?;

        let seq_len = encodings
            .iter()
            .map(|e| e.get_ids().len().min(MAX_SEQ_LEN))
            .max()
            .unwrap_or(0)
            .min(MAX_SEQ_LEN)
            .max(1);
        let batch = encodings.len();

        let mut input_ids = Vec::with_capacity(batch * seq_len);
        let mut attention_mask = Vec::with_capacity(batch * seq_len);
        let mut token_type_ids = Vec::with_capacity(batch * seq_len);
        for enc in &encodings {
            let ids = enc.get_ids();
            let mask = enc.get_attention_mask();
            let type_ids = enc.get_type_ids();
            let n = ids.len().min(seq_len);
            for i in 0..seq_len {
                if i < n {
                    input_ids.push(ids[i] as i64);
                    attention_mask.push(if i < mask.len() { mask[i] as i64 } else { 1 });
                    token_type_ids.push(if i < type_ids.len() {
                        type_ids[i] as i64
                    } else {
                        0
                    });
                } else {
                    input_ids.push(0);
                    attention_mask.push(0);
                    token_type_ids.push(0);
                }
            }
        }

        let shape = vec![batch as i64, seq_len as i64];
        let input_ids = Tensor::from_array((shape.clone(), input_ids))?;
        let attention_mask_t = Tensor::from_array((shape.clone(), attention_mask.clone()))?;
        let token_type_ids = Tensor::from_array((shape, token_type_ids))?;

        let mut session = self
            .session
            .lock()
            .map_err(|_| anyhow!("dense embedder session lock poisoned"))?;
        let run_start = Instant::now();
        let outputs = session.run(inputs! {
            "input_ids" => input_ids,
            "attention_mask" => attention_mask_t,
            "token_type_ids" => token_type_ids,
        })?;
        info!(
            "dense ORT run device={} batch={batch} seq_len={seq_len} elapsed_ms={}",
            self.device.label(),
            run_start.elapsed().as_millis()
        );

        let (out_shape, hidden) = outputs["last_hidden_state"].try_extract_tensor::<f32>()?;
        let hidden_dim = *out_shape.last().unwrap_or(&(self.dim as i64)) as usize;
        if hidden_dim == 0 {
            return Err(anyhow!("last_hidden_state has zero hidden dimension"));
        }

        // Mask-aware mean pool over seq_len.
        let mut pooled = Vec::with_capacity(batch);
        for b in 0..batch {
            let mut acc = vec![0.0f32; hidden_dim];
            let mut count = 0.0f32;
            for s in 0..seq_len {
                let m = attention_mask[b * seq_len + s] as f32;
                if m == 0.0 {
                    continue;
                }
                count += m;
                let base = (b * seq_len + s) * hidden_dim;
                for h in 0..hidden_dim {
                    acc[h] += hidden[base + h] * m;
                }
            }
            if count > 0.0 {
                for v in &mut acc {
                    *v /= count;
                }
            }
            pooled.push(acc);
        }
        Ok(pooled)
    }

    fn encode(&self, texts: &[String], batch_size: usize) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(batch_size.max(1)) {
            out.extend(self.encode_batch(chunk)?);
        }
        Ok(out)
    }
}

impl EncodeQuery for DenseEmbedder {
    fn encode_query(&self, query: &str) -> Result<Vec<f32>> {
        let text = if self.query_instruction.is_empty() {
            query.to_string()
        } else {
            format!("{}{}", self.query_instruction, query)
        };
        let mut vecs = self.encode(&[text], 1)?;
        Ok(vecs.remove(0))
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

impl EncodeDocuments for DenseEmbedder {
    fn encode_documents(&self, documents: &[&str], batch_size: usize) -> Result<Vec<Vec<f32>>> {
        let owned: Vec<String> = documents.iter().map(|s| s.to_string()).collect();
        self.encode(&owned, batch_size)
    }
}

pub(crate) fn build_session(model_path: &Path, device: InferenceDevice) -> Result<Session> {
    // Use most of the machine for CPU EP; GPU EP ignores this for the heavy
    // kernels but still benefits tokenizer/pre/post on the host.
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 24);

    let map_ort = |err: ort::Error| anyhow!("onnx runtime error: {err}");

    let builder = Session::builder()
        .map_err(map_ort)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| anyhow!("setting graph optimization level: {e}"))?
        .with_intra_threads(threads)
        .map_err(|e| anyhow!("setting intra threads: {e}"))?;

    let mut builder = match device {
        InferenceDevice::Cuda => {
            #[cfg(all(feature = "cuda", target_os = "linux"))]
            {
                builder
                    .with_execution_providers([
                        ort::ep::CUDA::default().build().error_on_failure(),
                    ])
                    .map_err(|e| anyhow!("registering CUDA execution provider: {e}"))?
            }
            #[cfg(not(all(feature = "cuda", target_os = "linux")))]
            {
                let _ = builder;
                return Err(anyhow!(
                    "CUDA requested but this build does not include the ORT CUDA EP (Linux only)"
                ));
            }
        }
        InferenceDevice::DirectMl => {
            #[cfg(all(feature = "directml", target_os = "windows"))]
            {
                builder
                    .with_execution_providers([
                        ort::ep::DirectML::default().build().error_on_failure(),
                    ])
                    .map_err(|e| anyhow!("registering DirectML execution provider: {e}"))?
            }
            #[cfg(not(all(feature = "directml", target_os = "windows")))]
            {
                let _ = builder;
                return Err(anyhow!(
                    "DirectML requested but this build does not include the ORT DirectML EP (Windows only)"
                ));
            }
        }
        InferenceDevice::Cpu => builder,
    };

    builder
        .commit_from_file(model_path)
        .map_err(|e| anyhow!("commit ONNX model {}: {e}", model_path.display()))
}

fn read_hidden_size(config_path: &Path) -> Option<usize> {
    let text = std::fs::read_to_string(config_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("hidden_size")?.as_u64().map(|n| n as usize)
}

pub(crate) fn load_tokenizer_with_truncation(tokenizer_path: &Path) -> Result<Tokenizer> {
    let mut tokenizer = Tokenizer::from_file(tokenizer_path)
        .map_err(|e| anyhow!("failed to load tokenizer {}: {e}", tokenizer_path.display()))?;
    tokenizer
        .with_truncation(Some(TruncationParams {
            max_length: MAX_SEQ_LEN,
            strategy: TruncationStrategy::LongestFirst,
            stride: 0,
            ..Default::default()
        }))
        .map_err(|e| anyhow!("failed to set tokenizer truncation: {e}"))?;
    Ok(tokenizer)
}
