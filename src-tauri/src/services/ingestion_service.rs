use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use text_splitter::{ChunkConfig, TextSplitter};
use tokenizers::Tokenizer;
use uuid::Uuid;

use crate::models::request::EmbeddingOptions;
use crate::services::embedders::{EncodeDocuments, EncodeQuery};
use crate::services::embedder_state::EmbedderState;
use crate::services::qdrant_service::QdrantService;
use crate::worker::{ProgressCallback, TaskRegistry};

/// Default chunk size in tokens (matches the Python `DOC_CHUNK_SIZE = 768`).
const DEFAULT_CHUNK_SIZE: usize = 768;
/// Default overlap between chunks (matches the Python `DOC_CHUNK_OVERLAP = 50`).
const DEFAULT_CHUNK_OVERLAP: usize = 50;
/// Batch size for Qdrant upserts.
const UPSERT_BATCH_SIZE: usize = 250;
/// Batch size for dense embedding forward passes.
const EMBED_BATCH_SIZE: usize = 16;

/// End-to-end ingestion pipeline: chunk text, embed via the dense model,
/// and upsert into Qdrant with hybrid (dense + BM25 sparse) vectors.
///
/// Three of the four Python ingestion entry points are wired end-to-end
/// here; `process_website_embed` returns an explicit "not yet implemented"
/// error and is realized in Phase 5 alongside the `reqwest`+`scraper`
/// crawler.
pub struct IngestionService {
    embedders: Arc<EmbedderState>,
    qdrant: QdrantService,
    splitter: TextSplitter<Tokenizer>,
}

