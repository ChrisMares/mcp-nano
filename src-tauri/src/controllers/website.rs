use crate::models::response::{CrawlResponse, EmbedWebsiteResponse};

#[tauri::command]
pub async fn crawl_website(
    url: String,
    depth: Option<i64>,
    same_domain_only: Option<bool>,
) -> Result<CrawlResponse, String> {
    println!("crawl_website: url={url}, depth={depth:?}, same_domain_only={same_domain_only:?}");
    Ok(CrawlResponse::default())
}

#[tauri::command]
pub async fn embed_website(
    urls: Vec<String>,
    group: Option<String>,
) -> Result<EmbedWebsiteResponse, String> {
    println!("embed_website: urls={urls:?}, group={group:?}");
    Ok(EmbedWebsiteResponse::default())
}
