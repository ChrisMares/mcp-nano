use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use ort::session::Session;
use ort::inputs;
use tokenizers::Tokenizer;
use tracing::info;

use super::dense::{
    build_bert_input_tensors, build_session, clamp_text_for_tokenize,
    load_tokenizer_with_truncation, InferenceDevice, MAX_SEQ_LEN,
};

/// Cross-encoder reranker wrapping an ONNX BERT sequence-classification model.
///
/// The bundled model is `cross-encoder/ms-marco-MiniLM-L6-v2`. For each
/// (query, document) pair we tokenize `[CLS] query [SEP] document [SEP]`,
/// run the ONNX graph, and read the relevance score from `logits`. The
/// MiniLM-L6-v2 checkpoint is a single-output regression cross-encoder
/// (logits shape `[batch, 1]`); for 2-class checkpoints we take `logits[1]`.
///
/// Sequences are truncated to [`MAX_SEQ_LEN`] (512) — without this, long
/// codebase chunks (e.g. minified JS) allocate multi‑GB tensors and OOM.
pub struct Reranker {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    device: InferenceDevice,
}

impl Reranker {
    pub fn load(model_dir: &Path) -> Result<Self> {
        Self::load_with_device(model_dir, InferenceDevice::Cpu)
    }

    pub fn load_with_device(model_dir: &Path, device: InferenceDevice) -> Result<Self> {
        let weights = model_dir.join("model.onnx");
        let tokenizer_path = model_dir.join("tokenizer.json");

        let tokenizer = load_tokenizer_with_truncation(&tokenizer_path)?;

        let session = build_session(&weights, device)
            .with_context(|| format!("loading ONNX session {}", weights.display()))?;

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            device,
        })
    }

    pub fn device(&self) -> InferenceDevice {
        self.device
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
        let query_clipped = clamp_text_for_tokenize(query);
        let pair_texts: Vec<String> = documents
            .iter()
            .map(|d| clamp_text_for_tokenize(d))
            .collect();
        let total_batches = pair_texts.len().div_ceil(batch_size.max(1));
        info!(
            "reranker start device={} docs={} batch_size={} batches={}",
            self.device.label(),
            documents.len(),
            batch_size.max(1),
            total_batches
        );

        let mut scores = Vec::with_capacity(documents.len());
        let mut batch_idx = 0usize;
        for chunk in pair_texts.chunks(batch_size.max(1)) {
            batch_idx += 1;
            let tok_start = Instant::now();
            let mut encodings = Vec::with_capacity(chunk.len());
            for doc in chunk {
                let pair = self
                    .tokenizer
                    .encode((query_clipped.as_str(), doc.as_str()), true)
                    .map_err(|e| anyhow!("tokenizer encode_pair failed: {e}"))?;
                encodings.push(pair);
            }
            let seq_len = encodings
                .iter()
                .map(|e| e.get_ids().len().min(MAX_SEQ_LEN))
                .max()
                .unwrap_or(0)
                .min(MAX_SEQ_LEN);
            if seq_len == 0 {
                scores.extend(std::iter::repeat_n(0.0, chunk.len()));
                continue;
            }
            let batch = encodings.len();
            let (input_ids, attention_mask, token_type_ids, _) =
                build_bert_input_tensors(&encodings, seq_len)?;
            let tokenize_ms = tok_start.elapsed().as_millis();

            let mut session = self
                .session
                .lock()
                .map_err(|_| anyhow!("reranker session lock poisoned"))?;
            let run_start = Instant::now();
            let outputs = session.run(inputs! {
                "input_ids" => input_ids,
                "attention_mask" => attention_mask,
                "token_type_ids" => token_type_ids,
            })?;
            info!(
                "reranker ORT run device={} batch={batch_idx}/{total_batches} size={batch} seq_len={seq_len} tokenize_ms={tokenize_ms} run_ms={}",
                self.device.label(),
                run_start.elapsed().as_millis()
            );

            let (out_shape, logits) = outputs["logits"].try_extract_tensor::<f32>()?;
            let num_labels = if out_shape.len() >= 2 {
                out_shape[1] as usize
            } else {
                1
            }
            .max(1);

            for b in 0..batch {
                let base = b * num_labels;
                let score = if num_labels == 1 {
                    logits[base]
                } else {
                    logits.get(base + 1).copied().unwrap_or(logits[base])
                };
                scores.push(score);
            }
        }
        Ok(scores)
    }
}
