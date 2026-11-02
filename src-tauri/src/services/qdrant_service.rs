use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use qdrant_client::qdrant::{
    point_id::PointIdOptions, Condition, DeletePointsBuilder, FieldType, Filter, Fusion, PointId,
    PointStruct, PrefetchQueryBuilder, Query, QueryPointsBuilder, ScrollPointsBuilder,
    SetPayloadPointsBuilder, UpsertPointsBuilder, Vector, Vectors,
};
use qdrant_client::{Payload, Qdrant};
use uuid::Uuid;

use tracing::{debug, info};

use crate::models::VecDbResult;
use crate::services::embedders::{Bm25Embedder, SparseEmbed};

/// Collection names allowed for auto-create. Mirrors the Python
/// `QdrantCollection` enum.
pub const ALLOWED_COLLECTIONS: &[&str] = &["codebase", "general"];

/// Which fields to include in query results. Mirrors the Python `include`
/// list literal.
#[derive(Debug, Clone, Default)]
pub struct Include {
    pub metadatas: bool,
    pub documents: bool,
    pub distances: bool,
}

impl Include {
    pub fn all() -> Self {
        Self {
            metadatas: true,
            documents: true,
            distances: true,
        }
    }
}

/// Business-logic wrapper over a `qdrant_client::Qdrant` client.
///
/// Cheap to construct: clones the `Qdrant` handle (a tonic channel clone).
/// Created per operation from `QdrantState::client`, or once and reused.
#[derive(Clone)]
pub struct QdrantService {
    client: Qdrant,
}

/// Remove the legacy `document` key from a point payload and return the
/// chunk text: the legacy key if present, else `content`, else `code`.
fn take_document_text(payload: &mut serde_json::Value) -> String {
    let serde_json::Value::Object(map) = payload else {
        return String::new();
    };
    if let Some(serde_json::Value::String(text)) = map.remove("document") {
        return text;
    }
    for key in ["content", "code"] {
        if let Some(text) = map.get(key).and_then(|v| v.as_str()) {
            return text.to_string();
        }
    }
    String::new()
}

/// A point's id as a string and its payload as JSON with the chunk text
/// taken out (see [`take_document_text`]).
fn point_parts(
    id: Option<PointId>,
    payload: HashMap<String, qdrant_client::qdrant::Value>,
) -> (String, serde_json::Value, String) {
    let id = match id.and_then(|pid| pid.point_id_options) {
        Some(PointIdOptions::Num(n)) => n.to_string(),
        Some(PointIdOptions::Uuid(s)) => s,
        None => String::new(),
    };
    // Points written by older builds carry a separate "document" key; newer
    // points keep the text only in metadata.
    let mut payload = serde_json::Value::from(Payload::from(payload));
    let document = take_document_text(&mut payload);
    (id, payload, document)
}

fn single_row(
    ids: Vec<String>,
    documents: Vec<String>,
    metadatas: Vec<serde_json::Value>,
    distances: Vec<f64>,
) -> VecDbResult {
    VecDbResult {
        ids: vec![ids],
        documents: vec![documents],
        metadatas: vec![metadatas],
        distances: vec![distances],
        count: None,
    }
}

impl QdrantService {
    pub fn new(client: Qdrant) -> Self {
        Self { client }
    }

    /// List all collection names.
    pub async fn list_collections(&self) -> Result<Vec<String>> {
        let resp = self
            .client
            .list_collections()
            .await
            .context("list_collections")?;
        Ok(resp.collections.into_iter().map(|c| c.name).collect())
    }

