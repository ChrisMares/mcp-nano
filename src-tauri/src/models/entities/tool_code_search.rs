use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Default, Serialize, FromRow)]
pub struct ToolCodeSearchScope {
    pub id: String,
    pub tool_definition_id: String,
    pub collection: String,
    #[sqlx(json)]
    pub repo_names: Vec<String>,
}
