use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Default, Serialize, FromRow)]
pub struct JobStatus {
    pub id: i64,
    pub job_id: String,
    pub status: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub result: Option<String>,
    pub error_message: Option<String>,
    pub progress_percentage: i64,
    pub file_name: Option<String>,
    pub storage_object_id: Option<String>,
    pub task_name: Option<String>,
    pub task_params: Option<String>,
}
