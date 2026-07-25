//! Localhost-only MCP streamable-HTTP endpoint (Axum + rmcp).
//!
//! Binds `127.0.0.1:18651` (or an ephemeral fallback) and serves a single
//! `/mcp` route. Clients pass `?server_id=<server_name>` to scope dynamic tools.

mod format;
mod handler;
mod server_id;

pub use handler::{McpAppState, McpHandler};
pub use server_id::{extract_server_id, ServerId};

use std::net::SocketAddr;
use std::sync::Arc;

use axum::middleware::from_fn;
use axum::Router;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::services::rag_service::RagService;

pub const DEFAULT_MCP_PORT: u16 = 18651;

/// Managed Tauri state for the live MCP listener.
#[derive(Clone)]
pub struct McpState {
    pub port: u16,
    pub cancel: CancellationToken,
}

/// Bind and serve the MCP endpoint. Returns the effective port and a join handle.
pub async fn start(
    state: Arc<McpAppState>,
    cancel: CancellationToken,
) -> Result<(u16, tokio::task::JoinHandle<()>), String> {
    let port = pick_port()?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("binding MCP listener on {addr}: {e}"))?;

    let factory_state = state.clone();
    let service = StreamableHttpService::new(
        move || Ok(McpHandler::new(factory_state.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_stateful_mode(false)
            .with_json_response(true)
            .with_cancellation_token(cancel.child_token()),
    );

    let app = Router::new()
        .nest_service("/mcp", service)
        .layer(from_fn(server_id::extract_server_id));

    let shutdown = cancel.clone();
    let handle = tokio::spawn(async move {
        let serve = axum::serve(listener, app).with_graceful_shutdown(async move {
            shutdown.cancelled().await;
        });
        if let Err(e) = serve.await {
            error!("MCP server error: {e}");
        }
    });

    info!("MCP endpoint listening on http://127.0.0.1:{port}/mcp");
    Ok((port, handle))
}

fn pick_port() -> Result<u16, String> {
    let try_addr = SocketAddr::from(([127, 0, 0, 1], DEFAULT_MCP_PORT));
    match std::net::TcpListener::bind(try_addr) {
        Ok(listener) => {
            let port = listener
                .local_addr()
                .map_err(|e| format!("reading MCP bind addr: {e}"))?
                .port();
            drop(listener);
            Ok(port)
        }
        Err(_) => portpicker::pick_unused_port()
            .ok_or_else(|| "no free port available for MCP endpoint".to_string()),
    }
}

impl McpAppState {
    pub fn new(pool: sqlx::SqlitePool, rag: Arc<RagService>) -> Self {
        Self { pool, rag }
    }
}
