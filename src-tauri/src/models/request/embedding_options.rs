use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbeddingOptions {
    pub collection: Option<String>,
    pub repo_name: Option<String>,
    pub group: Option<String>,
    pub metadata: Option<serde_json::Value>,
}