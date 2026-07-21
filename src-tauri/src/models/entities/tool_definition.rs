use serde::Serialize;

use super::{ToolCodeSearchScope, ToolDocumentSearchScope};

#[derive(Debug, Default, Serialize)]
pub struct ToolDefinition {
    pub id: String,
    pub user_id: String,
    pub mcp_server_id: String,
    pub name: String,
    pub description: Option<String>,
    pub active: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub code_search_scopes: Vec<ToolCodeSearchScope>,
    pub document_search_scopes: Vec<ToolDocumentSearchScope>,
}
