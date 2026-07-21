use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct EmbeddingOptions {
    pub collection: Option<String>,
    pub repo_name: Option<String>,
    pub group: Option<String>,
    pub metadata: Option<serde_json::Value>,
}
