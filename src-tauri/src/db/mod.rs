use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};
use tracing::info;

pub struct DbState {
    pub pool: SqlitePool,
}

pub fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("failed to resolve application data directory: {error}"))?;
    std::fs::create_dir_all(&data_dir)
        .map_err(|error| format!("failed to create application data directory: {error}"))?;
    Ok(data_dir.join("app.db"))
}

const MAX_DATABASE_BACKUPS: usize = 5;

fn backup_existing_database(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    let parent = path
        .parent()
        .ok_or_else(|| format!("database has no parent directory: {}", path.display()))?;
    let backup_dir = parent.join("backups");
    fs::create_dir_all(&backup_dir)
        .map_err(|error| format!("failed to create database backup directory: {error}"))?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before Unix epoch: {error}"))?
        .as_millis();
    let backup_name = format!(
        "app-{}-before-migration-{timestamp}.db",
        env!("CARGO_PKG_VERSION")
    );
    let backup_path = backup_dir.join(backup_name);
    fs::copy(path, &backup_path)
        .map_err(|error| format!("failed to back up SQLite database: {error}"))?;

    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{}", path.display(), suffix));
        if sidecar.exists() {
            let backup_sidecar = PathBuf::from(format!("{}{}", backup_path.display(), suffix));
            fs::copy(&sidecar, backup_sidecar)
                .map_err(|error| format!("failed to back up SQLite {suffix} file: {error}"))?;
        }
    }

    let mut backups: Vec<PathBuf> = fs::read_dir(&backup_dir)
        .map_err(|error| format!("failed to list database backups: {error}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|candidate| {
            candidate
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("app-")
                        && name.contains("-before-migration-")
                        && name.ends_with(".db")
                })
        })
        .collect();
    backups.sort();
    while backups.len() > MAX_DATABASE_BACKUPS {
        let old = backups.remove(0);
        let _ = fs::remove_file(&old);
        for suffix in ["-wal", "-shm"] {
            let sidecar = PathBuf::from(format!("{}{}", old.display(), suffix));
            let _ = fs::remove_file(sidecar);
        }
    }

    info!("SQLite migration backup created at {}", backup_path.display());
    Ok(())
}

pub async fn init(app: AppHandle) -> Result<(), String> {
    let path = db_path(&app)?;
    backup_existing_database(&path)?;
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

    #[test]
    fn backup_existing_database_copies_database_and_sidecars() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("app.db");
        std::fs::write(&path, b"database").expect("write database");
        std::fs::write(dir.path().join("app.db-wal"), b"wal").expect("write wal");

        backup_existing_database(&path).expect("backup database");

        let backups = std::fs::read_dir(dir.path().join("backups"))
            .expect("read backups")
            .map(|entry| entry.expect("backup entry").path())
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 2);
        assert!(backups.iter().any(|backup| {
            backup
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".db"))
        }));
        assert!(backups.iter().any(|backup| {
            backup
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".db-wal"))
        }));
    }

    #[tokio::test]
    async fn rerunning_migrations_preserves_existing_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("app.db");
        let pool = connect(&path).await.expect("initial migration");
        sqlx::query(
            "INSERT INTO mcp_servers (id, name, description, active, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind("server-id")
        .bind("existing-server")
        .bind("description")
        .bind(true)
        .bind("2026-01-01T00:00:00Z")
        .bind("2026-01-01T00:00:00Z")
        .execute(&pool)
        .await
        .expect("insert existing row");
        pool.close().await;

        let pool = connect(&path).await.expect("rerun migrations");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mcp_servers")
            .fetch_one(&pool)
            .await
            .expect("count existing rows");
        assert_eq!(count, 1);
    }
}