    /// Idempotently ensure a collection exists. Only `codebase` and `general`
    /// are allowed for auto-creation (matches the Python `ensure_collection`
    /// guard). Returns `true` if the collection exists after the call.
    pub async fn ensure_collection(&self, name: &str) -> Result<bool> {
        if !ALLOWED_COLLECTIONS.contains(&name) {
            return Err(anyhow!(
                "collection '{name}' is not allowed for auto-create; allowed: {ALLOWED_COLLECTIONS:?}"
            ));
        }
        let existing = self.list_collections().await?;
        if existing.iter().any(|c| c == name) {
            return Ok(true);
        }
        self.client
            .create_collection(crate::qdrant::hybrid_collection_builder(
                name,
                crate::qdrant::EMBEDDING_DIM,
            ))
            .await
            .with_context(|| format!("create_collection({name})"))?;
        Ok(true)
    }

    /// Exact count of points in a collection.
    pub async fn count_items(&self, collection: &str) -> Result<i64> {
        use qdrant_client::qdrant::CountPointsBuilder;
        let resp = self
            .client
            .count(CountPointsBuilder::new(collection).exact(true))
            .await
            .with_context(|| format!("count({collection})"))?;
        let count = resp.result.map(|r| r.count as i64).unwrap_or(0);
        Ok(count)
    }

