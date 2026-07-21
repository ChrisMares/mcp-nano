use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct RagQueryRequest {
    pub collection: String,
    pub query: String,
    pub limit: Option<i64>,
    pub show_documents: Option<bool>,
    #[serde(rename = "where", default)]
    pub where_clause: Option<serde_json::Value>,
}
