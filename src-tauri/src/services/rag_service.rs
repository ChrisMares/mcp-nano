use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use tracing::{debug, info};

use crate::models::request::RagQueryRequest;
use crate::models::response::RagResponse;
use crate::models::RagResult;
use crate::services::embedders::EncodeQuery;
use crate::services::embedder_state::EmbedderState;
use crate::services::qdrant_service::{Include, QdrantService};

const RERANK_BATCH_SIZE: usize = 64;
const DEFAULT_LIMIT: i64 = 10;

/// Shared RAG pipeline used by the Tauri `rag_query` command and MCP tool calls.
#[derive(Clone)]
pub struct RagService {
    embedders: Arc<EmbedderState>,
    qdrant: QdrantService,
}

impl RagService {
    pub fn new(embedders: Arc<EmbedderState>, qdrant: QdrantService) -> Self {
        Self { embedders, qdrant }
    }

    pub async fn run_rag_query(&self, requests: &[RagQueryRequest]) -> Result<RagResponse> {
        if requests.is_empty() {
            return Err(anyhow!("at least one query request is required"));
        }

        info!("RAG query: {} request(s)", requests.len());
        for req in requests {
            debug!("  collection={} query={} limit={:?}", req.collection, req.query, req.limit);
        }
        let mut all_results: Vec<RagResult> = Vec::new();
        let mut combined_query = String::new();
        let mut last_show_documents = true;

        for req in requests {
            let limit = normalize_limit(req.limit);
            last_show_documents = req.show_documents.unwrap_or(true);

            let dense = self
                .embedders
                .dense
                .encode_query(&req.query)
                .context("encoding query")?;

            let filter = match &req.where_clause {
                Some(w) if !w.is_null() && w.as_object().map(|o| !o.is_empty()).unwrap_or(true) => {
                    Some(QdrantService::build_filter(w).context("building filter")?)
                }
                _ => None,
            };

            // Overfetch candidates for the cross-encoder (prefetch overfetch is
            // handled inside query_items for RRF).
            let retrieve_n = (limit as usize).saturating_mul(4).max(1);
            let vec_result = self
                .qdrant
                .query_items(
                    &req.collection,
                    &dense,
                    Some(&req.query),
                    retrieve_n,
                    filter,
                    Include::all(),
                    Some(&self.embedders.bm25),
                )
                .await
                .with_context(|| format!("querying collection '{}'", req.collection))?;

            let mut results = vec_db_to_rag_results(&vec_result);
            results = self.rerank_results(&req.query, results, limit as usize)?;
            all_results.extend(results);

            if !combined_query.is_empty() {
                combined_query.push(' ');
            }
            combined_query.push_str(&req.query);
        }

        if !last_show_documents {
            for r in &mut all_results {
                r.document.clear();
            }
        }

        Ok(RagResponse {
            total_count: all_results.len() as i64,
            results: all_results,
            user_query: combined_query,
        })
    }

    fn rerank_results(
        &self,
        query: &str,
        mut results: Vec<RagResult>,
        top: usize,
    ) -> Result<Vec<RagResult>> {
        if results.is_empty() || top == 0 {
            return Ok(results);
        }

        let docs: Vec<&str> = results.iter().map(|r| r.document.as_str()).collect();
        let scores = self
            .embedders
            .reranker
            .rerank(query, &docs, RERANK_BATCH_SIZE)
            .context("reranking")?;

        for (result, score) in results.iter_mut().zip(scores.iter()) {
            result.score = f64::from(*score);
        }
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(top);
        Ok(results)
    }
}

fn normalize_limit(limit: Option<i64>) -> i64 {
    let n = limit.unwrap_or(DEFAULT_LIMIT);
    n.clamp(1, 100)
}

fn vec_db_to_rag_results(vec: &crate::models::VecDbResult) -> Vec<RagResult> {
    let ids = vec.ids.first().cloned().unwrap_or_default();
    let docs = vec.documents.first().cloned().unwrap_or_default();
    let metas = vec.metadatas.first().cloned().unwrap_or_default();
    let dists = vec.distances.first().cloned().unwrap_or_default();

    let n = ids.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(RagResult {
            id: ids.get(i).cloned().unwrap_or_default(),
            document: docs.get(i).cloned().unwrap_or_default(),
            metadata: metas
                .get(i)
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
            score: dists.get(i).copied().unwrap_or(0.0),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_limit_clamps() {
        assert_eq!(normalize_limit(None), 10);
        assert_eq!(normalize_limit(Some(0)), 1);
        assert_eq!(normalize_limit(Some(5)), 5);
        assert_eq!(normalize_limit(Some(200)), 100);
    }

    #[test]
    fn empty_vec_db_maps_to_empty() {
        assert!(vec_db_to_rag_results(&crate::models::VecDbResult::default()).is_empty());
    }
}
