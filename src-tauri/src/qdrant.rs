use std::{collections::HashMap, net::TcpListener, path::PathBuf, time::Duration};

use qdrant_client::qdrant::{
    vectors_config, CreateCollectionBuilder, CreateFieldIndexCollectionBuilder, Distance,
    FieldType, SparseIndexConfig, SparseVectorConfig, SparseVectorParams, VectorParams,
    VectorParamsMap, VectorsConfig,
};
use qdrant_client::Qdrant;
use tauri::{AppHandle, Manager};
use tauri_plugin_shell::{process::CommandEvent, ShellExt};

// Must match the dense embedder output size (Snowflake/snowflake-arctic-embed-xs).
pub const EMBEDDING_DIM: u64 = 384;

const COLLECTIONS: [&str; 2] = ["codebase", "general"];

const PAYLOAD_INDEXES: [(&str, &str, FieldType); 10] = [
    ("codebase", "repo_name", FieldType::Keyword),
    ("codebase", "file_name", FieldType::Keyword),
    ("codebase", "created_at", FieldType::Datetime),
    ("general", "file_name", FieldType::Keyword),
    ("general", "group", FieldType::Keyword),
    ("general", "url", FieldType::Keyword),
    ("general", "website_key", FieldType::Keyword),
    ("general", "zip_filename", FieldType::Keyword),
    ("general", "doc_type", FieldType::Keyword),
    ("general", "created_at", FieldType::Datetime),
];

pub struct QdrantState {
    pub client: Qdrant,
    pub http_port: u16,
    pub grpc_port: u16,
}

fn available_port() -> Result<u16, String> {
    TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("failed to reserve a Qdrant port: {error}"))?
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| format!("failed to read the reserved Qdrant port: {error}"))
}

fn storage_path(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("failed to resolve application data directory: {error}"))?;
    let storage = data_dir.join("qdrant");
    std::fs::create_dir_all(&storage)
        .map_err(|error| format!("failed to create Qdrant storage directory: {error}"))?;
    Ok(storage)
}

pub fn start(app: &AppHandle) -> Result<(u16, u16), String> {
    let http_port = available_port()?;
    let grpc_port = available_port()?;
    let storage = storage_path(app)?;
    let snapshots = storage.join("snapshots");
    std::fs::create_dir_all(&snapshots)
        .map_err(|error| format!("failed to create Qdrant snapshots directory: {error}"))?;
    let (mut events, child) = app
        .shell()
        .sidecar("qdrant")
        .map_err(|error| format!("failed to resolve bundled Qdrant sidecar: {error}"))?
        .env("QDRANT__SERVICE__HOST", "127.0.0.1")
        .env("QDRANT__SERVICE__HTTP_PORT", http_port.to_string())
        .env("QDRANT__SERVICE__GRPC_PORT", grpc_port.to_string())
        .env("QDRANT__STORAGE__STORAGE_PATH", storage.as_os_str())
        .env("QDRANT__STORAGE__SNAPSHOTS_PATH", snapshots.as_os_str())
        .spawn()
        .map_err(|error| format!("failed to start Qdrant: {error}"))?;

    println!(
        "Started Qdrant (pid {}) at http://127.0.0.1:{http_port}; storage: {}",
        child.pid(),
        storage.display()
    );

    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    println!("qdrant: {}", String::from_utf8_lossy(&line).trim_end());
                }
                CommandEvent::Stderr(line) => {
                    eprintln!("qdrant: {}", String::from_utf8_lossy(&line).trim_end());
                }
                CommandEvent::Error(error) => eprintln!("qdrant process error: {error}"),
                CommandEvent::Terminated(status) => {
                    eprintln!(
                        "qdrant exited: code={:?}, signal={:?}",
                        status.code, status.signal
                    );
                }
                _ => {}
            }
        }
    });

    Ok((http_port, grpc_port))
}

/// Connect to the sidecar over gRPC, ensure collections and payload indexes
/// exist, then register `QdrantState` in Tauri managed state.
pub async fn init(app: AppHandle, http_port: u16, grpc_port: u16) -> Result<(), String> {
    let client = connect_with_retry(grpc_port).await?;
    println!("Qdrant connected on gRPC port {grpc_port}");

    ensure_collections(&client).await?;

    app.manage(QdrantState {
        client,
        http_port,
        grpc_port,
    });
    println!("Qdrant initialization complete");
    Ok(())
}

pub async fn connect_with_retry(grpc_port: u16) -> Result<Qdrant, String> {
    let url = format!("http://127.0.0.1:{grpc_port}");
    let client = Qdrant::from_url(&url)
        .build()
        .map_err(|error| format!("failed to build Qdrant client: {error}"))?;

    const MAX_RETRIES: usize = 5;
    for attempt in 0..MAX_RETRIES {
        match client.list_collections().await {
            Ok(_) => return Ok(client),
            Err(error) => {
                if attempt + 1 < MAX_RETRIES {
                    let wait = 2 * (attempt as u64 + 1);
                    eprintln!("Qdrant not ready ({error}). Retrying in {wait}s...");
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                } else {
                    return Err(format!(
                        "could not connect to Qdrant after {MAX_RETRIES} attempts: {error}"
                    ));
                }
            }
        }
    }
    unreachable!()
}

pub async fn ensure_collections(client: &Qdrant) -> Result<(), String> {
    let existing: Vec<String> = client
        .list_collections()
        .await
        .map_err(|error| format!("failed to list collections: {error}"))?
        .collections
        .into_iter()
        .map(|collection| collection.name)
        .collect();
    println!("Qdrant collections found: {existing:?}");

    for name in COLLECTIONS {
        if existing.iter().any(|collection| collection == name) {
            println!("   [OK] Collection '{name}' exists.");
            continue;
        }

        let mut dense_map = HashMap::new();
        dense_map.insert(
            "dense".to_string(),
            VectorParams {
                size: EMBEDDING_DIM,
                distance: Distance::Cosine as i32,
                ..Default::default()
            },
        );

        let mut sparse_map = HashMap::new();
        sparse_map.insert(
            "sparse".to_string(),
            SparseVectorParams {
                index: Some(SparseIndexConfig {
                    on_disk: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );

        client
            .create_collection(
                CreateCollectionBuilder::new(name)
                    .vectors_config(VectorsConfig {
                        config: Some(vectors_config::Config::ParamsMap(VectorParamsMap {
                            map: dense_map,
                        })),
                    })
                    .sparse_vectors_config(SparseVectorConfig { map: sparse_map }),
            )
            .await
            .map_err(|error| format!("failed to create collection '{name}': {error}"))?;
        println!("Created collection '{name}' with dimension {EMBEDDING_DIM}");
    }

    for (collection, field, field_type) in PAYLOAD_INDEXES {
        match client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                collection, field, field_type,
            ))
            .await
        {
            Ok(_) => println!("Created index '{field}' on '{collection}'"),
            Err(error) => eprintln!("Index '{field}' on '{collection}' skipped: {error}"),
        }
    }

    Ok(())
}
