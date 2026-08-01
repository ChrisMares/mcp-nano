//! BM25 sparse embedder aligned with fastembed's `Qdrant/bm25`.
//!
//! Must be used with Qdrant sparse vectors configured `modifier: Idf`
//! (see `qdrant::hybrid_collection_builder`). IDF is applied server-side;
//! we only emit the TF-saturation component for documents, and unit weights
//! for queries — matching fastembed's `embed` / `query_embed` split.
//!
//! Reference: https://github.com/qdrant/fastembed/blob/main/fastembed/sparse/bm25.py

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::OnceLock;

use anyhow::Result;
use rust_stemmers::{Algorithm, Stemmer};

use super::traits::{SparseEmbed, SparseVector};

/// Default BM25 k1 (term-frequency saturation). Matches fastembed / Okapi BM25.
const DEFAULT_K1: f32 = 1.2;
/// Default BM25 b (document-length normalization). Matches fastembed.
const DEFAULT_B: f32 = 0.75;
/// Fixed corpus average document length. Matches fastembed `avg_len=256.0`.
/// (Using per-doc length as avg_len collapses length norm and is incorrect.)
const DEFAULT_AVG_LEN: f32 = 256.0;
/// Drop tokens longer than this (fastembed `token_max_length`).
const TOKEN_MAX_LENGTH: usize = 40;

/// NLTK-style English stopwords (used when stemming is enabled, like fastembed).
const ENGLISH_STOPWORDS: &[&str] = &[
    "a", "about", "above", "after", "again", "against", "ain", "all", "am", "an", "and", "any",
    "are", "aren", "aren't", "as", "at", "be", "because", "been", "before", "being", "below",
    "between", "both", "but", "by", "can", "couldn", "couldn't", "d", "did", "didn", "didn't",
    "do", "does", "doesn", "doesn't", "doing", "don", "don't", "down", "during", "each", "few",
    "for", "from", "further", "had", "hadn", "hadn't", "has", "hasn", "hasn't", "have", "haven",
    "haven't", "having", "he", "her", "here", "hers", "herself", "him", "himself", "his", "how",
    "i", "if", "in", "into", "is", "isn", "isn't", "it", "it's", "its", "itself", "just", "ll",
    "m", "ma", "me", "mightn", "mightn't", "more", "most", "mustn", "mustn't", "my", "myself",
    "needn", "needn't", "no", "nor", "not", "now", "o", "of", "off", "on", "once", "only", "or",
    "other", "our", "ours", "ourselves", "out", "over", "own", "re", "s", "same", "shan",
    "shan't", "she", "she's", "should", "should've", "shouldn", "shouldn't", "so", "some",
    "such", "t", "than", "that", "that'll", "the", "their", "theirs", "them", "themselves",
    "then", "there", "these", "they", "this", "those", "through", "to", "too", "under", "until",
    "up", "ve", "very", "was", "wasn", "wasn't", "we", "were", "weren", "weren't", "what",
    "when", "where", "which", "while", "who", "whom", "why", "will", "with", "won", "won't",
    "wouldn", "wouldn't", "y", "you", "you'd", "you'll", "you're", "you've", "your", "yours",
    "yourself", "yourselves",
];

fn english_stemmer() -> &'static Stemmer {
    static STEMMER: OnceLock<Stemmer> = OnceLock::new();
    STEMMER.get_or_init(|| Stemmer::create(Algorithm::English))
}

/// BM25 sparse embedder (fastembed `Qdrant/bm25` parity).
pub struct Bm25Embedder {
    k1: f32,
    b: f32,
    avg_len: f32,
    token_max_length: usize,
    stopwords: HashSet<&'static str>,
    stem: bool,
}

impl Bm25Embedder {
    pub fn new() -> Self {
        Self {
            k1: DEFAULT_K1,
            b: DEFAULT_B,
            avg_len: DEFAULT_AVG_LEN,
            token_max_length: TOKEN_MAX_LENGTH,
            stopwords: ENGLISH_STOPWORDS.iter().copied().collect(),
            stem: true,
        }
    }

    pub fn with_avg_len(mut self, avg_len: f32) -> Self {
        self.avg_len = avg_len.max(1.0);
        self
    }

    /// Disable stemming and stopword filtering (fastembed `disable_stemmer=True`).
    pub fn without_stemmer(mut self) -> Self {
        self.stem = false;
        self.stopwords.clear();
        self
    }

    /// Document embedding: TF-saturation weights (IDF applied by Qdrant).
    pub fn embed_documents(&self, texts: &[&str]) -> Result<Vec<SparseVector>> {
        Ok(texts.iter().map(|t| self.embed_document(t)).collect())
    }

    /// Query embedding: unique tokens with weight 1.0 (fastembed `query_embed`).
    pub fn embed_queries(&self, texts: &[&str]) -> Result<Vec<SparseVector>> {
        Ok(texts.iter().map(|t| self.embed_query(t)).collect())
    }

