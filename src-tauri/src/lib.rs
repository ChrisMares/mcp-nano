mod controllers;
mod models;
pub mod qdrant;

use controllers::{data, jobs, mcpconfig, rag, website};

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let (http_port, grpc_port) = qdrant::start(app.handle())?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = qdrant::init(handle, http_port, grpc_port).await {
                    eprintln!("Qdrant initialization failed: {error}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            rag::rag_query,
            rag::get_metadata_values,
            jobs::upload_repo_zip,
            jobs::upload_documents,
            jobs::upload_code_files,
            jobs::get_active_jobs,
            jobs::get_job_status,
            data::get_files,
            data::delete_repo,
            data::delete_document,
            data::delete_group,
            data::clear_user_collection,
            data::get_websites,
            data::delete_website,
            data::delete_website_group,
            data::clear_websites,
            mcpconfig::get_mcp_servers,
            mcpconfig::create_mcp_server,
            mcpconfig::get_mcp_server,
            mcpconfig::delete_mcp_server,
            mcpconfig::create_mcp_tool,
            mcpconfig::update_mcp_tool,
            mcpconfig::delete_mcp_tool,
            mcpconfig::toggle_mcp_tool,
            mcpconfig::get_mcp_connection_info,
            website::crawl_website,
            website::embed_website,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
