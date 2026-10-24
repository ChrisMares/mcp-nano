use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbeddingOptions {
    pub collection: Option<String>,
    pub repo_name: Option<String>,
    pub group: Option<String>,
    pub filter_test_files: Option<bool>,
    /// When true, exclude non-code/data/document file types (see
    /// `services::file_filters::is_code_only_ignored`). Defaults to true.
    pub code_files_only: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}