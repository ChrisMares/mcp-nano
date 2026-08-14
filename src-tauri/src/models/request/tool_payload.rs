use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ScopePayload {
    pub collection: String,
    pub repo_names: Option<Vec<String>>,
    pub group_ids: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ToolPayload {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub code_search_scopes: Vec<ScopePayload>,
    #[serde(default)]
    pub document_search_scopes: Vec<ScopePayload>,
    #[serde(default)]
    pub max_chunk_limit: Option<i32>,
}
