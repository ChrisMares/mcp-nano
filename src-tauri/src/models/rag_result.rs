use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct RagResult {
    pub id: String,
    pub document: String,
    pub metadata: serde_json::Value,
    pub score: f64,
}
