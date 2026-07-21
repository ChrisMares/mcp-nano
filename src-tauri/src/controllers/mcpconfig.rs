use tauri::{AppHandle, Manager};

use crate::db::DbState;
use crate::mcp::{McpState, DEFAULT_MCP_PORT};
use crate::models::request::ToolPayload;
use crate::models::response::{
    ConnectionInfo, MessageResponse, ServerResponse, ServersResponse, ToolResponse,
};
use crate::services::mcp_config;

fn pool(app: &AppHandle) -> Result<sqlx::SqlitePool, String> {
    app.try_state::<DbState>()
        .map(|s| s.pool.clone())
        .ok_or_else(|| "database not ready".into())
}

fn mcp_port(app: &AppHandle) -> u16 {
    app.try_state::<McpState>()
        .map(|s| s.port)
        .unwrap_or(DEFAULT_MCP_PORT)
}

#[tauri::command]
pub async fn get_mcp_servers(app: AppHandle) -> Result<ServersResponse, String> {
    mcp_config::list_servers(&pool(&app)?).await
}

#[tauri::command]
pub async fn create_mcp_server(
    app: AppHandle,
    name: String,
    description: Option<String>,
) -> Result<ServerResponse, String> {
    mcp_config::create_server(&pool(&app)?, name, description).await
}

#[tauri::command]
pub async fn get_mcp_server(app: AppHandle, server_id: String) -> Result<ServerResponse, String> {
    mcp_config::get_server(&pool(&app)?, &server_id).await
}

#[tauri::command]
pub async fn update_mcp_server(
    app: AppHandle,
    server_id: String,
    name: String,
    description: Option<String>,
) -> Result<ServerResponse, String> {
    mcp_config::update_server(&pool(&app)?, &server_id, name, description).await
}

#[tauri::command]
pub async fn toggle_mcp_server(
    app: AppHandle,
    server_id: String,
    active: bool,
) -> Result<ServerResponse, String> {
    mcp_config::toggle_server(&pool(&app)?, &server_id, active).await
}

#[tauri::command]
pub async fn delete_mcp_server(
    app: AppHandle,
    server_id: String,
) -> Result<MessageResponse, String> {
    mcp_config::delete_server(&pool(&app)?, &server_id).await
}

#[tauri::command]
pub async fn create_mcp_tool(
    app: AppHandle,
    server_id: String,
    tool_data: ToolPayload,
) -> Result<ToolResponse, String> {
    mcp_config::create_tool(&pool(&app)?, &server_id, tool_data).await
}

#[tauri::command]
pub async fn update_mcp_tool(
    app: AppHandle,
    server_id: String,
    tool_id: String,
    tool_data: ToolPayload,
) -> Result<ToolResponse, String> {
    mcp_config::update_tool(&pool(&app)?, &server_id, &tool_id, tool_data).await
}

#[tauri::command]
pub async fn delete_mcp_tool(
    app: AppHandle,
    server_id: String,
    tool_id: String,
) -> Result<MessageResponse, String> {
    mcp_config::delete_tool(&pool(&app)?, &server_id, &tool_id).await
}

#[tauri::command]
pub async fn toggle_mcp_tool(
    app: AppHandle,
    server_id: String,
    tool_id: String,
    active: bool,
) -> Result<ToolResponse, String> {
    mcp_config::toggle_tool(&pool(&app)?, &server_id, &tool_id, active).await
}

#[tauri::command]
pub async fn get_mcp_connection_info(
    app: AppHandle,
    server_id: String,
) -> Result<ConnectionInfo, String> {
    mcp_config::connection_info(&pool(&app)?, &server_id, mcp_port(&app)).await
}
