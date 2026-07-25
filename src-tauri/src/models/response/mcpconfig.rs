use serde::Serialize;

use crate::models::entities::{McpServer, ToolDefinition};

#[derive(Debug, Default, Serialize)]
pub struct ServersResponse {
    pub servers: Vec<McpServer>,
}

#[derive(Debug, Default, Serialize)]
pub struct ServerResponse {
    pub server: McpServer,
}

#[derive(Debug, Default, Serialize)]
pub struct ToolResponse {
    pub tool: ToolDefinition,
}

#[derive(Debug, Default, Serialize)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug, Default, Serialize)]
pub struct ConnectionInfo {
    pub mcp_url: String,
    pub server_id: String,
    pub server_name: String,
    pub full_url: String,
    pub config_snippets: serde_json::Value,
}
