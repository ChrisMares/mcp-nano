//! SQLite CRUD for MCP servers/tools. Shared by Tauri commands and the MCP HTTP handler.

use chrono::Utc;
use serde_json::json;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::entities::{
    McpServer, ToolCodeSearchScope, ToolDefinition, ToolDocumentSearchScope,
};
use crate::models::request::ToolPayload;
use crate::models::response::{
    ConnectionInfo, MessageResponse, ServerResponse, ServersResponse, ToolResponse,
};

fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
}

const DEFAULT_MAX_CHUNK_LIMIT: i32 = 5;
const MIN_MAX_CHUNK_LIMIT: i32 = 1;
const MAX_MAX_CHUNK_LIMIT: i32 = 50;

fn normalize_max_chunk_limit(value: Option<i32>) -> i32 {
    value
        .unwrap_or(DEFAULT_MAX_CHUNK_LIMIT)
        .clamp(MIN_MAX_CHUNK_LIMIT, MAX_MAX_CHUNK_LIMIT)
}

fn validate_server_name(name: &str) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Server name is required".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err("Only letters, numbers, and underscores allowed".into());
    }
    Ok(name)
}

async fn ensure_unique_server_name(pool: &SqlitePool, name: &str) -> Result<(), String> {
    let existing =
        sqlx::query_scalar::<_, String>("SELECT id FROM mcp_servers WHERE name = ? LIMIT 1")
            .bind(name)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("checking server name uniqueness: {e}"))?;

    if existing.is_some() {
        return Err(format!("Server name already exists: {name}"));
    }
    Ok(())
}

