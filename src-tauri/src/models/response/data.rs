use serde::Serialize;

use crate::models::entities::FileMetadata;
use crate::models::{RepoItem, WebsiteItem};

#[derive(Debug, Default, Serialize)]
pub struct UserFilesResponse {
    pub repos: Vec<RepoItem>,
    pub documents: Vec<FileMetadata>,
}

#[derive(Debug, Default, Serialize)]
pub struct WebsitesResponse {
    pub websites: Vec<WebsiteItem>,
}

#[derive(Debug, Default, Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
}
