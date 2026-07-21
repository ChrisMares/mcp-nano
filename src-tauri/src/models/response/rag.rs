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
