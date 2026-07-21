use crate::models::entities::JobStatus;
use crate::models::request::EmbeddingOptions;
use crate::models::response::{ActiveJobsResponse, UploadResponse};

#[tauri::command]
pub async fn upload_repo_zip(
    paths: Vec<String>,
    embedding_options: EmbeddingOptions,
) -> Result<UploadResponse, String> {
    println!("upload_repo_zip: paths={paths:?}, embedding_options={embedding_options:?}");
    Ok(UploadResponse::default())
}

#[tauri::command]
pub async fn upload_documents(
    paths: Vec<String>,
    embedding_options: EmbeddingOptions,
) -> Result<UploadResponse, String> {
    println!("upload_documents: paths={paths:?}, embedding_options={embedding_options:?}");
    Ok(UploadResponse::default())
}

#[tauri::command]
pub async fn upload_code_files(
    paths: Vec<String>,
    embedding_options: EmbeddingOptions,
) -> Result<UploadResponse, String> {
    println!("upload_code_files: paths={paths:?}, embedding_options={embedding_options:?}");
    Ok(UploadResponse::default())
}

#[tauri::command]
pub async fn get_active_jobs() -> Result<ActiveJobsResponse, String> {
    println!("get_active_jobs");
    Ok(ActiveJobsResponse::default())
}

#[tauri::command]
pub async fn get_job_status(job_id: String) -> Result<JobStatus, String> {
    println!("get_job_status: job_id={job_id}");
    Ok(JobStatus::default())
}
