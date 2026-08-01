use qdrant_client::qdrant::{facet_value::Variant, Condition, Filter};
use tauri::{AppHandle, Manager};

use crate::db::DbState;
use crate::models::entities::FileMetadata;
use crate::models::response::{
    DeleteResponse, FileMetadataDto, UserFilesResponse, WebsitesResponse,
};
use crate::models::{RepoItem, WebsiteItem};
use crate::qdrant::QdrantState;
use crate::services::qdrant_service::QdrantService;
use tracing::error;

fn qdrant_svc(app: &AppHandle) -> Result<QdrantService, String> {
    app.try_state::<QdrantState>()
        .map(|s| QdrantService::new(s.client.clone()))
        .ok_or_else(|| "Qdrant not ready".to_string())
}

fn pool(app: &AppHandle) -> Result<sqlx::SqlitePool, String> {
    app.try_state::<DbState>()
        .map(|s| s.pool.clone())
        .ok_or_else(|| "Database not ready".to_string())
}

#[tauri::command]
pub async fn get_files(app: AppHandle) -> Result<UserFilesResponse, String> {
    let svc = qdrant_svc(&app)?;

    // Best-effort backfill for zips uploaded before default repo_name existed.
    if let Err(e) = svc.repair_empty_repo_names_from_zip("codebase").await {
        error!("get_files: repo_name repair failed: {e:#}");
    }

    let mut repo_names: std::collections::BTreeSet<String> = match svc
        .get_metadata_values_by_key("codebase", "repo_name", None)
        .await
    {
        Ok(names) => names
            .into_iter()
            .filter_map(|v| match v {
                serde_json::Value::String(s) if !s.is_empty() => Some(s),
                _ => None,
            })
            .collect(),
        Err(e) => {
            error!("get_files: Qdrant facet repo_name failed: {e:#}");
            std::collections::BTreeSet::new()
        }
    };

    let pool = pool(&app)?;
    let completed_codebase = sqlx::query_as::<_, FileMetadata>(
        "SELECT * FROM file_metadata WHERE status = 'completed' AND collection = 'codebase'",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("querying file_metadata: {e}"))?;

    // Also surface repos that only exist in SQLite (or pending→completed
    // metadata before Qdrant facet catches up).
    for f in &completed_codebase {
        if let Some(rn) = f.repo_name.as_ref().filter(|s| !s.is_empty()) {
            repo_names.insert(rn.clone());
        }
    }

    let meta_by_repo: std::collections::HashMap<&str, &FileMetadata> = completed_codebase
        .iter()
        .filter_map(|f| f.repo_name.as_ref().map(|n| (n.as_str(), f)))
        .collect();

    let mut enriched_repos: Vec<RepoItem> = repo_names
        .into_iter()
        .map(|repo_name| {
            let mut r = RepoItem {
                repo_name: repo_name.clone(),
                created_at: None,
                storage_object_id: None,
            };
            if let Some(m) = meta_by_repo.get(repo_name.as_str()) {
                r.created_at = m.created_at.clone();
                r.storage_object_id = Some(m.storage_object_id.clone());
            }
            r
        })
        .collect();
    enriched_repos.sort_by(|a, b| a.repo_name.cmp(&b.repo_name));

    let completed_docs = sqlx::query_as::<_, FileMetadata>(
        "SELECT * FROM file_metadata WHERE status = 'completed' AND collection = 'general'",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("querying file_metadata for docs: {e}"))?;

    let mut documents: Vec<FileMetadataDto> = completed_docs
        .iter()
        .filter(|f| f.filename().is_some())
        .map(|f| FileMetadataDto::from_entity(f))
        .collect();
    documents.sort_by(|a, b| {
        b.created_at
            .as_deref()
            .unwrap_or("")
            .cmp(&a.created_at.as_deref().unwrap_or(""))
    });

    Ok(UserFilesResponse {
        repos: enriched_repos,
        documents,
    })
}

#[tauri::command]
pub async fn delete_repo(app: AppHandle, repo_name: String) -> Result<DeleteResponse, String> {
    let svc = qdrant_svc(&app)?;
    let filter = Filter::must([Condition::matches("repo_name", repo_name.clone())]);
    svc.delete_items("codebase", None, Some(filter))
        .await
        .map_err(|e| format!("delete repo vectors: {e:#}"))?;

    let pool = pool(&app)?;
    sqlx::query("DELETE FROM file_metadata WHERE repo_name = ? AND status = 'completed'")
        .bind(&repo_name)
        .execute(&pool)
        .await
        .map_err(|e| format!("deleting file_metadata: {e}"))?;

    Ok(DeleteResponse { deleted: true })
}

#[tauri::command]
pub async fn delete_document(app: AppHandle, filename: String) -> Result<DeleteResponse, String> {
    let svc = qdrant_svc(&app)?;
    let filter = Filter::must([Condition::matches("file_name", filename.clone())]);
    svc.delete_items("general", None, Some(filter))
        .await
        .map_err(|e| format!("delete document vectors: {e:#}"))?;

    let pool = pool(&app)?;
    let rows = sqlx::query_as::<_, FileMetadata>(
        "SELECT * FROM file_metadata WHERE status = 'completed'",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("querying file_metadata: {e}"))?;

    if let Some(row) = rows.iter().find(|f| f.filename() == Some(filename.as_str())) {
        sqlx::query("DELETE FROM file_metadata WHERE storage_object_id = ?")
            .bind(&row.storage_object_id)
            .execute(&pool)
            .await
            .map_err(|e| format!("deleting file_metadata: {e}"))?;
    }

    Ok(DeleteResponse { deleted: true })
}

