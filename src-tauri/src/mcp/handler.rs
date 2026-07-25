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

const DEFAULT_LIMIT: i64 = 5;
const MAX_LIMIT: i64 = 10;

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

        let mut limit = args
            .get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(DEFAULT_LIMIT);

        if limit > MAX_LIMIT || limit < 1 {
            limit = DEFAULT_LIMIT;
        }

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