impl IngestionService {
    pub fn new(embedders: Arc<EmbedderState>, qdrant: QdrantService, models_dir: &Path) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(models_dir.join("arctic-embed-xs/tokenizer.json"))
            .map_err(|e| anyhow!("loading chunk tokenizer: {e}"))?;
        let config = ChunkConfig::new(DEFAULT_CHUNK_SIZE)
            .with_sizer(tokenizer)
            .with_overlap(DEFAULT_CHUNK_OVERLAP)
            .context("building chunk config")?;
        let splitter = TextSplitter::new(config);
        Ok(Self {
            embedders,
            qdrant,
            splitter,
        })
    }

    /// Process a zip upload: unzip, walk extracted files, dispatch each to
    /// document or code ingestion based on `embedding_options.collection`.
    pub async fn process_zip_upload(
        &self,
        params: serde_json::Value,
        progress: crate::worker::ProgressCallback,
    ) -> Result<String> {
        let p: ProcessZipParams = serde_json::from_value(params)
            .context("deserializing process_zip_upload params")?;
        progress(5, Some("Starting zip extraction".to_string())).await;

        let extract_dir = PathBuf::from(format!("{}_extracted", strip_zip_ext(&p.zip_path)));
        unzip_to(&p.zip_path, &extract_dir)
            .with_context(|| format!("unzipping {}", p.zip_path))?;
        progress(20, Some("Zip extraction complete".to_string())).await;

        let collection = p
            .embedding_options
            .collection
            .clone()
            .unwrap_or_else(|| "general".to_string());
        self.qdrant.ensure_collection(&collection).await?;

        // For codebase uploads, propagate repo_name; for general uploads,
        // stamp zip_filename into metadata so Phase 5 can delete-by-zip on
        // re-upload.
        let mut embedding_options = p.embedding_options;
        if let Some(zf) = &p.zip_filename {
            let meta = embedding_options.metadata.get_or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if let serde_json::Value::Object(map) = meta {
                map.insert("zip_filename".to_string(), serde_json::Value::String(zf.clone()));
            }
        }
        let repo_name_for_dispatch = if collection == "codebase" {
            embedding_options.repo_name.as_deref()
        } else {
            None
        };

        // Walk extracted files and dispatch.
        let mut processed = 0usize;
        let mut errors: Vec<String> = Vec::new();
        for entry in walkdir::WalkDir::new(&extract_dir).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            // Skip images (no OCR in this build).
            if is_image_ext(path) {
                continue;
            }
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let result = self
                .ingest_text_file(
                    path,
                    &collection,
                    repo_name_for_dispatch,
                    &embedding_options,
                    &progress,
                )
                .await;
            match result {
                Ok(n) => processed += n,
                Err(e) => errors.push(format!("{file_name}: {e:#}")),
            }
        }
        progress(100, Some("Processing complete".to_string())).await;

        // Cleanup extraction dir + zip.
        let _ = std::fs::remove_dir_all(&extract_dir);
        let _ = std::fs::remove_file(&p.zip_path);

        if errors.is_empty() {
            Ok(format!("Processed {processed} file(s) from zip."))
        } else {
            Ok(format!(
                "Processed {processed} file(s); {} error(s): {}",
                errors.len(),
                errors.join("; ")
            ))
        }
    }

    /// Process a single document upload: read, chunk, embed, upsert.
    pub async fn process_documents_upload(
        &self,
        params: serde_json::Value,
        progress: crate::worker::ProgressCallback,
    ) -> Result<String> {
        let p: ProcessDocParams = serde_json::from_value(params)
            .context("deserializing process_documents_upload params")?;
        progress(1, Some("Starting document processing".to_string())).await;

        if !Path::new(&p.path).exists() {
            return Err(anyhow!("Path {} does not exist", p.path));
        }
        let collection = p.collection.unwrap_or_else(|| "general".to_string());
        self.qdrant.ensure_collection(&collection).await?;

        progress(70, Some(format!("Embedding chunks from {}", p.path))).await;
        let n = self
            .ingest_text_file(
                Path::new(&p.path),
                &collection,
                None,
                &EmbeddingOptions {
                    collection: Some(collection.clone()),
                    repo_name: None,
                    group: p.group.clone(),
                    metadata: p.metadata.clone(),
                },
                &progress,
            )
            .await?;

        // Cleanup: remove the processed file (matches Python behavior).
        let _ = std::fs::remove_file(&p.path);

        progress(100, Some("Document processing complete".to_string())).await;
        Ok(format!("Processed {n} chunk(s)."))
    }

    /// Process a single code file upload: read, chunk, embed, upsert.
    pub async fn process_code_file_upload(
        &self,
        params: serde_json::Value,
        progress: crate::worker::ProgressCallback,
    ) -> Result<String> {
        let p: ProcessCodeFileParams = serde_json::from_value(params)
            .context("deserializing process_code_file_upload params")?;
        progress(1, Some("Starting code file processing".to_string())).await;

        if !Path::new(&p.path).exists() {
            return Err(anyhow!("Path {} does not exist", p.path));
        }
        self.qdrant.ensure_collection(&p.collection).await?;

        progress(20, Some(format!("Embedding chunks from {}", p.path))).await;
        let n = self
            .ingest_text_file(
                Path::new(&p.path),
                &p.collection,
                p.repo_name.as_deref(),
                &EmbeddingOptions {
                    collection: Some(p.collection.clone()),
                    repo_name: p.repo_name.clone(),
                    group: None,
                    metadata: p.metadata.clone(),
                },
                &progress,
            )
            .await?;

        let _ = std::fs::remove_file(&p.path);
        progress(100, Some("Code file processing complete".to_string())).await;
        Ok(format!("Processed {n} chunk(s)."))
    }

    /// Website embedding is Phase 5 work (requires the `reqwest`+`scraper`
    /// crawler). Returns an explicit error so the worker marks the job FAILED
    /// rather than silently no-op'ing.
    pub async fn process_website_embed(
        &self,
        _params: serde_json::Value,
        progress: crate::worker::ProgressCallback,
    ) -> Result<String> {
        progress(0, Some("Website embedding not yet implemented".to_string())).await;
        Err(anyhow!(
            "process_website_embed is not implemented until Phase 5 (website crawler)"
        ))
    }

    /// Build the production task registry binding the four ingestion entry
    /// points to their worker dispatch names. Matches the Python
    /// `_TASK_FUNCTIONS` map in `webapi/tasks.py`.
    ///
    /// `self` must be wrapped in `Arc` so each registered closure can clone
    /// a handle per job invocation.
    pub fn build_task_registry(self: Arc<Self>) -> TaskRegistry {
        let mut reg = TaskRegistry::new();
        let ingest = self.clone();
        reg.register("process_zip", move |params, progress| {
            let ingest = ingest.clone();
            async move { ingest.process_zip_upload(params, progress).await }
        });
        let ingest = self.clone();
        reg.register("process_documents_upload", move |params, progress| {
            let ingest = ingest.clone();
            async move { ingest.process_documents_upload(params, progress).await }
        });
        let ingest = self.clone();
        reg.register("process_code_file_upload", move |params, progress| {
            let ingest = ingest.clone();
            async move { ingest.process_code_file_upload(params, progress).await }
        });
        let ingest = self.clone();
        reg.register("process_website_scrape", move |params, progress| {
            let ingest = ingest.clone();
            async move { ingest.process_website_embed(params, progress).await }
        });
        reg
    }

    /// Shared chunk + embed + upsert path. Returns the number of chunks
    /// upserted. Phase 3 reads files as UTF-8 text only; Phase 5 swaps in
    /// language-aware code chunkers and document loaders (PDF/DOCX/XLSX).
    async fn ingest_text_file(
        &self,
        path: &Path,
        collection: &str,
        repo_name: Option<&str>,
        options: &EmbeddingOptions,
        progress: &crate::worker::ProgressCallback,
    ) -> Result<usize> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {} as UTF-8 text", path.display()))?;
        if text.trim().is_empty() {
            return Ok(0);
        }

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let doc_type = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("txt")
            .to_lowercase();

        let chunks: Vec<String> = self.splitter.chunks(&text).map(String::from).collect();
        if chunks.is_empty() {
            return Ok(0);
        }

        let chunk_refs: Vec<&str> = chunks.iter().map(|s| s.as_str()).collect();
        let embeddings = self
            .embedders
            .dense
            .encode_documents(&chunk_refs, EMBED_BATCH_SIZE)
            .context("encoding document chunks")?;

        let base_meta = build_base_metadata(&file_name, &doc_type, repo_name, options);
        let metadatas: Vec<serde_json::Value> = chunks
            .iter()
            .enumerate()
            .map(|(i, chunk)| {
                let mut meta = base_meta.clone();
                if let serde_json::Value::Object(map) = &mut meta {
                    map.insert("chunk_index".to_string(), serde_json::Value::from(i));
                    map.insert("content".to_string(), serde_json::Value::String(chunk.clone()));
                }
                meta
            })
            .collect();

        let ids: Vec<Uuid> = (0..chunks.len()).map(|_| Uuid::new_v4()).collect();

        progress(
            80,
            Some(format!("Upserting {} chunks to Qdrant", chunks.len())),
        )
        .await;

        self.qdrant
            .upsert_items(
                collection,
                &ids,
                &chunks,
                &embeddings,
                &metadatas,
                &self.embedders.bm25,
                UPSERT_BATCH_SIZE,
            )
            .await
            .context("upserting chunks to Qdrant")?;

        Ok(chunks.len())
    }
}

