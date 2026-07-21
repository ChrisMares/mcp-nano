use std::path::Path;

use anyhow::{anyhow, Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::Tokenizer;

use super::traits::{EncodeDocuments, EncodeQuery};

/// Dense text embedder wrapping a Candle BERT model.
///
/// Loads `model.safetensors` via mmap and `tokenizer.json` for tokenization.
/// Forward pass produces token-level hidden states; we mean-pool over the
/// non-pad tokens (mask-aware) to get one vector per input.
///
/// The bundled model is `Snowflake/snowflake-arctic-embed-xs` (384-dim, 22M
/// params). The implementation is generic over any BERT-base-architecture
/// embedding model whose `config.json` parses into
/// `candle_transformers::models::bert::Config`.
pub struct DenseEmbedder {
    model: BertModel,
    tokenizer: Tokenizer,
    dim: usize,
    /// Optional instruction prepended to queries (Snowflake/arctic-embed
    /// convention). Empty for models that don't use query prefixes.
    query_instruction: String,
    device: Device,
}

impl DenseEmbedder {
    /// Load the dense embedder from a directory containing `model.safetensors`,
    /// `tokenizer.json`, and `config.json`.
    pub fn load(model_dir: &Path) -> Result<Self> {
        Self::load_with_device(model_dir, Device::Cpu)
    }

    pub fn load_with_device(model_dir: &Path, device: Device) -> Result<Self> {
        let weights = model_dir.join("model.safetensors");
        let tokenizer_path = model_dir.join("tokenizer.json");
        let config_path = model_dir.join("config.json");

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow!("failed to load tokenizer {}: {e}", tokenizer_path.display()))?;

        let config_text = std::fs::read_to_string(&config_path)
            .with_context(|| format!("reading {}", config_path.display()))?;
        let mut config: Config = serde_json::from_str(&config_text)
            .with_context(|| format!("parsing {}", config_path.display()))?;
        // arctic-embed-xs stores model_type="bert" which is already the default
        // in candle's Config; ensure the field is set so the fallback loader
        // path doesn't trip.
        if config.model_type.is_none() {
            config.model_type = Some("bert".to_string());
        }

        // Safety: memmap of a bundled, trusted safetensors file. The file is
        // shipped by us via scripts/download-models.sh and is not modified at
        // runtime.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[&weights], DType::F32, &device)
                .with_context(|| format!("mmap {}", weights.display()))?
        };
        let model = BertModel::load(vb, &config).context("building BERT model from weights")?;

        let dim = config.hidden_size;
        // Snowflake/arctic-embed-xs does not require a query prefix per its
        // model card. The field is kept as an extension point for models that
        // do (e.g. some BGE/E5 variants prefix queries with "query: ").
        let query_instruction = String::new();

        Ok(Self {
            model,
            tokenizer,
            dim,
            query_instruction,
            device,
        })
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    fn encode_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        // Encode with padding so all sequences in the batch have equal length.
        // HF tokenizers returns input_ids + attention_mask + token_type_ids.
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow!("tokenizer encode failed: {e}"))?;

        let seq_len = encodings[0].get_ids().len();
        let batch = encodings.len();

        let mut input_ids = Vec::with_capacity(batch * seq_len);
        let mut attention_mask = Vec::with_capacity(batch * seq_len);
        let mut token_type_ids = Vec::with_capacity(batch * seq_len);
        for enc in &encodings {
            let ids = enc.get_ids();
            let mask = enc.get_attention_mask();
            let type_ids = enc.get_type_ids();
            // Defensive: pad/truncate to seq_len so the tensor is rectangular.
            for i in 0..seq_len {
                input_ids.push(if i < ids.len() { ids[i] as i64 } else { 0 });
                attention_mask.push(if i < mask.len() { mask[i] as i64 } else { 0 });
                token_type_ids.push(if i < type_ids.len() { type_ids[i] as i64 } else { 0 });
            }
        }

        let input_ids = Tensor::from_vec(input_ids, (batch, seq_len), &self.device)?;
        let attention_mask = Tensor::from_vec(attention_mask, (batch, seq_len), &self.device)?;
        let token_type_ids = Tensor::from_vec(token_type_ids, (batch, seq_len), &self.device)?;

        // (batch, seq_len, hidden)
        let hidden = self
            .model
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))?;

        // Mask-aware mean pool over seq_len.
        // mask_f: (batch, seq_len, 1)
        let mask_f = attention_mask.to_dtype(DType::F32)?.unsqueeze(2)?;
        // weighted: (batch, seq_len, hidden) = hidden * mask_f
        let weighted = hidden.broadcast_mul(&mask_f)?;
        // sum over seq: (batch, hidden)
        let summed = weighted.sum(1)?;
        // counts: (batch, 1)
        let counts = mask_f.sum(1)?;
        // pooled: (batch, hidden)
        let pooled = summed.broadcast_div(&counts)?;

        let pooled_vec: Vec<Vec<f32>> = pooled.to_vec2()?;
        Ok(pooled_vec)
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
