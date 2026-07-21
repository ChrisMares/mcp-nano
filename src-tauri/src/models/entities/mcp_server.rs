use serde::Serialize;

use super::ToolDefinition;

#[derive(Debug, Default, Serialize)]
pub struct McpServer {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    pub active: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
}
