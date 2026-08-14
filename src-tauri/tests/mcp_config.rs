use mcp_nano_lib::models::request::{ScopePayload, ToolPayload};
use mcp_nano_lib::services::mcp_config;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

async fn test_pool() -> SqlitePool {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("app.db");
    // Keep tempdir alive for the pool lifetime.
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

#[tokio::test]
async fn server_and_tool_crud() {
    let pool = test_pool().await;

    let created = mcp_config::create_server(&pool, "My_Server".into(), Some("desc".into()))
        .await
        .expect("create server");
    assert_eq!(created.server.name, "My_Server");
    assert!(created.server.active);

    let server_id = created.server.id.clone();
    let server_name = created.server.name.clone();
    let listed = mcp_config::list_servers(&pool).await.expect("list");
    assert_eq!(listed.servers.len(), 1);

    let tool = mcp_config::create_tool(
        &pool,
        &server_id,
        ToolPayload {
            name: "search_code".into(),
            description: Some("Search code".into()),
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
    .expect("create tool");
    assert_eq!(tool.tool.name, "search_code");
    assert_eq!(tool.tool.code_search_scopes.len(), 1);
    assert_eq!(tool.tool.code_search_scopes[0].repo_names, vec!["mcp-nano"]);

    let active = mcp_config::list_active_tools(&pool, Some(&server_name))
        .await
        .expect("active tools");
    assert_eq!(active.len(), 1);

    let toggled = mcp_config::toggle_tool(&pool, &server_id, &tool.tool.id, false)
        .await
        .expect("toggle");
    assert!(!toggled.tool.active);

    let active2 = mcp_config::list_active_tools(&pool, Some(&server_name))
        .await
        .expect("active after toggle");
    assert!(active2.is_empty());

    let info = mcp_config::connection_info(&pool, &server_id, 18651)
        .await
        .expect("connection info");
    assert_eq!(
        info.full_url,
        "http://127.0.0.1:18651/mcp?server_id=My_Server"
    );
    assert_eq!(info.server_id, "My_Server");

    let dup = mcp_config::create_server(&pool, "My_Server".into(), None).await;
    assert!(dup.is_err());

    mcp_config::delete_server(&pool, &server_id)
        .await
        .expect("delete");
    assert!(mcp_config::list_servers(&pool).await.unwrap().servers.is_empty());
}

#[tokio::test]
async fn find_tool_by_name_respects_server_id() {
    let pool = test_pool().await;
    let s1 = mcp_config::create_server(&pool, "s1".into(), None)
        .await
        .unwrap();
    let s2 = mcp_config::create_server(&pool, "s2".into(), None)
        .await
        .unwrap();

    mcp_config::create_tool(
        &pool,
        &s1.server.id,
        ToolPayload {
            name: "shared_name".into(),
            description: Some("a".into()),
            code_search_scopes: vec![],
            document_search_scopes: vec![],
            max_chunk_limit: None,
        },
    )
    .await
    .unwrap();
    mcp_config::create_tool(
        &pool,
        &s2.server.id,
        ToolPayload {
            name: "shared_name".into(),
            description: Some("b".into()),
            code_search_scopes: vec![],
            document_search_scopes: vec![],
            max_chunk_limit: None,
        },
    )
    .await
    .unwrap();

    let t1 = mcp_config::find_active_tool_by_name(&pool, Some(&s1.server.name), "shared_name")
        .await
        .unwrap()
        .expect("tool on s1");
    assert_eq!(t1.description.as_deref(), Some("a"));

    let t2 = mcp_config::find_active_tool_by_name(&pool, Some(&s2.server.name), "shared_name")
        .await
        .unwrap()
        .expect("tool on s2");
    assert_eq!(t2.description.as_deref(), Some("b"));
}