    fn embed_document(&self, text: &str) -> SparseVector {
        let tokens = self.tokenize_and_stem(text);
        if tokens.is_empty() {
            return SparseVector::default();
        }
        let mut tf: HashMap<String, u32> = HashMap::new();
        for tok in &tokens {
            *tf.entry(tok.clone()).or_insert(0) += 1;
        }
        let doc_len = tokens.len() as f32;
        let avg = self.avg_len.max(1.0);
        let mut entries: Vec<(u32, f32)> = tf
            .into_iter()
            .map(|(term, count)| {
                let tf = count as f32;
                let denom = tf + self.k1 * (1.0 - self.b + self.b * (doc_len / avg));
                let weight = (tf * (self.k1 + 1.0)) / denom;
                (token_id(&term), weight)
            })
            .collect();
        entries.sort_by_key(|(idx, _)| *idx);
        // Collapse any hash collisions by summing weights (should be rare).
        coalesce_sorted(&mut entries);
        let (indices, values): (Vec<u32>, Vec<f32>) = entries.into_iter().unzip();
        SparseVector { indices, values }
    }

    fn embed_query(&self, text: &str) -> SparseVector {
        let tokens = self.tokenize_and_stem(text);
        if tokens.is_empty() {
            return SparseVector::default();
        }
        let mut ids: Vec<u32> = tokens.iter().map(|t| token_id(t)).collect();
        ids.sort_unstable();
        ids.dedup();
        let values = vec![1.0f32; ids.len()];
        SparseVector {
            indices: ids,
            values,
        }
    }

    /// Tokenize like fastembed SimpleTokenizer + optional Snowball stem/stopwords.
    fn tokenize_and_stem(&self, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for raw in tokenize_simple(text) {
            if raw.is_empty() {
                continue;
            }
            if raw.chars().count() > self.token_max_length {
                continue;
            }
            if self.stem {
                if self.stopwords.contains(raw.as_str()) {
                    continue;
                }
                let stemmed = english_stemmer().stem(&raw).into_owned();
                if stemmed.is_empty() {
                    continue;
                }
                out.push(stemmed);
            } else {
                out.push(raw);
            }
        }
        out
    }
}

impl Default for Bm25Embedder {
    fn default() -> Self {
        Self::new()
    }
}

impl SparseEmbed for Bm25Embedder {
    fn embed_sparse(&self, texts: &[&str]) -> Result<Vec<SparseVector>> {
        self.embed_documents(texts)
    }

    fn embed_query_sparse(&self, texts: &[&str]) -> Result<Vec<SparseVector>> {
        self.embed_queries(texts)
    }
}

/// fastembed SimpleTokenizer: lower-case, non-word → space, split on whitespace.
fn tokenize_simple(text: &str) -> Vec<String> {
    let mut buf = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            for c in ch.to_lowercase() {
                buf.push(c);
            }
        } else {
            buf.push(' ');
        }
    }
    buf.split_whitespace().map(|s| s.to_string()).collect()
}

/// MurmurHash3_x86_32 (seed 0) then `abs` as signed i32 — matches Python `mmh3.hash`.
fn token_id(token: &str) -> u32 {
    let mut cursor = Cursor::new(token.as_bytes());
    let h = murmur3::murmur3_32(&mut cursor, 0).unwrap_or(0);
    (h as i32).unsigned_abs()
}

