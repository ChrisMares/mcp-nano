use crate::models::request::RagQueryRequest;
use crate::models::response::{MetadataValuesResponse, RagResponse};

#[tauri::command]
pub async fn rag_query(payload: RagQueryRequest) -> Result<RagResponse, String> {
    println!("rag_query: {payload:?}");
    Ok(RagResponse {
        user_query: payload.query,
        ..Default::default()
    })
}

#[tauri::command]
pub async fn get_metadata_values(
    collection_name: String,
    key: String,
) -> Result<MetadataValuesResponse, String> {
    println!("get_metadata_values: collection_name={collection_name}, key={key}");
    Ok(MetadataValuesResponse::default())
}
