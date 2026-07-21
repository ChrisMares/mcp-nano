use serde::Serialize;
use sqlx::FromRow;

/// `file_metadata` SQLite row — matches `migrations/0001_initial.sql`.
///
/// The `filename` accessor derives from `full_path` (basename). For the UI
/// shape that exposes `filename` directly, use [`crate::models::response::FileMetadataDto`].
#[derive(Debug, Default, Clone, Serialize, FromRow)]
pub struct FileMetadata {
    pub id: i64,
    pub storage_object_id: String,
    pub full_path: String,
    pub file_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub repo_name: Option<String>,
    pub group_id: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub created_at: Option<String>,
    pub collection: Option<String>,
}

impl FileMetadata {
    /// Basename of `full_path` — mirrors the Python `FileMetadata.filename`
    /// property.
    pub fn filename(&self) -> Option<&str> {
        self.full_path.rsplit('/').next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_derives_from_full_path() {
        let m = FileMetadata {
            full_path: "/tmp/uploads/main.py".to_string(),
            ..Default::default()
        };
        assert_eq!(m.filename(), Some("main.py"));
    }

    #[test]
    fn filename_handles_no_slash() {
        let m = FileMetadata {
            full_path: "nofile".to_string(),
            ..Default::default()
        };
        assert_eq!(m.filename(), Some("nofile"));
    }
}
