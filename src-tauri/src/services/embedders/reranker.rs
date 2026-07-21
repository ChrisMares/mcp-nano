use std::path::Path;

use anyhow::{anyhow, Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::{linear, Linear, Module, VarBuilder};
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::Tokenizer;

/// Cross-encoder reranker wrapping a Candle BERT + sequence-classification
/// head.
///
/// The bundled model is `cross-encoder/ms-marco-MiniLM-L6-v2`. For each
/// (query, document) pair we tokenize `[CLS] query [SEP] document [SEP]`,
/// run the BERT encoder, take the `[CLS]` (index 0) hidden state, apply the
/// linear classifier head, and read the relevance score. The MiniLM-L6-v2
/// checkpoint is a single-output regression cross-encoder (classifier shape
/// `[1, hidden]`), so the one logit IS the score; for 2-class checkpoints
/// we'd take `logits[1]` (relevant class).
pub struct Reranker {
    encoder: BertModel,
    classifier: Linear,
    tokenizer: Tokenizer,
    device: Device,
}

impl Reranker {
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
        if config.model_type.is_none() {
            config.model_type = Some("bert".to_string());
        }
        // cross-encoder/ms-marco-MiniLM-L6-v2 is a single-output regression
        // cross-encoder: classifier shape is [1, hidden], and the single logit
        // IS the relevance score (no softmax). Determine num_labels from
        // `id2label` (falling back to 2 for canonical 2-class checkpoints).
        let extras: ConfigExtras = serde_json::from_str(&config_text).unwrap_or_default();
        let num_labels = extras.num_labels();

        // Safety: memmap of a bundled, trusted safetensors file.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[&weights], DType::F32, &device)
                .with_context(|| format!("mmap {}", weights.display()))?
        };

        // Cross-encoder checkpoints typically store the encoder under
        // `bert.*`. `BertModel::load` already has a fallback that retries
        // under `{model_type}.embeddings` / `{model_type}.encoder` if the
        // flat path fails, so passing the root VarBuilder handles both
        // layouts (top-level weights and `bert.`-prefixed weights).
        let encoder = BertModel::load(vb.clone(), &config).context("loading BERT encoder")?;

        // Classifier head: Linear(hidden_size, num_labels). Cross-encoder
        // safetensors store this as `classifier.weight` (and optionally
        // `classifier.bias`). Load the tensors directly when present so we
        // use the trained head; otherwise fall back to a fresh linear
        // (random init — only happens for non-canonical checkpoints).
        let classifier = match (
            vb.get((num_labels, config.hidden_size), "classifier.weight"),
            vb.get((num_labels,), "classifier.bias"),
        ) {
            (Ok(weight), Ok(bias)) => Linear::new(weight, Some(bias)),
            (Ok(weight), Err(_)) => Linear::new(weight, None),
            (Err(_), _) => linear(config.hidden_size, num_labels, vb.pp("classifier"))?,
        };

        Ok(Self {
            encoder,
            classifier,
            tokenizer,
            device,
        })
    }

    /// Score each (query, document) pair. Returns one f32 per document,
    /// higher = more relevant. `batch_size` controls how many pairs are
    /// forwarded through the encoder at once.
    pub fn rerank(
        &self,
        query: &str,
        documents: &[&str],
        batch_size: usize,
    ) -> Result<Vec<f32>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        // Build pair texts: the tokenizer handles [CLS]/[SEP] insertion.
        // HF BERT tokenizers encode_pair with truncation produces the right
        // structure. We pass truncation=true so long docs don't overflow.
        let pair_texts: Vec<String> = documents.iter().map(|d| d.to_string()).collect();

        let mut scores = Vec::with_capacity(documents.len());
        for chunk in pair_texts.chunks(batch_size.max(1)) {
            // Encode each (query, doc) pair as a single sequence.
            let mut encodings = Vec::with_capacity(chunk.len());
            for doc in chunk {
                let pair = self
                    .tokenizer
                    .encode((query, doc.as_str()), true)
                    .map_err(|e| anyhow!("tokenizer encode_pair failed: {e}"))?;
                encodings.push(pair);
            }
            // Pad manually to the max length in this chunk.
            let seq_len = encodings.iter().map(|e| e.get_ids().len()).max().unwrap_or(0);
            if seq_len == 0 {
                scores.extend(std::iter::repeat_n(0.0, chunk.len()));
                continue;
            }
            let batch = encodings.len();
            let mut input_ids = Vec::with_capacity(batch * seq_len);
            let mut attention_mask = Vec::with_capacity(batch * seq_len);
            let mut token_type_ids = Vec::with_capacity(batch * seq_len);
            for enc in &encodings {
                let ids = enc.get_ids();
                let mask = enc.get_attention_mask();
                let type_ids = enc.get_type_ids();
                for i in 0..seq_len {
                    input_ids.push(if i < ids.len() { ids[i] as i64 } else { 0 });
                    attention_mask.push(if i < mask.len() { mask[i] as i64 } else { 0 });
                    token_type_ids.push(if i < type_ids.len() { type_ids[i] as i64 } else { 0 });
                }
            }
            let input_ids = Tensor::from_vec(input_ids, (batch, seq_len), &self.device)?;
            let attention_mask =
                Tensor::from_vec(attention_mask, (batch, seq_len), &self.device)?;
            let token_type_ids =
                Tensor::from_vec(token_type_ids, (batch, seq_len), &self.device)?;

            // (batch, seq_len, hidden)
            let hidden = self
                .encoder
                .forward(&input_ids, &token_type_ids, Some(&attention_mask))?;

            // [CLS] is at index 0 of each sequence. narrow + squeeze keeps
            // the batch dimension: (batch, seq_len, hidden) -> (batch, hidden).
            let cls = hidden.narrow(1, 0, 1)?.squeeze(1)?;
            let logits = self.classifier.forward(&cls)?; // (batch, num_labels)
            let logits_vec: Vec<Vec<f32>> = logits.to_vec2()?;
            for row in logits_vec {
                // Single-output regression cross-encoder: the one logit IS
                // the score. Two-class classifier: take logits[1] (relevant).
                let score = if row.len() == 1 {
                    row[0]
                } else {
                    row.get(1).copied().unwrap_or(row[0])
                };
                scores.push(score);
            }
        }
        Ok(scores)
    }
}

#[derive(serde::Deserialize, Default)]
struct ConfigExtras {
    num_labels: Option<usize>,
    id2label: Option<serde_json::Map<String, serde_json::Value>>,
}

impl ConfigExtras {
    /// Resolve the classifier output dimension: explicit `num_labels` field,
    /// else `id2label.len()`, else default 2 (canonical BERT 2-class).
    fn num_labels(&self) -> usize {
        if let Some(n) = self.num_labels {
            return n;
        }
        if let Some(map) = &self.id2label {
            return map.len().max(1);
        }
        2
    }
}
