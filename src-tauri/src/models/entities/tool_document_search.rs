use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Default, Serialize, FromRow)]
pub struct ToolDocumentSearchScope {
    pub id: String,
    pub tool_definition_id: String,
    pub collection: String,
    #[sqlx(json)]
    pub group_ids: Vec<String>,
}