pub async fn list_servers(pool: &SqlitePool) -> Result<ServersResponse, String> {
    let mut servers = sqlx::query_as::<_, McpServer>(
        "SELECT id, name, description, active, created_at, updated_at FROM mcp_servers ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("listing mcp servers: {e}"))?;

    for server in &mut servers {
        server.tools = Some(load_tools_for_server(pool, &server.id).await?);
    }
    Ok(ServersResponse { servers })
}

pub async fn create_server(
    pool: &SqlitePool,
    name: String,
    description: Option<String>,
) -> Result<ServerResponse, String> {
    let name = validate_server_name(&name)?;
    ensure_unique_server_name(pool, &name).await?;
    let id = Uuid::new_v4().to_string();
    let now = now_iso();
    sqlx::query(
        "INSERT INTO mcp_servers (id, name, description, active, created_at, updated_at) VALUES (?, ?, ?, 1, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&description)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| format!("creating mcp server: {e}"))?;

    Ok(ServerResponse {
        server: McpServer {
            id,
            name,
            description,
            active: true,
            created_at: Some(now.clone()),
            updated_at: Some(now),
            tools: Some(vec![]),
        },
    })
}

pub async fn get_server(pool: &SqlitePool, server_id: &str) -> Result<ServerResponse, String> {
    let mut server = get_server_row(pool, server_id).await?;
    server.tools = Some(load_tools_for_server(pool, server_id).await?);
    Ok(ServerResponse { server })
}

pub async fn delete_server(pool: &SqlitePool, server_id: &str) -> Result<MessageResponse, String> {
    let _ = get_server_row(pool, server_id).await?;
    let tools = sqlx::query_as::<_, ToolDefinition>(
        "SELECT id, mcp_server_id, name, description, active, created_at, updated_at, max_chunk_limit \
         FROM tool_definitions WHERE mcp_server_id = ?",
    )
    .bind(server_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("listing tools for delete: {e}"))?;

    for tool in &tools {
        delete_tool_scopes(pool, &tool.id).await?;
        sqlx::query("DELETE FROM tool_definitions WHERE id = ?")
            .bind(&tool.id)
            .execute(pool)
            .await
            .map_err(|e| format!("deleting tool: {e}"))?;
    }

    sqlx::query("DELETE FROM mcp_servers WHERE id = ?")
        .bind(server_id)
        .execute(pool)
        .await
        .map_err(|e| format!("deleting mcp server: {e}"))?;

    Ok(MessageResponse {
        message: format!("Server {server_id} deleted"),
    })
}

pub async fn create_tool(
    pool: &SqlitePool,
    server_id: &str,
    tool_data: ToolPayload,
) -> Result<ToolResponse, String> {
    let _ = get_server_row(pool, server_id).await?;
    let name = tool_data.name.trim().to_string();
    if name.is_empty() {
        return Err("Tool name is required".into());
    }
    let id = Uuid::new_v4().to_string();
    let now = now_iso();
    let max_chunk_limit = normalize_max_chunk_limit(tool_data.max_chunk_limit);
    sqlx::query(
        "INSERT INTO tool_definitions (id, name, description, active, mcp_server_id, created_at, updated_at, max_chunk_limit) \
         VALUES (?, ?, ?, 1, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&tool_data.description)
    .bind(server_id)
    .bind(&now)
    .bind(&now)
    .bind(max_chunk_limit)
    .execute(pool)
    .await
    .map_err(|e| format!("creating tool: {e}"))?;

    let (code_scopes, doc_scopes) = replace_tool_scopes(pool, &id, &tool_data).await?;
    Ok(ToolResponse {
        tool: ToolDefinition {
            id,
            mcp_server_id: server_id.to_string(),
            name,
            description: tool_data.description,
            active: true,
            created_at: Some(now.clone()),
            updated_at: Some(now),
            max_chunk_limit: Some(max_chunk_limit),
            code_search_scopes: code_scopes,
            document_search_scopes: doc_scopes,
        },
    })
}

pub async fn update_tool(
    pool: &SqlitePool,
    server_id: &str,
    tool_id: &str,
    tool_data: ToolPayload,
) -> Result<ToolResponse, String> {
    let mut tool = get_tool_row(pool, server_id, tool_id).await?;
    let name = tool_data.name.trim().to_string();
    if name.is_empty() {
        return Err("Tool name is required".into());
    }
    let now = now_iso();
    let max_chunk_limit = normalize_max_chunk_limit(tool_data.max_chunk_limit);
    sqlx::query(
        "UPDATE tool_definitions SET name = ?, description = ?, updated_at = ?, max_chunk_limit = ? WHERE id = ?",
    )
    .bind(&name)
    .bind(&tool_data.description)
    .bind(&now)
    .bind(max_chunk_limit)
    .bind(tool_id)
    .execute(pool)
    .await
    .map_err(|e| format!("updating tool: {e}"))?;

    let (code_scopes, doc_scopes) = replace_tool_scopes(pool, tool_id, &tool_data).await?;
    tool.name = name;
    tool.description = tool_data.description;
    tool.updated_at = Some(now);
    tool.max_chunk_limit = Some(max_chunk_limit);
    tool.code_search_scopes = code_scopes;
    tool.document_search_scopes = doc_scopes;
    Ok(ToolResponse { tool })
}

pub async fn delete_tool(
    pool: &SqlitePool,
    server_id: &str,
    tool_id: &str,
) -> Result<MessageResponse, String> {
    let _ = get_tool_row(pool, server_id, tool_id).await?;
    delete_tool_scopes(pool, tool_id).await?;
    sqlx::query("DELETE FROM tool_definitions WHERE id = ?")
        .bind(tool_id)
        .execute(pool)
        .await
        .map_err(|e| format!("deleting tool: {e}"))?;
    Ok(MessageResponse {
        message: format!("Tool {tool_id} deleted"),
    })
}

pub async fn toggle_tool(
    pool: &SqlitePool,
    server_id: &str,
    tool_id: &str,
    active: bool,
) -> Result<ToolResponse, String> {
    let mut tool = get_tool_row(pool, server_id, tool_id).await?;
    let now = now_iso();
    sqlx::query("UPDATE tool_definitions SET active = ?, updated_at = ? WHERE id = ?")
        .bind(active)
        .bind(&now)
        .bind(tool_id)
        .execute(pool)
        .await
        .map_err(|e| format!("toggling tool: {e}"))?;

    let (code_scopes, doc_scopes) = load_scopes(pool, tool_id).await?;
    tool.active = active;
    tool.updated_at = Some(now);
    tool.code_search_scopes = code_scopes;
    tool.document_search_scopes = doc_scopes;
    Ok(ToolResponse { tool })
}

pub async fn connection_info(
    pool: &SqlitePool,
    server_id: &str,
    port: u16,
) -> Result<ConnectionInfo, String> {
    let server = get_server_row(pool, server_id).await?;
    let mcp_url = format!("http://127.0.0.1:{port}/mcp");
    let full_url = format!("{mcp_url}?server_id={}", server.name);
    Ok(ConnectionInfo {
        mcp_url,
        server_id: server.name.clone(),
        server_name: server.name.clone(),
        full_url: full_url.clone(),
        config_snippets: json!({
            "claude_desktop": { "mcpServers": { server.name.clone(): { "url": full_url } } },
            "opencode": { "mcp": { server.name.clone(): { "type": "remote", "url": full_url } } },
            "vscode": { "servers": { server.name.clone(): { "type": "http", "url": full_url } } },
        }),
    })
}

/// Active tools for an optional MCP server name (`?server_id=` query value).
pub async fn list_active_tools(
    pool: &SqlitePool,
    server_name: Option<&str>,
) -> Result<Vec<ToolDefinition>, String> {
    let tools = if let Some(name) = server_name {
        sqlx::query_as::<_, ToolDefinition>(
            "SELECT t.id, t.mcp_server_id, t.name, t.description, t.active, t.created_at, t.updated_at, t.max_chunk_limit \
             FROM tool_definitions t \
             JOIN mcp_servers s ON s.id = t.mcp_server_id \
             WHERE t.active = 1 AND s.active = 1 AND s.name = ? \
             ORDER BY t.created_at ASC",
        )
        .bind(name)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, ToolDefinition>(
            "SELECT t.id, t.mcp_server_id, t.name, t.description, t.active, t.created_at, t.updated_at, t.max_chunk_limit \
             FROM tool_definitions t \
             JOIN mcp_servers s ON s.id = t.mcp_server_id \
             WHERE t.active = 1 AND s.active = 1 \
             ORDER BY t.created_at ASC",
        )
        .fetch_all(pool)
        .await
    }
    .map_err(|e| format!("listing active tools: {e}"))?;

    let mut out = Vec::with_capacity(tools.len());
    for mut tool in tools {
        let (code, doc) = load_scopes(pool, &tool.id).await?;
        tool.code_search_scopes = code;
        tool.document_search_scopes = doc;
        out.push(tool);
    }
    Ok(out)
}

pub async fn find_active_tool_by_name(
    pool: &SqlitePool,
    server_name: Option<&str>,
    name: &str,
) -> Result<Option<ToolDefinition>, String> {
    let tools = list_active_tools(pool, server_name).await?;
    Ok(tools.into_iter().find(|t| t.name == name))
}

/// Build one RAG request payload for a scope: equality filter for a single
/// value, `$or` for several, empty filter for none.
fn scope_request(
    collection: &str,
    default_collection: &str,
    filter_key: &str,
    values: &[String],
) -> serde_json::Value {
    let values: Vec<&str> = values
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .collect();
    let where_clause = match values.as_slice() {
        [] => json!({}),
        [one] => json!({ filter_key: one }),
        many => json!({
            "$or": many.iter().map(|v| json!({ filter_key: v })).collect::<Vec<_>>()
        }),
    };
    json!({
        "collection": if collection.is_empty() { default_collection } else { collection },
        "where": where_clause,
    })
}

/// Build per-collection RAG request payloads from a tool's scopes.
/// Mirrors Python `_collection_requests` in `protocol.py`.
pub fn collection_requests(tool: &ToolDefinition) -> Vec<serde_json::Value> {
    let mut requests: Vec<serde_json::Value> = tool
        .code_search_scopes
        .iter()
        .map(|s| scope_request(&s.collection, "codebase", "repo_name", &s.repo_names))
        .chain(tool.document_search_scopes.iter().map(|s| {
            scope_request(&s.collection, "general", "group", &s.group_ids)
        }))
        .collect();

    if requests.is_empty() {
        requests.push(json!({ "collection": "codebase", "where": {} }));
    }
    requests
}

async fn get_server_row(pool: &SqlitePool, server_id: &str) -> Result<McpServer, String> {
    sqlx::query_as::<_, McpServer>(
        "SELECT id, name, description, active, created_at, updated_at FROM mcp_servers WHERE id = ?",
    )
    .bind(server_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("fetching mcp server: {e}"))?
    .ok_or_else(|| format!("Server not found: {server_id}"))
}

async fn get_tool_row(
    pool: &SqlitePool,
    server_id: &str,
    tool_id: &str,
) -> Result<ToolDefinition, String> {
    sqlx::query_as::<_, ToolDefinition>(
        "SELECT id, mcp_server_id, name, description, active, created_at, updated_at, max_chunk_limit \
         FROM tool_definitions WHERE id = ? AND mcp_server_id = ?",
    )
    .bind(tool_id)
    .bind(server_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("fetching tool: {e}"))?
    .ok_or_else(|| format!("Tool not found: {tool_id}"))
}

async fn load_tools_for_server(
    pool: &SqlitePool,
    server_id: &str,
) -> Result<Vec<ToolDefinition>, String> {
    let tools = sqlx::query_as::<_, ToolDefinition>(
        "SELECT id, mcp_server_id, name, description, active, created_at, updated_at, max_chunk_limit \
         FROM tool_definitions WHERE mcp_server_id = ? ORDER BY created_at ASC",
    )
    .bind(server_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("listing tools: {e}"))?;

    let mut out = Vec::with_capacity(tools.len());
    for mut tool in tools {
        let (code, doc) = load_scopes(pool, &tool.id).await?;
        tool.code_search_scopes = code;
        tool.document_search_scopes = doc;
        out.push(tool);
    }
    Ok(out)
}

async fn load_scopes(
    pool: &SqlitePool,
    tool_id: &str,
) -> Result<(Vec<ToolCodeSearchScope>, Vec<ToolDocumentSearchScope>), String> {
    let code = sqlx::query_as::<_, ToolCodeSearchScope>(
        "SELECT id, tool_definition_id, collection, repo_names FROM tool_code_search WHERE tool_definition_id = ?",
    )
    .bind(tool_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("loading code scopes: {e}"))?;

    let doc = sqlx::query_as::<_, ToolDocumentSearchScope>(
        "SELECT id, tool_definition_id, collection, group_ids FROM tool_document_search WHERE tool_definition_id = ?",
    )
    .bind(tool_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("loading document scopes: {e}"))?;

    Ok((code, doc))
}

async fn delete_tool_scopes(pool: &SqlitePool, tool_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM tool_code_search WHERE tool_definition_id = ?")
        .bind(tool_id)
        .execute(pool)
        .await
        .map_err(|e| format!("deleting code scopes: {e}"))?;
    sqlx::query("DELETE FROM tool_document_search WHERE tool_definition_id = ?")
        .bind(tool_id)
        .execute(pool)
        .await
        .map_err(|e| format!("deleting document scopes: {e}"))?;
    Ok(())
}

async fn replace_tool_scopes(
    pool: &SqlitePool,
    tool_id: &str,
    payload: &ToolPayload,
) -> Result<(Vec<ToolCodeSearchScope>, Vec<ToolDocumentSearchScope>), String> {
    delete_tool_scopes(pool, tool_id).await?;
    let now = now_iso();
    let mut code_scopes = Vec::new();
    for scope in &payload.code_search_scopes {
        let id = Uuid::new_v4().to_string();
        let repo_names = scope.repo_names.clone().unwrap_or_default();
        let repo_json =
            serde_json::to_string(&repo_names).map_err(|e| format!("serialize repo_names: {e}"))?;
        sqlx::query(
            "INSERT INTO tool_code_search (id, tool_definition_id, collection, repo_names, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(tool_id)
        .bind(&scope.collection)
        .bind(&repo_json)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(|e| format!("inserting code scope: {e}"))?;
        code_scopes.push(ToolCodeSearchScope {
            id,
            tool_definition_id: tool_id.to_string(),
            collection: scope.collection.clone(),
            repo_names,
        });
    }

    let mut doc_scopes = Vec::new();
    for scope in &payload.document_search_scopes {
        let id = Uuid::new_v4().to_string();
        let group_ids = scope.group_ids.clone().unwrap_or_default();
        let group_json =
            serde_json::to_string(&group_ids).map_err(|e| format!("serialize group_ids: {e}"))?;
        sqlx::query(
            "INSERT INTO tool_document_search (id, tool_definition_id, collection, group_ids, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(tool_id)
        .bind(&scope.collection)
        .bind(&group_json)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(|e| format!("inserting document scope: {e}"))?;
        doc_scopes.push(ToolDocumentSearchScope {
            id,
            tool_definition_id: tool_id.to_string(),
            collection: scope.collection.clone(),
            group_ids,
        });
    }

    Ok((code_scopes, doc_scopes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::entities::ToolCodeSearchScope;

    #[test]
    fn collection_requests_empty_defaults_to_codebase() {
        let tool = ToolDefinition::default();
        let reqs = collection_requests(&tool);
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0]["collection"], "codebase");
        assert_eq!(reqs[0]["where"], json!({}));
    }

    #[test]
    fn collection_requests_single_repo() {
        let tool = ToolDefinition {
            code_search_scopes: vec![ToolCodeSearchScope {
                id: "1".into(),
                tool_definition_id: "t".into(),
                collection: "codebase".into(),
                repo_names: vec!["mcp-nano".into()],
            }],
            ..Default::default()
        };
        let reqs = collection_requests(&tool);
        assert_eq!(reqs[0]["where"], json!({ "repo_name": "mcp-nano" }));
    }

    #[test]
    fn collection_requests_multi_repo_or() {
        let tool = ToolDefinition {
            code_search_scopes: vec![ToolCodeSearchScope {
                id: "1".into(),
                tool_definition_id: "t".into(),
                collection: "codebase".into(),
                repo_names: vec!["a".into(), "b".into()],
            }],
            ..Default::default()
        };
        let reqs = collection_requests(&tool);
        assert_eq!(
            reqs[0]["where"],
            json!({ "$or": [{ "repo_name": "a" }, { "repo_name": "b" }] })
        );
    }

    #[test]
    fn collection_requests_mixed_scopes_preserve_order() {
        let tool = ToolDefinition {
            code_search_scopes: vec![ToolCodeSearchScope {
                id: "1".into(),
                tool_definition_id: "t".into(),
                collection: "codebase".into(),
                repo_names: vec!["r".into()],
            }],
            document_search_scopes: vec![crate::models::entities::ToolDocumentSearchScope {
                id: "2".into(),
                tool_definition_id: "t".into(),
                collection: "general".into(),
                group_ids: vec!["g".into()],
            }],
            ..Default::default()
        };
        let reqs = collection_requests(&tool);
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0]["collection"], "codebase");
        assert_eq!(reqs[0]["where"], json!({ "repo_name": "r" }));
        assert_eq!(reqs[1]["collection"], "general");
        assert_eq!(reqs[1]["where"], json!({ "group": "g" }));
    }

    #[test]
    fn collection_requests_skips_empty_scope_values() {
        let tool = ToolDefinition {
            code_search_scopes: vec![ToolCodeSearchScope {
                id: "1".into(),
                tool_definition_id: "t".into(),
                collection: String::new(),
                repo_names: vec![String::new()],
            }],
            ..Default::default()
        };
        let reqs = collection_requests(&tool);
        // Empty collection falls back to codebase; empty values mean no filter.
        assert_eq!(reqs[0]["collection"], "codebase");
        assert_eq!(reqs[0]["where"], json!({}));
    }

    #[test]
    fn validate_server_name_trims_and_accepts() {
        assert_eq!(validate_server_name("  My_Server1 ").unwrap(), "My_Server1");
    }

    #[test]
    fn validate_server_name_rejects_empty() {
        assert!(validate_server_name("").is_err());
        assert!(validate_server_name("   ").is_err());
    }

    #[test]
    fn validate_server_name_rejects_invalid_chars() {
        for bad in ["has space", "dash-name", "dot.name", "slash/name", "emoji🦀"] {
            assert!(validate_server_name(bad).is_err(), "{bad} should fail");
        }
    }
}
