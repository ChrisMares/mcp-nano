//! MCP streamable-HTTP endpoint: list_tools filtered by server name.

mod common;

use std::sync::Arc;
use std::time::Duration;

use mcp_nano_lib::mcp::{extract_server_id, McpAppState, McpHandler};
use mcp_nano_lib::models::request::{ScopePayload, ToolPayload};
use mcp_nano_lib::services::mcp_config;
use mcp_nano_lib::services::rag_service::RagService;
use mcp_nano_lib::services::QdrantService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

async fn memory_pool() -> SqlitePool {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("app.db");
    std::mem::forget(dir);
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .expect("connect");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrate");
    pool
}

async fn build_rag_if_possible() -> Option<(Arc<RagService>, common::ChildGuard)> {
    let embedders = common::load_embedders()?;
    let (client, child) = common::spawn_qdrant().await?;
    let rag = Arc::new(RagService::new(embedders, QdrantService::new(client)));
    Some((rag, child))
}

#[tokio::test]
async fn list_tools_filtered_by_server_id() {
    let pool = memory_pool().await;
    let server = mcp_config::create_server(&pool, "code_search".into(), None)
        .await
        .unwrap();
    mcp_config::create_tool(
        &pool,
        &server.server.id,
        ToolPayload {
            name: "search_my_code".into(),
            description: Some("Search the codebase".into()),
            code_search_scopes: vec![ScopePayload {
                collection: "codebase".into(),
                repo_names: Some(vec!["mcp-nano".into()]),
                group_ids: None,
            }],
            document_search_scopes: vec![],
            max_chunk_limit: None,
        },
    )
    .await
    .unwrap();

    let other = mcp_config::create_server(&pool, "other".into(), None)
        .await
        .unwrap();
    mcp_config::create_tool(
        &pool,
        &other.server.id,
        ToolPayload {
            name: "other_tool".into(),
            description: Some("other".into()),
            code_search_scopes: vec![],
            document_search_scopes: vec![],
            max_chunk_limit: None,
        },
    )
    .await
    .unwrap();

    let tools = mcp_config::list_active_tools(&pool, Some(&server.server.name))
        .await
        .unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "search_my_code");

    let all = mcp_config::list_active_tools(&pool, None).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn mcp_http_list_tools_via_streamable_http() {
    let Some((rag, _child)) = build_rag_if_possible().await else {
        eprintln!("skipping mcp_http_list_tools: models/qdrant unavailable");
        return;
    };

    let pool = memory_pool().await;
    let server = mcp_config::create_server(&pool, "http_server".into(), None)
        .await
        .unwrap();
    let server_id = server.server.id.clone();
    let server_name = server.server.name.clone();
    mcp_config::create_tool(
        &pool,
        &server_id,
        ToolPayload {
            name: "http_search".into(),
            description: Some("HTTP exposed tool".into()),
            code_search_scopes: vec![],
            document_search_scopes: vec![],
            max_chunk_limit: None,
        },
    )
    .await
    .unwrap();

    let state = Arc::new(McpAppState::new(pool, rag));
    let ct = CancellationToken::new();
    let service: StreamableHttpService<McpHandler, LocalSessionManager> =
        StreamableHttpService::new(
            {
                let state = state.clone();
                move || Ok(McpHandler::new(state.clone()))
            },
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default()
                .with_stateful_mode(false)
                .with_json_response(true)
                .with_sse_keep_alive(None)
                .with_cancellation_token(ct.child_token()),
        );

    let app = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(axum::middleware::from_fn(extract_server_id));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let serve_ct = ct.clone();
    let serve = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move { serve_ct.cancelled().await })
            .await;
    });

    tokio::time::sleep(Duration::from_millis(80)).await;

    let url = format!("http://{addr}/mcp?server_id={server_name}");
    let transport = StreamableHttpClientTransport::from_uri(url);
    let client = ().serve(transport).await.expect("mcp client connect");

    let tools = client.list_all_tools().await.expect("list tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name.as_ref(), "http_search");

    let _ = client.cancel().await;
    ct.cancel();
    let _ = serve.await;
}
