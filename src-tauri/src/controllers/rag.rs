use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::db::DbState;
use crate::models::request::RagQueryRequest;
use crate::models::response::{
    BackendStatusResponse, CollectionsResponse, EmbedderStatusResponse, MetadataValuesResponse,
    RagResponse,
};
use crate::qdrant::{BackendStatusState, QdrantState};
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
    let embedder_state = app.try_state::<Arc<EmbedderState>>();
    let embedders_loaded = embedder_state.is_some();
    let embedding_device = embedder_state.map(|s| s.device_mode().to_string());
    let models_dir = EmbedderState::models_dir(&app)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(EmbedderStatusResponse {
        dense_loaded: embedders_loaded,
        reranker_loaded: embedders_loaded,
        bm25_loaded: embedders_loaded,
        models_dir,
        embedding_device,
    })
}

#[tauri::command]
pub async fn get_backend_status(app: AppHandle) -> Result<BackendStatusResponse, String> {
    if let Some(state) = app.try_state::<BackendStatusState>() {
        if let Ok(guard) = state.0.read() {
            return Ok(BackendStatusResponse {
                qdrant_ready: guard.qdrant_ready,
                qdrant_error: guard.qdrant_error.clone(),
                http_port: guard.http_port,
                grpc_port: guard.grpc_port,
                db_ready: guard.db_ready,
                embedders_ready: guard.embedders_ready,
                embedding_device: guard.embedding_device.clone(),
                worker_ready: guard.worker_ready,
            });
        }
    }
    Ok(BackendStatusResponse {
        qdrant_ready: app.try_state::<QdrantState>().is_some(),
        qdrant_error: None,
        http_port: app.try_state::<QdrantState>().map(|s| s.http_port),
        grpc_port: app.try_state::<QdrantState>().map(|s| s.grpc_port),
        db_ready: app.try_state::<DbState>().is_some(),
        embedders_ready: app.try_state::<Arc<EmbedderState>>().is_some(),
        embedding_device: app
            .try_state::<Arc<EmbedderState>>()
            .map(|state| state.device_mode().to_string()),
        worker_ready: app.try_state::<Arc<RagService>>().is_some(),
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
