use crate::models::request::ToolPayload;
use crate::models::response::{
    ConnectionInfo, MessageResponse, ServerResponse, ServersResponse, ToolResponse,
};

#[tauri::command]
pub async fn get_mcp_servers() -> Result<ServersResponse, String> {
    println!("get_mcp_servers");
    Ok(ServersResponse::default())
}

#[tauri::command]
pub async fn create_mcp_server(
    name: String,
    description: Option<String>,
) -> Result<ServerResponse, String> {
    println!("create_mcp_server: name={name}, description={description:?}");
    Ok(ServerResponse::default())
}

#[tauri::command]
pub async fn get_mcp_server(server_id: String) -> Result<ServerResponse, String> {
    println!("get_mcp_server: server_id={server_id}");
    Ok(ServerResponse::default())
}

#[tauri::command]
pub async fn delete_mcp_server(server_id: String) -> Result<MessageResponse, String> {
    println!("delete_mcp_server: server_id={server_id}");
    Ok(MessageResponse::default())
}

#[tauri::command]
pub async fn create_mcp_tool(
    server_id: String,
    tool_data: ToolPayload,
) -> Result<ToolResponse, String> {
    println!("create_mcp_tool: server_id={server_id}, tool_data={tool_data:?}");
    Ok(ToolResponse::default())
}

#[tauri::command]
pub async fn update_mcp_tool(
    server_id: String,
    tool_id: String,
    tool_data: ToolPayload,
) -> Result<ToolResponse, String> {
    println!("update_mcp_tool: server_id={server_id}, tool_id={tool_id}, tool_data={tool_data:?}");
    Ok(ToolResponse::default())
}

#[tauri::command]
pub async fn delete_mcp_tool(server_id: String, tool_id: String) -> Result<MessageResponse, String> {
    println!("delete_mcp_tool: server_id={server_id}, tool_id={tool_id}");
    Ok(MessageResponse::default())
}

#[tauri::command]
pub async fn toggle_mcp_tool(
    server_id: String,
    tool_id: String,
    active: bool,
) -> Result<ToolResponse, String> {
    println!("toggle_mcp_tool: server_id={server_id}, tool_id={tool_id}, active={active}");
    Ok(ToolResponse::default())
}

#[tauri::command]
pub async fn get_mcp_connection_info(server_id: String) -> Result<ConnectionInfo, String> {
    println!("get_mcp_connection_info: server_id={server_id}");
    Ok(ConnectionInfo::default())
}
