use serde::Serialize;
use sqlx::FromRow;

use super::{ToolCodeSearchScope, ToolDocumentSearchScope};

#[derive(Debug, Default, Serialize, FromRow)]
pub struct ToolDefinition {
    pub id: String,
    pub mcp_server_id: String,
    pub name: String,
    pub description: Option<String>,
    pub active: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    #[sqlx(skip)]
    pub code_search_scopes: Vec<ToolCodeSearchScope>,
    #[sqlx(skip)]
    pub document_search_scopes: Vec<ToolDocumentSearchScope>,
}
