use std::collections::HashMap;

use anyhow::Result;

use super::traits::{SparseEmbed, SparseVector};

/// Standard English stopwords used by the BM25 sparse embedder.
/// Inline list (~50 words) — no external resource file needed.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "if", "in", "into", "is", "it",
    "no", "not", "of", "on", "or", "such", "that", "the", "their", "then", "there", "these",
    "they", "this", "to", "was", "will", "with", "i", "you", "he", "she", "we", "his", "her",
    "our", "your", "its", "from", "have", "has", "had", "were", "been", "being", "do", "does",
    "did", "doing", "can", "could", "should", "would", "may", "might", "must", "shall", "will",
    "about", "above", "after", "again", "all", "any", "because", "down", "during", "each", "few",
    "more", "most", "other", "over", "own", "same", "than", "too", "under", "until", "up",
    "very", "what", "when", "where", "which", "while", "who", "whom", "why", "how",
];

/// Hand-rolled BM25 sparse embedder.
///
/// Stateless: produces BM25-weighted term-frequency vectors where each unique
/// term maps to a stable u32 index via FNV-1a hashing. IDF is computed by
/// Qdrant server-side from the index; we only emit per-document term weights
/// following the BM25 TF saturation formula:
///
///   weight(term, doc) = tf_saturation * idf_approximation
///
/// where `tf_saturation = (tf * (k1 + 1)) / (tf + k1 * (1 - b + b * dl/avgdl))`.
/// Since we don't know corpus statistics at embed time, we use `avgdl =
/// dl` (so the length normalization collapses to 1) and assume `idf = 1` —
/// Qdrant's sparse-vector scorer multiplies by IDF from the index. The
/// resulting vector is the saturated term frequency, which matches the
/// `Qdrant/bm25` model semantics used in the Python prototype.
pub struct Bm25Embedder {
    k1: f32,
    b: f32,
    stopwords: std::collections::HashSet<&'static str>,
}

impl Bm25Embedder {
    pub fn new() -> Self {
        Self {
            k1: 1.2,
            b: 0.75,
            stopwords: STOPWORDS.iter().copied().collect(),
        }
    }

    /// Override the defaults (useful for tests / tuning).
    pub fn with_params(k1: f32, b: f32) -> Self {
        Self {
            k1,
            b,
            stopwords: STOPWORDS.iter().copied().collect(),
        }
    }

    fn tokenize<'a>(&self, text: &'a str) -> Vec<&'a str> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|s| {
                let lower = s.to_ascii_lowercase();
                !lower.is_empty() && !self.stopwords.contains(lower.as_str())
            })
            .collect()
    }

    /// Stable u32 term id via FNV-1a hash of the lowercased term.
    fn term_id(term: &str) -> u32 {
        let mut hash: u32 = 0x811c9dc5;
        for byte in term.as_bytes() {
            hash ^= *byte as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
        hash
    }

    fn embed_one(&self, text: &str) -> SparseVector {
        let tokens = self.tokenize(text);
        if tokens.is_empty() {
            return SparseVector::default();
        }
        let dl = tokens.len() as f32;
        let avgdl = dl; // no corpus stats; length norm collapses to 1

        // Term frequencies within this document.
        let mut tfs: HashMap<u32, (String, u32)> = HashMap::new();
        for tok in &tokens {
            let lower = tok.to_ascii_lowercase();
            let id = Self::term_id(&lower);
            tfs.entry(id)
                .or_insert_with(|| (lower.clone(), 0))
                .1 += 1;
        }

        let mut entries: Vec<(u32, f32)> = tfs
            .into_values()
            .map(|(_term, tf)| {
                let tf = tf as f32;
                let sat = (tf * (self.k1 + 1.0)) / (tf + self.k1 * (1.0 - self.b + self.b * (dl / avgdl)));
                // idf folded in server-side; emit sat * 1.0
                (Self::term_id(&_term), sat)
            })
            .collect();

        // Sort by index for deterministic output (Qdrant accepts unsorted,
        // but sorted makes tests + debugging easier).
        entries.sort_by_key(|(idx, _)| *idx);
        let (indices, values): (Vec<u32>, Vec<f32>) = entries.into_iter().unzip();
        SparseVector { indices, values }
    }
}

impl Default for Bm25Embedder {
    fn default() -> Self {
        Self::new()
    }
}

impl SparseEmbed for Bm25Embedder {
    fn embed_sparse(&self, texts: &[&str]) -> Result<Vec<SparseVector>> {
        Ok(texts.iter().map(|t| self.embed_one(t)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_yields_empty_sparse_vector() {
        let bm = Bm25Embedder::new();
        let v = bm.embed_one("");
        assert!(v.is_empty());
    }

    #[test]
    fn stopwords_are_dropped() {
        let bm = Bm25Embedder::new();
        // "the" (×2) and "over" are stopwords. Meaningful terms: quick, brown,
        // fox, jumps, lazy, dog → 6 unique tokens.
        let v = bm.embed_one("the quick brown fox jumps over the lazy dog");
        assert_eq!(v.indices.len(), 6);
        assert!(v.values.iter().all(|&x| x > 0.0));
        // A text with only stopwords must produce an empty vector.
        let empty = bm.embed_one("the the over and the");
        assert!(empty.is_empty());
    }

    #[test]
    fn term_id_is_stable() {
        assert_eq!(Bm25Embedder::term_id("hello"), Bm25Embedder::term_id("hello"));
        assert_ne!(Bm25Embedder::term_id("hello"), Bm25Embedder::term_id("world"));
    }

    #[test]
    fn embed_sparse_batch_preserves_order() {
        let bm = Bm25Embedder::new();
        let texts = ["rust programming language", "machine learning models"];
        let vs = bm.embed_sparse(&texts.iter().copied().collect::<Vec<_>>()).unwrap();
        assert_eq!(vs.len(), 2);
        assert!(!vs[0].is_empty());
        assert!(!vs[1].is_empty());
        // Different texts should produce different vectors (almost always).
        assert_ne!(vs[0].indices, vs[1].indices);
    }

    #[test]
    fn indices_are_sorted() {
        let bm = Bm25Embedder::new();
        let v = bm.embed_one("alpha beta gamma delta epsilon");
        assert!(v.indices.windows(2).all(|w| w[0] < w[1]), "indices must be strictly increasing: {:?}", v.indices);
    }

    #[test]
    fn saturation_formula_matches_bm25() {
        // Single term appearing once in a 1-token doc:
        //   sat = (1 * (1.2 + 1)) / (1 + 1.2 * (1 - 0.75 + 0.75 * 1)) = 2.2 / (1 + 1.2) = 1.0
        let bm = Bm25Embedder::new();
        let v = bm.embed_one("rust");
        assert_eq!(v.indices.len(), 1);
        assert!((v.values[0] - 1.0).abs() < 1e-5, "got {}", v.values[0]);
    }
}