fn coalesce_sorted(entries: &mut Vec<(u32, f32)>) {
    if entries.len() < 2 {
        return;
    }
    let mut w = 0usize;
    for r in 1..entries.len() {
        if entries[r].0 == entries[w].0 {
            entries[w].1 += entries[r].1;
        } else {
            w += 1;
            entries[w] = entries[r];
        }
    }
    entries.truncate(w + 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!(
            (a - b).abs() < 1e-4,
            "expected {b}, got {a} (diff {})",
            (a - b).abs()
        );
    }

    #[test]
    fn mmh3_token_ids_match_python() {
        // Values from mmh3.hash + abs in VectorFlow's venv.
        assert_eq!(token_id("rust"), 326552902);
        assert_eq!(token_id("hello"), 613153351);
        assert_eq!(token_id("jump"), 1913189942);
        assert_eq!(token_id("fox"), 1621867415);
        assert_eq!(token_id("dog"), 1312749093);
        assert_eq!(token_id("run"), 243905464);
        assert_eq!(token_id("world"), 74040069);
        assert_eq!(token_id("123"), 1632341525);
    }

    #[test]
    fn empty_text_yields_empty_sparse_vector() {
        let bm = Bm25Embedder::new();
        assert!(bm.embed_document("").is_empty());
        assert!(bm.embed_query("").is_empty());
        assert!(bm.embed_document("the the and or").is_empty());
    }

    #[test]
    fn stopwords_are_dropped_when_stemming() {
        let bm = Bm25Embedder::new();
        let v = bm.embed_document("the quick brown fox jumps over the lazy dog");
        // stemmed non-stop: quick brown fox jump lazi dog → 6
        assert_eq!(v.indices.len(), 6);
        assert!(v.values.iter().all(|&x| x > 0.0));
    }

    #[test]
    fn stemming_collapses_jumps_to_jump() {
        let bm = Bm25Embedder::new();
        let a = bm.embed_document("jumps");
        let b = bm.embed_document("jump");
        assert_eq!(a.indices, b.indices);
        assert_eq!(a.indices, vec![token_id("jump")]);
    }

    #[test]
    fn document_tf_matches_fastembed_formula() {
        // Single token doc: sat = 1*(1.2+1) / (1 + 1.2*(1-0.75+0.75*1/256))
        // = 2.2 / (1 + 1.2*(0.25 + 0.75/256)) ≈ 1.687743
        let bm = Bm25Embedder::new();
        let v = bm.embed_document("rust");
        assert_eq!(v.indices, vec![326552902]);
        approx(v.values[0], 1.687743);
    }

    #[test]
    fn fox_sentence_matches_python_reference() {
        let bm = Bm25Embedder::new();
        let v = bm.embed_document("the quick brown fox jumps over the lazy dog");
        let expected: &[(u32, f32)] = &[
            (226376294, 1.665287),
            (741580288, 1.665287),
            (771291085, 1.665287),
            (1312749093, 1.665287),
            (1621867415, 1.665287),
            (1913189942, 1.665287),
        ];
        assert_eq!(v.indices.len(), expected.len());
        for (i, &(idx, val)) in expected.iter().enumerate() {
            assert_eq!(v.indices[i], idx, "index mismatch at {i}");
            approx(v.values[i], val);
        }
    }

    #[test]
    fn query_uses_unit_weights_and_unique_ids() {
        let bm = Bm25Embedder::new();
        let q = bm.embed_query("the quick brown fox jumps over the lazy dog");
        assert_eq!(q.indices.len(), 6);
        assert!(q.values.iter().all(|&x| (x - 1.0).abs() < 1e-6));
        // query ids are sorted unique
        assert!(q.indices.windows(2).all(|w| w[0] < w[1]));
        // document weights differ from query unit weights
        let d = bm.embed_document("the quick brown fox jumps over the lazy dog");
        assert_eq!(d.indices, q.indices);
        assert!(d.values.iter().any(|&x| (x - 1.0).abs() > 0.1));
    }

    #[test]
    fn indices_strictly_increasing() {
        let bm = Bm25Embedder::new();
        let v = bm.embed_document("alpha beta gamma delta epsilon zeta eta");
        assert!(
            v.indices.windows(2).all(|w| w[0] < w[1]),
            "indices must be strictly increasing: {:?}",
            v.indices
        );
    }

    #[test]
    fn long_tokens_are_dropped() {
        let bm = Bm25Embedder::new();
        let long = "a".repeat(41);
        let v = bm.embed_document(&format!("rust {long} code"));
        assert!(v.indices.contains(&token_id("rust")));
        assert!(v.indices.contains(&token_id("code")));
        assert!(!v.indices.contains(&token_id(&long)));
    }

    #[test]
    fn without_stemmer_keeps_surface_forms() {
        let bm = Bm25Embedder::new().without_stemmer();
        let v = bm.embed_document("jumps jumping");
        assert!(v.indices.contains(&token_id("jumps")));
        assert!(v.indices.contains(&token_id("jumping")));
        assert!(!v.indices.contains(&token_id("jump")));
    }

    #[test]
    fn embed_sparse_batch_preserves_order() {
        let bm = Bm25Embedder::new();
        let texts = ["rust programming language", "machine learning models"];
        let vs = bm
            .embed_sparse(&texts.iter().copied().collect::<Vec<_>>())
            .unwrap();
        assert_eq!(vs.len(), 2);
        assert!(!vs[0].is_empty());
        assert!(!vs[1].is_empty());
        assert_ne!(vs[0].indices, vs[1].indices);
    }

    #[test]
    fn avg_len_affects_weight() {
        let short_avg = Bm25Embedder::new().with_avg_len(1.0);
        let long_avg = Bm25Embedder::new().with_avg_len(10_000.0);
        // Longer doc relative to avg_len → smaller weight when b>0
        let text = "alpha beta gamma delta epsilon zeta eta theta";
        let a = short_avg.embed_document(text);
        let b = long_avg.embed_document(text);
        assert_eq!(a.indices, b.indices);
        assert!(a.values[0] < b.values[0]);
    }

    #[test]
    fn to_tuples_never_panics_on_empty() {
        let empty = SparseVector::default();
        assert!(empty.to_tuples().is_empty());
        let bm = Bm25Embedder::new();
        assert!(bm.embed_document("").to_tuples().is_empty());
    }
}