/// Build the base metadata payload (file_name, doc_type, repo_name, group,
/// + user-provided metadata). Chunk-specific fields (chunk_index, content)
/// are added by the caller.
fn build_base_metadata(
    file_name: &str,
    doc_type: &str,
    repo_name: Option<&str>,
    options: &EmbeddingOptions,
) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "file_name".to_string(),
        serde_json::Value::String(file_name.to_string()),
    );
    map.insert(
        "doc_type".to_string(),
        serde_json::Value::String(doc_type.to_string()),
    );
    if let Some(rn) = repo_name {
        map.insert(
            "repo_name".to_string(),
            serde_json::Value::String(rn.to_string()),
        );
    }
    if let Some(group) = &options.group {
        map.insert(
            "group".to_string(),
            serde_json::Value::String(group.clone()),
        );
    }
    if let Some(serde_json::Value::Object(user_meta)) = &options.metadata {
        for (k, v) in user_meta {
            map.insert(k.clone(), v.clone());
        }
    }
    let now = crate::worker::progress::now_iso();
    map.insert(
        "created_at".to_string(),
        serde_json::Value::String(now),
    );
    serde_json::Value::Object(map)
}

fn strip_zip_ext(zip_path: &str) -> String {
    if let Some(stripped) = zip_path.strip_suffix(".zip") {
        stripped.to_string()
    } else {
        zip_path.to_string()
    }
}

