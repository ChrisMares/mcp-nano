use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct WebsiteItem {
    pub url: String,
    pub group: String,
    pub chunk_count: i64,
    pub embedded_at: String,
}
