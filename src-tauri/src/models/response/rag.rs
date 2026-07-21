use serde::Serialize;

use crate::models::RagResult;

#[derive(Debug, Default, Serialize)]
pub struct RagResponse {
    pub results: Vec<RagResult>,
    pub total_count: i64,
    pub user_query: String,
}

#[derive(Debug, Default, Serialize)]
pub struct MetadataValuesResponse {
    pub values: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct CollectionsResponse {
    pub collections: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct EmbedderStatusResponse {
    pub dense_loaded: bool,
    pub reranker_loaded: bool,
    pub bm25_loaded: bool,
    pub models_dir: String,
}
