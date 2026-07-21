use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct FileMetadata {
    pub filename: String,
    pub file_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub created_at: Option<String>,
    pub storage_object_id: Option<String>,
    pub group: String,
}
