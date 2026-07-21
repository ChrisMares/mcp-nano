use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct ToolDocumentSearchScope {
    pub id: String,
    pub tool_definition_id: String,
    pub collection: String,
    pub group_ids: Vec<String>,
}
