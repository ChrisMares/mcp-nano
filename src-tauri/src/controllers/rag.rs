use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::models::request::RagQueryRequest;
use crate::models::response::{
    CollectionsResponse, EmbedderStatusResponse, MetadataValuesResponse, RagResponse,
};
use crate::qdrant::QdrantState;
use crate::services::embedder_state::EmbedderState;
use crate::services::qdrant_service::QdrantService;
use crate::services::rag_service::RagService;

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

#[tauri::command]
pub async fn get_collections(app: AppHandle) -> Result<CollectionsResponse, String> {
    let client = app
        .try_state::<QdrantState>()
        .map(|s| s.client.clone())
        .ok_or_else(|| "qdrant not ready".to_string())?;
    let svc = QdrantService::new(client);
    let collections = svc
        .list_collections()
        .await
        .map_err(|e| format!("list collections: {e:#}"))?;
    Ok(CollectionsResponse { collections })
}

#[tauri::command]
pub async fn get_metadata_keys(
    app: AppHandle,
    collection_name: String,
) -> Result<MetadataValuesResponse, String> {
    let client = app
        .try_state::<QdrantState>()
        .map(|s| s.client.clone())
        .ok_or_else(|| "qdrant not ready".to_string())?;
    let svc = QdrantService::new(client);
    let keys = svc
        .get_metadata_values_by_key(&collection_name, "file_name", None)
        .await
        .map_err(|e| format!("metadata keys: {e:#}"))?;
    Ok(MetadataValuesResponse {
        values: keys
            .into_iter()
            .filter_map(|v| match v {
                serde_json::Value::String(s) => Some(s),
                other => Some(other.to_string()),
            })
            .collect(),
    })
}

#[tauri::command]
pub async fn get_embedders_status(app: AppHandle) -> Result<EmbedderStatusResponse, String> {
    let embedders_loaded = app.try_state::<Arc<EmbedderState>>().is_some();
    let models_dir = EmbedderState::models_dir(&app)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(EmbedderStatusResponse {
        dense_loaded: embedders_loaded,
        reranker_loaded: embedders_loaded,
        bm25_loaded: embedders_loaded,
        models_dir,
    })
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
            .filter_map(|v| match v {
                serde_json::Value::String(s) => Some(s),
                other => Some(other.to_string()),
            })
            .collect(),
    })
}