fn is_image_ext(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|s| s.to_lowercase()).as_deref(),
        Some("png") | Some("jpg") | Some("jpeg")
    )
}

fn unzip_to(zip_path: &str, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let entry_path = match entry.enclosed_name() {
            Some(p) => p.to_owned(),
            None => continue,
        };
        let out_path = dest.join(entry_path);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct ProcessZipParams {
    zip_path: String,
    zip_filename: Option<String>,
    embedding_options: EmbeddingOptions,
}

#[derive(Deserialize)]
struct ProcessDocParams {
    path: String,
    collection: Option<String>,
    group: Option<String>,
    metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ProcessCodeFileParams {
    path: String,
    collection: String,
    repo_name: Option<String>,
    metadata: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn strip_zip_ext_removes_zip_suffix() {
        assert_eq!(strip_zip_ext("/tmp/foo.zip"), "/tmp/foo");
        assert_eq!(strip_zip_ext("/tmp/foo.tar"), "/tmp/foo.tar");
    }

    #[test]
    fn is_image_ext_detects_image_extensions() {
        assert!(is_image_ext(Path::new("photo.png")));
        assert!(is_image_ext(Path::new("photo.JPG")));
        assert!(is_image_ext(Path::new("photo.jpeg")));
        assert!(!is_image_ext(Path::new("doc.pdf")));
        assert!(!is_image_ext(Path::new("noext")));
    }

    #[test]
    fn build_base_metadata_includes_required_fields() {
        let opts = EmbeddingOptions {
            collection: Some("general".to_string()),
            repo_name: None,
            group: Some("docs".to_string()),
            metadata: Some(serde_json::json!({"user_id": "u1"})),
        };
        let meta = build_base_metadata("notes.txt", "txt", None, &opts);
        let map = meta.as_object().expect("metadata is object");
        assert_eq!(map.get("file_name").unwrap().as_str().unwrap(), "notes.txt");
        assert_eq!(map.get("doc_type").unwrap().as_str().unwrap(), "txt");
        assert_eq!(map.get("group").unwrap().as_str().unwrap(), "docs");
        assert_eq!(map.get("user_id").unwrap().as_str().unwrap(), "u1");
        assert!(map.get("created_at").is_some());
        assert!(map.get("repo_name").is_none());
    }

    #[test]
    fn build_base_metadata_includes_repo_name_for_code() {
        let opts = EmbeddingOptions {
            collection: Some("codebase".to_string()),
            repo_name: Some("my_repo".to_string()),
            group: None,
            metadata: None,
        };
        let meta = build_base_metadata("main.py", "py", Some("my_repo"), &opts);
        let map = meta.as_object().unwrap();
        assert_eq!(map.get("repo_name").unwrap().as_str().unwrap(), "my_repo");
    }

    #[test]
    fn unzip_to_extracts_files() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let zip_path = dir.path().join("test.zip");
        let extract_dir = dir.path().join("extracted");

        // Build a zip with one file.
        let file = std::fs::File::create(&zip_path)?;
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("hello.txt", options)?;
        writer.write_all(b"hello world")?;
        let _ = writer.finish()?;

        unzip_to(zip_path.to_str().unwrap(), &extract_dir)?;
        let extracted = std::fs::read_to_string(extract_dir.join("hello.txt"))?;
        assert_eq!(extracted, "hello world");
        Ok(())
    }

    #[test]
    fn deserialize_process_doc_params_uses_defaults() {
        let v = serde_json::json!({"path": "/tmp/x.txt"});
        let p: ProcessDocParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.path, "/tmp/x.txt");
        assert!(p.collection.is_none());
        assert!(p.group.is_none());
    }

    #[test]
    fn deserialize_process_zip_params_requires_embedding_options() {
        let v = serde_json::json!({"zip_path": "/tmp/x.zip"});
        let err: Result<ProcessZipParams, _> = serde_json::from_value(v);
        assert!(err.is_err());
    }
}
