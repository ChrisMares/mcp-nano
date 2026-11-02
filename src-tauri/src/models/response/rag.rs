use serde::Serialize;

use crate::models::RagResult;

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatusResponse {
    pub role: String,
    pub model: String,
    pub device: String,
    pub cpu_reason: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct RagResponse {
    pub results: Vec<RagResult>,
    /// Definitions of functions called by `results`. Only filled (and only
    /// serialized) for `expand` requests.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<RagResult>,
    /// Hints about the request, e.g. that a symbol is defined in several
    /// files and how to qualify it. Only serialized when present.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    pub total_count: i64,
    pub user_query: String,
}

#[derive(Debug, Default, Serialize)]
pub struct MetadataValuesResponse {
    pub values: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct BackendStatusResponse {
    pub qdrant_ready: bool,
    pub qdrant_error: Option<String>,
    pub http_port: Option<u16>,
    pub grpc_port: Option<u16>,
    pub db_ready: bool,
    pub embedders_ready: bool,
    pub embedding_device: Option<String>,
    pub model_statuses: Vec<ModelStatusResponse>,
    pub qdrant_storage_path: Option<String>,
    pub sqlite_path: Option<String>,
    pub logs_path: Option<String>,
    pub logs_size_bytes: Option<u64>,
    pub worker_ready: bool,
}
