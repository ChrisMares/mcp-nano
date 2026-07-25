use std::path::PathBuf;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};
use tracing::info;

pub struct DbState {
    pub pool: SqlitePool,
}

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("failed to resolve application data directory: {error}"))?;
    std::fs::create_dir_all(&data_dir)
        .map_err(|error| format!("failed to create application data directory: {error}"))?;
    Ok(data_dir.join("app.db"))
}

pub async fn init(app: AppHandle) -> Result<(), String> {
    let path = db_path(&app)?;
    let pool = connect(&path).await?;
    app.manage(DbState { pool });
    info!("SQLite initialized at {}", path.display());
    Ok(())
}

async fn connect(path: &PathBuf) -> Result<SqlitePool, String> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .map_err(|error| format!("failed to open SQLite database {}: {error}", path.display()))?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|error| format!("failed to run SQLite migrations: {error}"))?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;

    #[tokio::test]
    async fn migrations_create_expected_schema() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("app.db");
        let pool = connect(&path).await.expect("connect and migrate");

        let tables: Vec<String> = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .expect("list tables")
        .into_iter()
        .map(|row| row.get(0))
        .collect();
        assert_eq!(
            tables,
            vec![
                "file_metadata",
                "job_status",
                "mcp_servers",
                "tool_code_search",
                "tool_definitions",
                "tool_document_search",
            ]
        );

        let indexes: Vec<String> = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .expect("list indexes")
        .into_iter()
        .map(|row| row.get(0))
        .collect();
        assert_eq!(
            indexes,
            vec![
                "idx_file_metadata_status_collection_created_at",
                "ix_file_metadata_status",
                "ix_tool_code_search_tool_definition_id",
                "ix_tool_definitions_mcp_server_id",
                "ix_tool_document_search_tool_definition_id",
                "ux_mcp_servers_name",
            ]
        );

        for (table, query) in [
            ("file_metadata", "PRAGMA table_info(file_metadata)"),
            ("job_status", "PRAGMA table_info(job_status)"),
            ("mcp_servers", "PRAGMA table_info(mcp_servers)"),
            ("tool_definitions", "PRAGMA table_info(tool_definitions)"),
            ("tool_code_search", "PRAGMA table_info(tool_code_search)"),
            ("tool_document_search", "PRAGMA table_info(tool_document_search)"),
        ] {
            let columns: Vec<String> = sqlx::query(query)
                .fetch_all(&pool)
                .await
                .expect("table info")
                .into_iter()
                .map(|row| row.get("name"))
                .collect();
            assert!(
                !columns.iter().any(|column| {
                    column == "user" || column == "user_id" || column.starts_with("user_")
                }),
                "table {table} must not have user-related columns: {columns:?}"
            );
        }
    }
}
