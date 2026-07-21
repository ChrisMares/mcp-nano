use serde::Serialize;

/// Result of a vector-db query — mirror of the Python `VecDbResult` TypedDict.
///
/// Each outer Vec corresponds to one query in a batch; inner Vecs hold the
/// per-hit values in the order returned by Qdrant. `distances` carries Qdrant
/// scores (higher = more similar for Cosine + RRF fusion).
#[derive(Debug, Default, Clone, Serialize)]
pub struct VecDbResult {
    pub ids: Vec<Vec<String>>,
    pub documents: Vec<Vec<String>>,
    pub metadatas: Vec<Vec<serde_json::Value>>,
    pub distances: Vec<Vec<f64>>,
    pub count: Option<i64>,
}

impl VecDbResult {
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty() || self.ids.iter().all(|row| row.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_result_is_empty() {
        assert!(VecDbResult::default().is_empty());
    }

    #[test]
    fn result_with_hits_is_not_empty() {
        let r = VecDbResult {
            ids: vec![vec!["a".into(), "b".into()]],
            documents: vec![vec!["doc a".into(), "doc b".into()]],
            metadatas: vec![vec![serde_json::json!({"k": 1}), serde_json::json!({"k": 2})]],
            distances: vec![vec![0.9, 0.7]],
            count: Some(2),
        };
        assert!(!r.is_empty());
    }

    #[test]
    fn result_with_empty_inner_rows_is_empty() {
        let r = VecDbResult {
            ids: vec![vec![]],
            ..Default::default()
        };
        assert!(r.is_empty());
    }
}
