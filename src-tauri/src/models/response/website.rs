use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct CrawlResponse {
    pub urls: Vec<String>,
    pub count: i64,
}

#[derive(Debug, Default, Serialize)]
pub struct EmbedWebsiteResponse {
    pub job_id: String,
    pub url_count: i64,
}
