use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct ToolCodeSearchScope {
    pub id: String,
    pub tool_definition_id: String,
    pub collection: String,
    pub repo_names: Vec<String>,
}