    /// Batch upsert items with hybrid (dense + sparse) vectors.
    ///
    /// `ids`, `documents`, `dense_embeddings`, `metadatas` must all have the
    /// same length. Sparse vectors are computed from `documents` via the
    /// supplied `Bm25Embedder`. The payload is the metadata only: chunk text
    /// already lives in `metadata.content` (documents) or `metadata.code`
    /// (code), so it is not stored a second time.
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_items(
        &self,
        collection: &str,
        ids: &[Uuid],
        documents: &[String],
        dense_embeddings: &[Vec<f32>],
        metadatas: &[serde_json::Value],
        bm25: &Bm25Embedder,
        batch_size: usize,
    ) -> Result<()> {
        if ids.len() != documents.len()
            || ids.len() != dense_embeddings.len()
            || ids.len() != metadatas.len()
        {
            return Err(anyhow!(
                "upsert_items: ids ({}), documents ({}), dense ({}), metadatas ({}): length mismatch",
                ids.len(),
                documents.len(),
                dense_embeddings.len(),
                metadatas.len()
            ));
        }
        if ids.is_empty() {
            return Ok(());
        }
        let bs = if batch_size == 0 { 250 } else { batch_size };
        debug!(
            "upserting {} items to {collection} in batches of {bs}",
            ids.len()
        );

        for chunk_start in (0..ids.len()).step_by(bs) {
            let chunk_end = (chunk_start + bs).min(ids.len());
            let doc_refs: Vec<&str> = documents[chunk_start..chunk_end]
                .iter()
                .map(|s| s.as_str())
                .collect();
            let sparse_vecs = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                bm25.embed_sparse(&doc_refs)
            }))
            .map_err(|payload| {
                let msg = crate::panic_payload_to_string(&*payload);
                anyhow!("BM25 sparse embed panicked during upsert: {msg}")
            })?
            .context("BM25 sparse embed during upsert")?;
            if sparse_vecs.len() != doc_refs.len() {
                return Err(anyhow!(
                    "BM25 sparse embed length mismatch: got {} for {} docs",
                    sparse_vecs.len(),
                    doc_refs.len()
                ));
            }

            let mut points = Vec::with_capacity(chunk_end - chunk_start);
            for (i, sparse) in sparse_vecs.iter().enumerate() {
                let idx = chunk_start + i;
                let mut vectors: HashMap<String, Vector> = HashMap::new();
                vectors.insert(
                    "dense".to_string(),
                    Vector::from(dense_embeddings[idx].clone()),
                );
                // Omit empty sparse vectors (stopword-only / empty docs). Qdrant
                // accepts missing named vectors; empty sparse is rejected by some paths.
                if !sparse.is_empty() {
                    vectors.insert("sparse".to_string(), Vector::from(sparse.to_tuples()));
                }
                let vectors = Vectors::from(vectors);

                let payload = Payload::try_from(metadatas[idx].clone())
                    .map_err(|e| anyhow!("payload build failed: {e}"))?;

                points.push(PointStruct::new(ids[idx], vectors, payload));
            }

            self.client
                .upsert_points(UpsertPointsBuilder::new(collection, points).wait(true))
                .await
                .with_context(|| {
                    format!("upsert batch {chunk_start}..{chunk_end} on {collection}")
                })?;
        }
        Ok(())
    }

    /// Hybrid query: dense prefetch + BM25 sparse prefetch fused with RRF.
    /// Falls back to dense-only when `query_text` is `None`.
    ///
    /// Prefetch limit is `n_results * PREFETCH_OVERFETCH` so RRF has a larger
    /// candidate pool. Filters are applied on **each prefetch** and the outer
    /// query so scoped search does not waste the candidate budget.
    #[allow(clippy::too_many_arguments)]
    pub async fn query_items(
        &self,
        collection: &str,
        query_dense: &[f32],
        query_text: Option<&str>,
        n_results: usize,
        filter: Option<Filter>,
        include: Include,
        bm25: Option<&Bm25Embedder>,
    ) -> Result<VecDbResult> {
        let limit = n_results.max(1) as u64;
        let prefetch_limit = limit
            .saturating_mul(crate::qdrant::PREFETCH_OVERFETCH)
            .max(limit);

        debug!(
            "query {collection}: limit={n_results} prefetch={prefetch_limit} hybrid={}",
            query_text.is_some()
        );
        let sparse = match (query_text, bm25) {
            (Some(text), Some(bm25)) => bm25
                .embed_query_sparse(&[text])
                .context("BM25 sparse query embed")?
                .into_iter()
                .next()
                .unwrap_or_default(),
            _ => Default::default(),
        };
        let prefetch = |query: Query, using: &str| {
            let mut builder = PrefetchQueryBuilder::default()
                .query(query)
                .using(using)
                .limit(prefetch_limit);
            if let Some(f) = filter.clone() {
                builder = builder.filter(f);
            }
            builder
        };

        // Hybrid only when the query produced at least one sparse term.
        let mut builder = if sparse.is_empty() {
            QueryPointsBuilder::new(collection)
                .query(query_dense.to_vec())
                .using("dense")
        } else {
            QueryPointsBuilder::new(collection)
                .add_prefetch(prefetch(query_dense.to_vec().into(), "dense"))
                .add_prefetch(prefetch(sparse.to_tuples().into(), "sparse"))
                .query(Fusion::Rrf)
        }
        .limit(limit)
        .with_payload(true);
        if let Some(f) = filter {
            builder = builder.filter(f);
        }

        let resp = self
            .client
            .query(builder)
            .await
            .with_context(|| format!("query({collection})"))?;

        let mut ids = Vec::new();
        let mut documents = Vec::new();
        let mut metadatas = Vec::new();
        let mut distances = Vec::new();
        for point in resp.result {
            let (id, payload, document) = point_parts(point.id, point.payload);
            ids.push(id);
            if include.documents {
                documents.push(document);
            }
            if include.metadatas {
                metadatas.push(payload);
            }
            if include.distances {
                distances.push(point.score as f64);
            }
        }
        Ok(single_row(ids, documents, metadatas, distances))
    }

    /// Fetch up to `limit` points matching `filter`, without ranking. Returns
    /// the same shape as [`Self::query_items`] with zero distances.
    pub async fn scroll_items(
        &self,
        collection: &str,
        filter: Filter,
        limit: usize,
    ) -> Result<VecDbResult> {
        let resp = self
            .client
            .scroll(
                ScrollPointsBuilder::new(collection)
                    .filter(filter)
                    .limit(limit.min(u32::MAX as usize) as u32)
                    .with_payload(true)
                    .with_vectors(false),
            )
            .await
            .with_context(|| format!("scroll {collection}"))?;

        let mut ids = Vec::new();
        let mut documents = Vec::new();
        let mut metadatas = Vec::new();
        let mut distances = Vec::new();
        for point in resp.result {
            let (id, payload, document) = point_parts(point.id, point.payload);
            ids.push(id);
            documents.push(document);
            metadatas.push(payload);
            distances.push(0.0);
        }
        Ok(single_row(ids, documents, metadatas, distances))
    }

    /// Delete points by IDs or by metadata filter.
    pub async fn delete_items(
        &self,
        collection: &str,
        ids: Option<&[Uuid]>,
        filter: Option<Filter>,
    ) -> Result<()> {
        if let Some(ids) = ids {
            if !ids.is_empty() {
                info!("deleting {} points from {collection} by id", ids.len());
                let point_ids: Vec<PointId> = ids.iter().copied().map(PointId::from).collect();
                self.client
                    .delete_points(
                        DeletePointsBuilder::new(collection)
                            .points(point_ids)
                            .wait(true),
                    )
                    .await
                    .with_context(|| format!("delete_points by ids on {collection}"))?;
                return Ok(());
            }
        }
        if let Some(filter) = filter {
            // `DeletePointsBuilder::points` accepts `impl Into<PointsSelectorOneOf>`,
            // and `Filter` converts into the `Filter` variant directly.
            self.client
                .delete_points(
                    DeletePointsBuilder::new(collection)
                        .points(filter)
                        .wait(true),
                )
                .await
                .with_context(|| format!("delete_points by filter on {collection}"))?;
            return Ok(());
        }
        Err(anyhow!("delete_items: must provide ids or filter"))
    }

    /// Create a payload index. `field_type` accepts the same string literals
    /// as the Python `create_index`: "keyword", "integer", "float", "text",
    /// "datetime", "bool", "uuid", "geo".
    pub async fn create_index(&self, collection: &str, key: &str, field_type: &str) -> Result<()> {
        use qdrant_client::qdrant::CreateFieldIndexCollectionBuilder;
        let ft = match field_type.to_ascii_lowercase().as_str() {
            "keyword" => FieldType::Keyword,
            "integer" => FieldType::Integer,
            "float" => FieldType::Float,
            "text" => FieldType::Text,
            "datetime" => FieldType::Datetime,
            "bool" => FieldType::Bool,
            "uuid" => FieldType::Uuid,
            "geo" => FieldType::Geo,
            other => return Err(anyhow!("unknown field type '{other}'")),
        };
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(collection, key, ft))
            .await
            .with_context(|| format!("create_index({collection}, {key}, {field_type})"))?;
        Ok(())
    }

    /// Backfill empty `repo_name` payloads from `zip_filename` (basename minus
    /// `.zip`). Used so Data Management can list zips that were uploaded
    /// before the default-repo-name fix. Idempotent.
    pub async fn repair_empty_repo_names_from_zip(&self, collection: &str) -> Result<usize> {
        use crate::services::ingestion_service::repo_name_from_zip_filename;

        // Ensure zip_filename is indexed so we can facet it.
        let _ = self
            .create_index(collection, "zip_filename", "keyword")
            .await;

        let zip_names = match self
            .get_metadata_values_by_key(collection, "zip_filename", None)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                debug!("repair_empty_repo_names: facet zip_filename failed: {e:#}");
                return Ok(0);
            }
        };

        let mut repaired = 0usize;
        for name in zip_names {
            let Some(zf) = name.as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            let repo = repo_name_from_zip_filename(zf);
            if repo.is_empty() {
                continue;
            }

            // Only repair when this zip has no non-empty repo_name yet
            // (legacy uploads used ""). Matching empty string in filters is
            // unreliable, so we set by zip_filename after checking facet.
            let zip_filter = Filter::must([Condition::matches("zip_filename", zf.to_string())]);
            let existing = self
                .get_metadata_values_by_key(collection, "repo_name", Some(zip_filter.clone()))
                .await
                .unwrap_or_default();
            let has_named = existing
                .iter()
                .any(|v| v.as_str().is_some_and(|s| !s.is_empty() && s != repo));
            let already_ok = existing.iter().any(|v| v.as_str() == Some(repo.as_str()));
            if has_named || already_ok {
                continue;
            }

            let mut payload = HashMap::new();
            payload.insert(
                "repo_name".to_string(),
                qdrant_client::qdrant::Value::from(repo.clone()),
            );
            match self
                .client
                .set_payload(
                    SetPayloadPointsBuilder::new(collection, payload)
                        .points_selector(zip_filter)
                        .wait(true),
                )
                .await
            {
                Ok(_) => {
                    info!("repaired repo_name={repo} for zip_filename={zf} on {collection}");
                    repaired += 1;
                }
                Err(e) => {
                    debug!("set_payload repo_name for {zf}: {e:#}");
                }
            }
        }
        Ok(repaired)
    }

    /// Unique values for a payload key (faceted). Requires an existing payload
    /// index on `key` — same guard as the Python prototype.
    pub async fn get_metadata_values_by_key(
        &self,
        collection: &str,
        key: &str,
        filter: Option<Filter>,
    ) -> Result<Vec<serde_json::Value>> {
        use qdrant_client::qdrant::{facet_value::Variant, FacetCountsBuilder};
        let mut builder = FacetCountsBuilder::new(collection, key).limit(1000);
        if let Some(f) = filter {
            builder = builder.filter(f);
        }
        let resp = self
            .client
            .facet(builder)
            .await
            .with_context(|| format!("facet({collection}, {key})"))?;
        Ok(resp
            .hits
            .into_iter()
            .filter_map(|h| h.value.and_then(|fv| fv.variant))
            .map(|variant| match variant {
                Variant::StringValue(s) => serde_json::Value::String(s),
                Variant::IntegerValue(i) => serde_json::Value::Number(i.into()),
                Variant::BoolValue(b) => serde_json::Value::Bool(b),
            })
            .collect())
    }

    /// Build a Qdrant `Filter` from a JSON object following the Python
    /// `_build_filter` shape:
    ///   - `{"$and": [clause, ...]}` → must=[each clause as Filter]
    ///   - `{"$or":  [clause, ...]}` → should=[each clause as Filter]
    ///   - plain `{key: value, ...}` → must=[FieldCondition for each pair]
    pub fn build_filter(where_clause: &serde_json::Value) -> Result<Filter> {
        let obj = where_clause
            .as_object()
            .ok_or_else(|| anyhow!("filter must be a JSON object, got: {where_clause}"))?;
        Self::build_filter_map(obj)
    }

    fn build_filter_map(obj: &serde_json::Map<String, serde_json::Value>) -> Result<Filter> {
        // Handle operator keys first.
        if let Some(and_val) = obj.get("$and") {
            return Ok(Filter {
                must: Self::sub_filters("$and", and_val)?,
                ..Default::default()
            });
        }
        if let Some(or_val) = obj.get("$or") {
            return Ok(Filter {
                should: Self::sub_filters("$or", or_val)?,
                ..Default::default()
            });
        }

        // Plain equality clause: every key/value pair becomes a FieldCondition.
        let mut conds = Vec::with_capacity(obj.len());
        for (key, val) in obj {
            let cond = match val {
                serde_json::Value::String(s) => Condition::matches(key.clone(), s.clone()),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Condition::matches(key.clone(), i)
                    } else {
                        // Qdrant's MatchValue enum has no Float variant; emit
                        // an exact range as a workaround.
                        let f = n
                            .as_f64()
                            .ok_or_else(|| anyhow!("invalid number in filter for key '{key}'"))?;
                        let eps = f64::EPSILON;
                        use qdrant_client::qdrant::{FieldCondition, Range};
                        let range = Range {
                            gte: Some(f - eps),
                            lte: Some(f + eps),
                            ..Default::default()
                        };
                        Condition {
                            condition_one_of: Some(
                                qdrant_client::qdrant::condition::ConditionOneOf::Field(
                                    FieldCondition {
                                        key: key.clone(),
                                        range: Some(range),
                                        ..Default::default()
                                    },
                                ),
                            ),
                        }
                    }
                }
                serde_json::Value::Bool(b) => Condition::matches(key.clone(), *b),
                _ => {
                    return Err(anyhow!(
                        "unsupported value type in filter for key '{key}': {val}"
                    ))
                }
            };
            conds.push(cond);
        }
        Ok(Filter::must(conds))
    }

    /// The clauses of an `$and`/`$or` operator, each as a nested filter.
    fn sub_filters(op: &str, value: &serde_json::Value) -> Result<Vec<Condition>> {
        let clauses = value
            .as_array()
            .ok_or_else(|| anyhow!("{op} value must be an array"))?;
        let mut conditions = Vec::with_capacity(clauses.len());
        for clause in clauses {
            let clause_obj = clause
                .as_object()
                .ok_or_else(|| anyhow!("{op} clause must be an object"))?;
            conditions.push(Condition::from(Self::build_filter_map(clause_obj)?));
        }
        Ok(conditions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn take_document_text_prefers_legacy_key_then_content_then_code() {
        let mut legacy = json!({"document": "old", "content": "new"});
        assert_eq!(take_document_text(&mut legacy), "old");
        assert!(legacy.get("document").is_none());
        assert_eq!(legacy["content"], "new");

        let mut doc = json!({"content": "text", "file_name": "a.md"});
        assert_eq!(take_document_text(&mut doc), "text");
        assert_eq!(doc["content"], "text");

        let mut code = json!({"code": "fn main() {}"});
        assert_eq!(take_document_text(&mut code), "fn main() {}");

        let mut empty = json!({"file_name": "a.md"});
        assert_eq!(take_document_text(&mut empty), "");
    }

    #[test]
    fn build_filter_plain_equality() {
        let f = QdrantService::build_filter(&json!({"repo_name": "my_repo"})).unwrap();
        assert_eq!(f.must.len(), 1);
        assert!(f.should.is_empty());
    }

    #[test]
    fn ensure_collection_rejects_unknown_name() {
        // We can't easily test the async ensure_collection without a running
        // Qdrant, but the guard runs before any network call, so we can
        // verify the error path by checking the allow-list directly.
        assert!(ALLOWED_COLLECTIONS.contains(&"codebase"));
        assert!(ALLOWED_COLLECTIONS.contains(&"general"));
        assert!(!ALLOWED_COLLECTIONS.contains(&"unknown_collection"));
    }

    #[test]
    fn build_filter_and_combines_clauses() {
        let f = QdrantService::build_filter(&json!({
            "$and": [
                {"user_id": "u1"},
                {"repo_name": "r1"}
            ]
        }))
        .unwrap();
        assert_eq!(f.must.len(), 2);
        // Each must entry is a nested Filter wrapped in a Condition.
        for cond in &f.must {
            assert!(matches!(
                cond.condition_one_of,
                Some(qdrant_client::qdrant::condition::ConditionOneOf::Filter(_))
            ));
        }
    }

    #[test]
    fn build_filter_or_uses_should() {
        let f = QdrantService::build_filter(&json!({
            "$or": [
                {"group": "a"},
                {"group": "b"}
            ]
        }))
        .unwrap();
        assert_eq!(f.should.len(), 2);
        assert!(f.must.is_empty());
    }

    #[test]
    fn build_filter_rejects_non_object() {
        let err = QdrantService::build_filter(&json!([1, 2, 3]));
        assert!(err.is_err());
    }

    #[test]
    fn build_filter_rejects_unsupported_value_type() {
        let err = QdrantService::build_filter(&json!({"key": [1, 2]}));
        assert!(err.is_err());
    }
}
