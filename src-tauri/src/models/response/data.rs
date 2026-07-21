use serde::Serialize;

use crate::models::{RepoItem, WebsiteItem};

/// UI-facing projection of a `file_metadata` row. Mirrors the shape the
/// frontend expects (filename, group, etc.) without leaking internal fields
/// like `full_path` or `error_message`.
#[derive(Debug, Default, Serialize)]
pub struct FileMetadataDto {
    pub filename: String,
    pub file_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub created_at: Option<String>,
    pub storage_object_id: Option<String>,
    pub group: String,
}

impl FileMetadataDto {
    /// Project a DB row into the UI shape. `filename` comes from the row's
    /// `full_path` basename; `group` falls back to the row's `group_id`.
    pub fn from_entity(row: &crate::models::entities::FileMetadata) -> Self {
        Self {
            filename: row.filename().unwrap_or("").to_string(),
            file_type: row.file_type.clone(),
            size_bytes: row.size_bytes,
            created_at: row.created_at.clone(),
            storage_object_id: Some(row.storage_object_id.clone()),
            group: row.group_id.clone().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct UserFilesResponse {
    pub repos: Vec<RepoItem>,
    pub documents: Vec<FileMetadataDto>,
}

#[derive(Debug, Default, Serialize)]
pub struct WebsitesResponse {
    pub websites: Vec<WebsiteItem>,
}

#[derive(Debug, Default, Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::entities::FileMetadata;

    #[test]
    fn dto_from_entity_maps_fields() {
        let row = FileMetadata {
            id: 1,
            storage_object_id: "abc-123".to_string(),
            full_path: "/uploads/notes.txt".to_string(),
            file_type: Some("txt".to_string()),
            size_bytes: Some(1234),
            repo_name: None,
            group_id: Some("docs".to_string()),
            status: "completed".to_string(),
            error_message: None,
            created_at: Some("2026-07-21T00:00:00Z".to_string()),
            collection: Some("general".to_string()),
        };
        let dto = FileMetadataDto::from_entity(&row);
        assert_eq!(dto.filename, "notes.txt");
        assert_eq!(dto.file_type.as_deref(), Some("txt"));
        assert_eq!(dto.size_bytes, Some(1234));
        assert_eq!(dto.storage_object_id.as_deref(), Some("abc-123"));
        assert_eq!(dto.group, "docs");
    }

    #[test]
    fn dto_from_entity_handles_missing_group() {
        let row = FileMetadata {
            full_path: "x.py".to_string(),
            group_id: None,
            ..Default::default()
        };
        let dto = FileMetadataDto::from_entity(&row);
        assert_eq!(dto.group, "");
        assert_eq!(dto.filename, "x.py");
    }
}
