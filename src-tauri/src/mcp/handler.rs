use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, Implementation, ListToolsResult,
    PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use sqlx::SqlitePool;

use super::format::format_rag_response;
use super::server_id::{server_id_from_uri, ServerId};
use crate::models::request::RagQueryRequest;
use crate::services::mcp_config;
use crate::services::rag_service::RagService;

/// Fallback result count when the caller doesn't request a limit, and the
/// ceiling used if the tool itself has no configured `max_chunk_limit`.
const DEFAULT_LIMIT: i64 = 5;

/// Shared state for every MCP session handler.
#[derive(Clone)]
pub struct McpAppState {
    pub pool: SqlitePool,
    pub rag: Arc<RagService>,
}

/// Dynamic MCP server: tools are loaded from SQLite per server name (`server_id` query).
#[derive(Clone)]
pub struct McpHandler {
    state: Arc<McpAppState>,
}

impl McpHandler {
    pub fn new(state: Arc<McpAppState>) -> Self {
        Self { state }
    }
}

impl ServerHandler for McpHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "VectorFlow RAG Server",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions("Local RAG search tools scoped by MCP server configuration.")
            .with_protocol_version(ProtocolVersion::V_2025_03_26)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let server_id = resolve_server_id(&context);
        let tools = mcp_config::list_active_tools(&self.state.pool, server_id.as_deref())
            .await
            .map_err(|e| McpError::internal_error(e, None))?;

        let schema = search_input_schema();
        let mcp_tools: Vec<Tool> = tools
            .into_iter()
            .map(|t| {
                Tool::new(
                    t.name,
                    t.description.unwrap_or_default(),
                    Arc::new(schema.clone()),
                )
            })
            .collect();

        Ok(ListToolsResult::with_all_items(mcp_tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let server_id = resolve_server_id(&context);
        let name = request.name.as_ref();

        let tool = match mcp_config::find_active_tool_by_name(
            &self.state.pool,
            server_id.as_deref(),
            name,
        )
        .await
        {
            Ok(Some(t)) => t,
            Ok(None) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                    "Error: tool '{name}' not found for this server"
                ))]));
            }
            Err(e) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                    "Error loading tool: {e}"
                ))]));
            }
        };

        let args = request.arguments.unwrap_or_default();
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let requested_limit = args.get("limit").and_then(|v| v.as_i64());
        let limit = effective_limit(requested_limit, tool.max_chunk_limit);

        let mut requests = Vec::new();
        for req in mcp_config::collection_requests(&tool) {
            let collection = req
                .get("collection")
                .and_then(|v| v.as_str())
                .unwrap_or("codebase")
                .to_string();
            let where_clause = req.get("where").cloned();
            requests.push(RagQueryRequest {
                collection,
                query: query.clone(),
                limit: Some(limit),
                show_documents: Some(false),
                where_clause,
            });
        }

        match self.state.rag.run_rag_query(&requests).await {
            Ok(result) => {
                let text = format_rag_response(&result, &query, limit as usize);
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "Error executing search: {e:#}"
            ))])),
        }
    }
}

fn resolve_server_id(context: &RequestContext<RoleServer>) -> Option<String> {
    if let Some(parts) = context.extensions.get::<axum::http::request::Parts>() {
        if let Some(ServerId(id)) = parts.extensions.get::<ServerId>() {
            return Some(id.clone());
        }
        if let Some(id) = server_id_from_uri(&parts.uri) {
            return Some(id);
        }
    }
    None
}

fn search_input_schema() -> serde_json::Map<String, serde_json::Value> {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Natural language search query"
            },
            "limit": {
                "type": "integer",
                "description": "Maximum number of results to return",
                "default": 5
            }
        },
        "required": ["query"]
    });
    schema.as_object().cloned().unwrap_or_default()
}

/// Resolves the effective result limit for a tool call.
///
/// - `requested`: the `limit` argument the caller/LLM passed to the tool (if any).
/// - `tool_max`: the tool's configured `max_chunk_limit` (set by the admin in the UI).
///
/// The requested limit is always capped at the tool's configured max. If the
/// tool has no configured max (or an invalid/non-positive one, which
/// shouldn't happen given `mcp_config::normalize_max_chunk_limit`, but is
/// handled defensively here), `DEFAULT_LIMIT` is used as the cap instead. If
/// the caller didn't request a limit, or requested an invalid (missing,
/// zero, or negative) value, `DEFAULT_LIMIT` is used as the requested value
/// before capping. This guarantees a sane, positive limit no matter what
/// (or whether) the caller sends anything.
fn effective_limit(requested: Option<i64>, tool_max: Option<i32>) -> i64 {
    let cap = tool_max
        .filter(|v| *v > 0)
        .map(i64::from)
        .unwrap_or(DEFAULT_LIMIT);
    let requested = requested.filter(|v| *v > 0).unwrap_or(DEFAULT_LIMIT);
    requested.min(cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_five_when_nothing_requested_and_no_tool_max() {
        assert_eq!(effective_limit(None, None), 5);
    }

    #[test]
    fn default_is_five_when_nothing_requested_even_with_tool_max_set() {
        // Caller didn't ask for anything specific; default (5) still applies,
        // as long as it fits under the tool's cap.
        assert_eq!(effective_limit(None, Some(20)), 5);
    }

    #[test]
    fn requested_within_tool_max_is_respected() {
        assert_eq!(effective_limit(Some(3), Some(20)), 3);
    }

    #[test]
    fn requested_above_tool_max_is_capped_at_tool_max() {
        assert_eq!(effective_limit(Some(100), Some(20)), 20);
    }

    #[test]
    fn requested_above_tool_max_of_one_is_capped_at_one() {
        assert_eq!(effective_limit(Some(100), Some(1)), 1);
    }

    #[test]
    fn requested_zero_falls_back_to_default_then_capped() {
        assert_eq!(effective_limit(Some(0), Some(20)), 5);
        assert_eq!(effective_limit(Some(0), Some(2)), 2);
    }

    #[test]
    fn requested_negative_falls_back_to_default_then_capped() {
        assert_eq!(effective_limit(Some(-5), Some(20)), 5);
        assert_eq!(effective_limit(Some(-5), Some(2)), 2);
    }

    #[test]
    fn no_tool_max_falls_back_to_default_as_the_cap() {
        // Tool has no configured max_chunk_limit at all (None): default (5)
        // is used as the ceiling, so a large request is capped to 5.
        assert_eq!(effective_limit(Some(100), None), 5);
        assert_eq!(effective_limit(Some(3), None), 3);
    }

    #[test]
    fn invalid_stored_tool_max_falls_back_to_default_defensively() {
        // Shouldn't happen in practice (mcp_config::normalize_max_chunk_limit
        // clamps to [1, 50] on save), but the handler should never panic or
        // produce a non-positive limit if it ever does.
        assert_eq!(effective_limit(Some(100), Some(0)), 5);
        assert_eq!(effective_limit(Some(100), Some(-10)), 5);
    }

    #[test]
    fn everything_missing_or_invalid_still_returns_positive_default() {
        assert_eq!(effective_limit(None, Some(0)), 5);
        assert_eq!(effective_limit(Some(0), None), 5);
        assert_eq!(effective_limit(Some(-1), Some(-1)), 5);
    }
}
