use anyhow::Result;

/// A sparse vector: parallel index/value arrays (Qdrant sparse-vector format).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SparseVector {
    pub indices: Vec<u32>,
    pub values: Vec<f32>,
}

impl SparseVector {
    pub fn new(indices: Vec<u32>, values: Vec<f32>) -> Self {
        debug_assert_eq!(
            indices.len(),
            values.len(),
            "sparse indices and values must have equal length"
        );
        Self { indices, values }
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Convert to the `(u32, f32)` tuple form the qdrant-client accepts for
    /// `Vector::from(Vec<(u32, f32)>)`.
    pub fn to_tuples(&self) -> Vec<(u32, f32)> {
        self.indices
            .iter()
            .copied()
            .zip(self.values.iter().copied())
            .collect()
    }
}

/// Embed a single user query into a dense vector.
///
/// Query encoding may differ from document encoding for models that prefix
/// queries (e.g. arctic-embed uses `"query: "`); implementations handle that
/// internally.
pub trait EncodeQuery {
    fn encode_query(&self, query: &str) -> Result<Vec<f32>>;

    /// Dimensionality of the dense vectors produced by this embedder.
    fn dim(&self) -> usize;
}

/// Embed a batch of documents (chunks, code, passages) into dense vectors.
pub trait EncodeDocuments {
    /// `batch_size` controls intra-batch sizing passed to the underlying model.
    fn encode_documents(&self, documents: &[&str], batch_size: usize) -> Result<Vec<Vec<f32>>>;
}

/// Produce sparse (BM25-style) vectors for a batch of texts.
pub trait SparseEmbed {
    /// Document / passage encoding (TF-saturation weights for BM25).
    fn embed_sparse(&self, texts: &[&str]) -> Result<Vec<SparseVector>>;

    /// Query encoding. Default: same as documents. BM25 overrides this with
    /// unit weights on unique tokens (fastembed `query_embed`).
    fn embed_query_sparse(&self, texts: &[&str]) -> Result<Vec<SparseVector>> {
        self.embed_sparse(texts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_vector_to_tuples_preserves_order() {
        let sv = SparseVector::new(vec![3, 1, 2], vec![0.5, 0.25, 0.75]);
        assert_eq!(sv.to_tuples(), vec![(3, 0.5), (1, 0.25), (2, 0.75)]);
        assert!(!sv.is_empty());
    }

    #[test]
    fn empty_sparse_vector() {
        let sv = SparseVector::default();
        assert!(sv.is_empty());
        assert!(sv.to_tuples().is_empty());
    }
}
