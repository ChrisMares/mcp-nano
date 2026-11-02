use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::models::request::RagQueryRequest;
use crate::models::response::{BackendStatusResponse, MetadataValuesResponse, RagResponse};
use crate::qdrant::{BackendStatus, BackendStatusState, QdrantState};
use crate::services::qdrant_service::QdrantService;
use crate::services::rag::RagService;

#[tauri::command]
pub async fn rag_query(app: AppHandle, payload: RagQueryRequest) -> Result<RagResponse, String> {
    let rag = app
        .try_state::<Arc<RagService>>()
        .ok_or_else(|| "rag service not ready".to_string())?
        .inner()
        .clone();
    rag.run_rag_query(&[payload])
        .await
        .map_err(|e| format!("rag query failed: {e:#}"))
}

/// Current backend readiness plus storage/log locations.
#[tauri::command]
pub async fn get_backend_status(app: AppHandle) -> Result<BackendStatusResponse, String> {
    let status = match app.try_state::<BackendStatusState>() {
        Some(state) => state
            .0
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default(),
        None => BackendStatus::default(),
    };
    Ok(crate::qdrant::status_response(&app, &status))
}

#[tauri::command]
pub async fn get_metadata_values(
    app: AppHandle,
    collection_name: String,
    key: String,
) -> Result<MetadataValuesResponse, String> {
    let client = app
        .try_state::<QdrantState>()
        .map(|s| s.client.clone())
        .ok_or_else(|| "qdrant not ready".to_string())?;
    let svc = QdrantService::new(client);
    let values = svc
        .get_metadata_values_by_key(&collection_name, &key, None)
        .await
        .map_err(|e| format!("metadata values: {e:#}"))?;
    Ok(MetadataValuesResponse {
        values: values
            .into_iter()
            .map(|v| match v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            })
            .collect(),
    })
}
