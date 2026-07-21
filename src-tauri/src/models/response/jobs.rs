use serde::Serialize;

use crate::models::entities::JobStatus;

#[derive(Debug, Default, Serialize)]
pub struct ActiveJobsResponse {
    pub jobs: Vec<JobStatus>,
    pub total_count: i64,
}

#[derive(Debug, Default, Serialize)]
pub struct UploadJobEntry {
    pub filename: String,
    pub job_id: String,
    pub collection: String,
    pub status: String,
}

#[derive(Debug, Default, Serialize)]
pub struct UploadResponse {
    pub message: String,
    pub jobs: Vec<UploadJobEntry>,
    pub errors: Vec<String>,
}
