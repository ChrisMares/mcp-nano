use crate::models::response::{DeleteResponse, UserFilesResponse, WebsitesResponse};

#[tauri::command]
pub async fn get_files() -> Result<UserFilesResponse, String> {
    println!("get_files");
    Ok(UserFilesResponse::default())
}

#[tauri::command]
pub async fn delete_repo(repo_name: String) -> Result<DeleteResponse, String> {
    println!("delete_repo: repo_name={repo_name}");
    Ok(DeleteResponse::default())
}

#[tauri::command]
pub async fn delete_document(filename: String) -> Result<DeleteResponse, String> {
    println!("delete_document: filename={filename}");
    Ok(DeleteResponse::default())
}

#[tauri::command]
pub async fn delete_group(group_name: String) -> Result<DeleteResponse, String> {
    println!("delete_group: group_name={group_name}");
    Ok(DeleteResponse::default())
}

#[tauri::command]
pub async fn clear_user_collection(collection_name: String) -> Result<DeleteResponse, String> {
    println!("clear_user_collection: collection_name={collection_name}");
    Ok(DeleteResponse::default())
}

#[tauri::command]
pub async fn get_websites() -> Result<WebsitesResponse, String> {
    println!("get_websites");
    Ok(WebsitesResponse::default())
}

#[tauri::command]
pub async fn delete_website(url: String) -> Result<DeleteResponse, String> {
    println!("delete_website: url={url}");
    Ok(DeleteResponse::default())
}

#[tauri::command]
pub async fn delete_website_group(group_name: String) -> Result<DeleteResponse, String> {
    println!("delete_website_group: group_name={group_name}");
    Ok(DeleteResponse::default())
}

#[tauri::command]
pub async fn clear_websites() -> Result<DeleteResponse, String> {
    println!("clear_websites");
    Ok(DeleteResponse::default())
}
