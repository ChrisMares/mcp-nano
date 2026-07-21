use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct RepoItem {
    pub repo_name: String,
    pub created_at: Option<String>,
    pub storage_object_id: Option<String>,
}