#[tauri::command]
pub async fn delete_group(app: AppHandle, group_name: String) -> Result<DeleteResponse, String> {
    let svc = qdrant_svc(&app)?;
    let filter = qdrant_client::qdrant::Filter {
        must: vec![Condition::matches("group", group_name.clone())],
        must_not: vec![Condition::matches("doc_type", "website".to_string())],
        ..Default::default()
    };
    svc.delete_items("general", None, Some(filter))
        .await
        .map_err(|e| format!("delete group vectors: {e:#}"))?;

    let pool = pool(&app)?;
    sqlx::query(
        "DELETE FROM file_metadata WHERE group_id = ? AND status = 'completed' AND collection = 'general'",
    )
    .bind(&group_name)
    .execute(&pool)
    .await
    .map_err(|e| format!("deleting file_metadata: {e}"))?;

    Ok(DeleteResponse { deleted: true })
}

#[tauri::command]
pub async fn clear_user_collection(
    app: AppHandle,
    collection_name: String,
) -> Result<DeleteResponse, String> {
    let allowed = ["codebase", "general"];
    if !allowed.contains(&collection_name.as_str()) {
        return Err(format!("collection_name must be one of {:?}", allowed));
    }

    let svc = qdrant_svc(&app)?;
    if collection_name == "general" {
        let filter = Filter {
            must_not: vec![Condition::matches(
                "doc_type",
                "website".to_string(),
            )],
            ..Default::default()
        };
        svc.delete_items(&collection_name, None, Some(filter))
            .await
            .map_err(|e| format!("clear collection vectors: {e:#}"))?;
    } else {
        svc.delete_items(&collection_name, None, Some(Filter::default()))
            .await
            .map_err(|e| format!("clear collection vectors: {e:#}"))?;
    }

    let pool = pool(&app)?;
    sqlx::query("DELETE FROM file_metadata WHERE collection = ?")
        .bind(&collection_name)
        .execute(&pool)
        .await
        .map_err(|e| format!("deleting file_metadata for collection: {e}"))?;

    Ok(DeleteResponse { deleted: true })
}

#[tauri::command]
pub async fn get_websites(app: AppHandle) -> Result<WebsitesResponse, String> {
    use qdrant_client::qdrant::FacetCountsBuilder;

    let state = app
        .try_state::<QdrantState>()
        .ok_or_else(|| "Qdrant not ready".to_string())?;

    let facet_filter = Filter::must([Condition::matches(
        "doc_type",
        "website".to_string(),
    )]);
    let resp = state
        .client
        .facet(
            FacetCountsBuilder::new("general", "website_key")
                .filter(facet_filter)
                .limit(10_000),
        )
        .await
        .map_err(|e| format!("facet website_key: {e}"))?;

    let mut by_url: std::collections::HashMap<String, (String, String, i64, String)> =
        std::collections::HashMap::new();
    for hit in resp.hits {
        let key_str = match hit.value {
            Some(ref fv) => match fv.variant {
                Some(Variant::StringValue(ref s)) => s.clone(),
                _ => continue,
            },
            None => continue,
        };
        if let Ok(parts) = serde_json::from_str::<Vec<String>>(&key_str) {
            if parts.len() >= 3 {
                let url = parts[0].clone();
                let group = parts[1].clone();
                let embedded_at = parts[2].clone();
                let count = hit.count as i64;
                let entry = by_url
                    .entry(url.clone())
                    .or_insert_with(|| (url, group.clone(), 0, embedded_at.clone()));
                entry.2 += count;
                if embedded_at > entry.3 {
                    entry.1 = group.clone();
                    entry.3 = embedded_at.clone();
                }
            }
        }
    }

    let mut websites: Vec<WebsiteItem> = by_url
        .into_values()
        .map(|(url, group, chunk_count, embedded_at)| WebsiteItem {
            url,
            group,
            chunk_count,
            embedded_at,
        })
        .collect();
    websites.sort_by(|a, b| a.url.cmp(&b.url));

    Ok(WebsitesResponse { websites })
}

#[tauri::command]
pub async fn delete_website(app: AppHandle, url: String) -> Result<DeleteResponse, String> {
    let svc = qdrant_svc(&app)?;
    let filter = Filter::must([
        Condition::matches("doc_type", "website".to_string()),
        Condition::matches("url", url),
    ]);
    svc.delete_items("general", None, Some(filter))
        .await
        .map_err(|e| format!("delete website vectors: {e:#}"))?;

    Ok(DeleteResponse { deleted: true })
}

#[tauri::command]
pub async fn delete_website_group(
    app: AppHandle,
    group_name: String,
) -> Result<DeleteResponse, String> {
    let svc = qdrant_svc(&app)?;
    let filter = Filter::must([
        Condition::matches("doc_type", "website".to_string()),
        Condition::matches("group", group_name),
    ]);
    svc.delete_items("general", None, Some(filter))
        .await
        .map_err(|e| format!("delete website group vectors: {e:#}"))?;

    Ok(DeleteResponse { deleted: true })
}

#[tauri::command]
pub async fn clear_websites(app: AppHandle) -> Result<DeleteResponse, String> {
    let svc = qdrant_svc(&app)?;
    let filter = Filter::must([Condition::matches(
        "doc_type",
        "website".to_string(),
    )]);
    svc.delete_items("general", None, Some(filter))
        .await
        .map_err(|e| format!("clear website vectors: {e:#}"))?;

    Ok(DeleteResponse { deleted: true })
}