use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use futures_util::FutureExt;
use serde::Deserialize;
use text_splitter::{ChunkConfig, TextSplitter};
use tokenizers::Tokenizer;
use uuid::Uuid;

use tracing::{debug, error, info};

use crate::models::request::EmbeddingOptions;
use crate::services::embedders::EncodeDocuments;
use crate::services::embedder_state::EmbedderState;
use crate::services::ingestion;
use crate::services::qdrant_service::QdrantService;
use crate::worker::TaskRegistry;

/// Default chunk size in tokens (matches the Python `DOC_CHUNK_SIZE = 768`).
const DEFAULT_CHUNK_SIZE: usize = 768;
/// Default overlap between chunks (matches the Python `DOC_CHUNK_OVERLAP = 50`).
const DEFAULT_CHUNK_OVERLAP: usize = 50;
/// Default chunk size in tokens for code chunks before splitting oversized
/// chunks (matches Python `CODE_CHUNK_SIZE = 768`).
const CODE_CHUNK_SIZE: usize = 768;
/// Hard ceiling on stored chunk text. Token splitters can still emit huge
/// pieces for single-line minified blobs; we force-split by characters after.
const MAX_CHUNK_CHARS: usize = 6_000;
/// Batch size for Qdrant upserts.
const UPSERT_BATCH_SIZE: usize = 250;
/// Batch size for dense embedding forward passes.
const EMBED_BATCH_SIZE: usize = 16;

fn panic_payload_to_string(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Run a sync closure, converting panics into `Err` so batch jobs can skip
/// one bad unit of work instead of aborting the whole task.
fn catch_sync_panic<T>(label: &str, f: impl FnOnce() -> T) -> Result<T> {
    std::panic::catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        let msg = panic_payload_to_string(&payload);
        error!("{label} panicked: {msg}");
        anyhow!("{label} panicked: {msg}")
    })
}

/// Await a future, converting panics into `Err`.
async fn catch_async_panic<F, T>(label: &str, fut: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    match AssertUnwindSafe(fut).catch_unwind().await {
        Ok(result) => result,
        Err(payload) => {
            let msg = panic_payload_to_string(&payload);
            error!("{label} panicked: {msg}");
            Err(anyhow!("{label} panicked: {msg}"))
        }
    }
}

/// Map a 0-based batch index into `[range_start, range_end]` percent.
pub fn map_batch_progress(
    range_start: i32,
    range_end: i32,
    batch_index: i32,
    total_batches: i32,
) -> i32 {
    let span = (range_end - range_start).max(0);
    let total = total_batches.max(1);
    (range_start + (span * (batch_index + 1) / total)).clamp(0, 100)
}

/// Overall zip progress for file `idx` of `total_files` within the post-extract
/// band `[20, 100)`.
pub fn zip_file_progress_bounds(idx: usize, total_files: usize) -> (i32, i32) {
    let n = total_files.max(1) as i32;
    let i = idx as i32;
    let start = 20 + (80 * i) / n;
    let end = 20 + (80 * (i + 1)) / n;
    (start, end.max(start + 1).min(99))
}

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
    /// Second splitter used by `split_oversized_code_chunks`; mirrors the
    /// Python `CODE_CHUNK_SIZE=768, chunk_overlap=64` config.
    code_splitter: TextSplitter<Tokenizer>,
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

        // Code-splitter: separate `Tokenizer` instance (TextSplitter borrows
        // it via `Arc<Tokenizer>`); use a fresh clone of the same file.
        let code_tokenizer = Tokenizer::from_file(models_dir.join("arctic-embed-xs/tokenizer.json"))
            .map_err(|e| anyhow!("loading code tokenizer: {e}"))?;
        let code_config = ChunkConfig::new(CODE_CHUNK_SIZE)
            .with_sizer(code_tokenizer)
            .with_overlap(64)
            .context("building code chunk config")?;
        let code_splitter = TextSplitter::new(code_config);

        Ok(Self {
            embedders,
            qdrant,
            splitter,
            code_splitter,
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
            .map_err(|e| {
                error!("process_zip_upload: invalid params: {e:#}");
                anyhow!("invalid params: {e:#}")
            })
            .context("deserializing process_zip_upload params")?;
        info!("process_zip_upload: zip={} collection={:?}", p.zip_path, p.embedding_options.collection);
        progress(5, Some("Starting zip extraction".to_string())).await;

        let extract_dir = PathBuf::from(format!("{}_extracted", strip_zip_ext(&p.zip_path)));
        unzip_to(&p.zip_path, &extract_dir)
            .with_context(|| format!("unzipping {}", p.zip_path))?;
        info!("extracted zip to {}", extract_dir.display());
        progress(20, Some("Zip extraction complete".to_string())).await;

        let collection = p
            .embedding_options
            .collection
            .clone()
            .unwrap_or_else(|| "general".to_string());
        self.qdrant.ensure_collection(&collection).await?;

        // For codebase uploads, default repo_name to the zip basename (minus
        // `.zip`) when the client did not supply one. Stamp zip_filename into
        // metadata so re-uploads / deletes can target the zip.
        let mut embedding_options = p.embedding_options;
        if let Some(zf) = &p.zip_filename {
            let meta = embedding_options
                .metadata
                .get_or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if let serde_json::Value::Object(map) = meta {
                map.insert(
                    "zip_filename".to_string(),
                    serde_json::Value::String(zf.clone()),
                );
            }
        }
        if collection == "codebase" {
            let needs_default = embedding_options
                .repo_name
                .as_ref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true);
            if needs_default {
                if let Some(zf) = p.zip_filename.as_deref().filter(|s| !s.is_empty()) {
                    let default_name = repo_name_from_zip_filename(zf);
                    if !default_name.is_empty() {
                        info!(
                            "process_zip_upload: defaulting repo_name to {default_name} from zip"
                        );
                        embedding_options.repo_name = Some(default_name);
                    }
                }
            }
        }
        let repo_name_for_dispatch = if collection == "codebase" {
            embedding_options.repo_name.as_deref()
        } else {
            None
        };

        let mut file_paths: Vec<PathBuf> = walkdir::WalkDir::new(&extract_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .filter(|p| !should_skip_ingest_path(p))
            .collect();
        file_paths.sort();

        let total_files = file_paths.len().max(1);
        info!(
            "process_zip_upload: {} ingestible file(s) under {}",
            file_paths.len(),
            extract_dir.display()
        );
        crate::write_ingest_breadcrumb(
            "zip_scan_complete",
            &format!(
                "files={} extract_dir={}",
                file_paths.len(),
                extract_dir.display()
            ),
        );
        let mut processed = 0usize;
        let mut errors: Vec<String> = Vec::new();
        for (idx, path) in file_paths.iter().enumerate() {
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let rel = path
                .strip_prefix(&extract_dir)
                .unwrap_or(path.as_path())
                .display()
                .to_string();
            let (file_start, file_end) = zip_file_progress_bounds(idx, total_files);
            info!(
                "process_zip_upload: file {}/{} pct={}-{} path={}",
                idx + 1,
                total_files,
                file_start,
                file_end,
                rel
            );
            crate::write_ingest_breadcrumb(
                "zip_file_start",
                &format!(
                    "{}/{} pct={} path={}",
                    idx + 1,
                    total_files,
                    file_start,
                    rel
                ),
            );
            progress(
                file_start,
                Some(format!(
                    "Processing {} ({}/{})",
                    file_name,
                    idx + 1,
                    total_files
                )),
            )
            .await;

            // One bad file must not abort the whole zip job.
            let result = catch_async_panic(
                &format!("ingest {rel}"),
                self.ingest_text_file_with_range(
                    path,
                    &collection,
                    repo_name_for_dispatch,
                    &embedding_options,
                    &progress,
                    file_start,
                    file_end,
                ),
            )
            .await;
            match result {
                Ok(n) => {
                    processed += n;
                    debug!(
                        "process_zip_upload: ok file={rel} chunks={n} running_total={processed}"
                    );
                }
                Err(e) => {
                    error!("process_zip_upload: skip file={rel}: {e:#}");
                    crate::write_ingest_breadcrumb(
                        "zip_file_error",
                        &format!("{rel}: {e:#}"),
                    );
                    errors.push(format!("{rel}: {e:#}"));
                    progress(
                        file_end.min(99),
                        Some(format!(
                            "Skipped {} ({}/{})",
                            file_name,
                            idx + 1,
                            total_files
                        )),
                    )
                    .await;
                }
            }
        }
        crate::write_ingest_breadcrumb(
            "zip_complete",
            &format!("processed_chunks={processed} errors={}", errors.len()),
        );
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
            .map_err(|e| {
                error!("process_documents_upload: invalid params: {e:#}");
                anyhow!("invalid params: {e:#}")
            })
            .context("deserializing process_documents_upload params")?;
        info!("process_documents_upload: path={} group={:?}", p.path, p.group);
        let display = Path::new(&p.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document");
        progress(5, Some(format!("Loading {display}"))).await;

        if !Path::new(&p.path).exists() {
            error!("process_documents_upload: path does not exist: {}", p.path);
            return Err(anyhow!("Path {} does not exist", p.path));
        }
        let collection = p.collection.unwrap_or_else(|| "general".to_string());
        self.qdrant.ensure_collection(&collection).await?;

        let n = catch_async_panic(
            &format!("document {}", p.path),
            self.ingest_text_file_with_range(
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
                10,
                95,
            ),
        )
        .await?;

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
            .map_err(|e| {
                error!("process_code_file_upload: invalid params: {e:#}");
                anyhow!("invalid params: {e:#}")
            })
            .context("deserializing process_code_file_upload params")?;
        info!("process_code_file_upload: path={} collection={}", p.path, p.collection);
        let display = Path::new(&p.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("code");
        progress(5, Some(format!("Loading {display}"))).await;

        if !Path::new(&p.path).exists() {
            error!("process_code_file_upload: path does not exist: {}", p.path);
            return Err(anyhow!("Path {} does not exist", p.path));
        }
        self.qdrant.ensure_collection(&p.collection).await?;

        let n = catch_async_panic(
            &format!("code file {}", p.path),
            self.ingest_text_file_with_range(
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
                10,
                95,
            ),
        )
        .await?;

        let _ = std::fs::remove_file(&p.path);
        progress(100, Some("Code file processing complete".to_string())).await;
        Ok(format!("Processed {n} chunk(s)."))
    }

    /// Website embedding: scrape + chunk + embed + upsert. Mirrors the
    /// Python `process_website_embed`.
    pub async fn process_website_embed(
        &self,
        params: serde_json::Value,
        progress: crate::worker::ProgressCallback,
    ) -> Result<String> {
        let p: ProcessWebsiteParams = serde_json::from_value(params)
            .map_err(|e| {
                error!("process_website_embed: invalid params: {e:#}");
                anyhow!("invalid params: {e:#}")
            })
            .context("deserializing process_website_embed params")?;
        info!("process_website_embed: {} urls, group={}", p.urls.len(), p.group);
        progress(5, Some(format!("Scraping {} pages", p.urls.len()))).await;

        let mut base_metadata = serde_json::Map::new();
        base_metadata.insert("group".into(), serde_json::Value::String(p.group.clone()));
        for (k, v) in p.metadata.unwrap_or_default().as_object().into_iter().flatten() {
            base_metadata.insert(k.clone(), v.clone());
        }

        let chunks = catch_async_panic(
            "website scrape",
            async {
                ingestion::website::process_website(&p.urls, &base_metadata)
                    .await
                    .context("scraping + chunking website pages")
            },
        )
        .await?;
        if chunks.is_empty() {
            return Err(anyhow!("No content extracted from any URL"));
        }
        progress(30, Some(format!("Embedding {} chunks", chunks.len()))).await;
        let n = catch_async_panic(
            "website embed",
            self.embed_and_upsert_documents(&chunks, "general", None, &progress, 30, 100),
        )
        .await?;
        progress(100, Some("Website embedding complete".to_string())).await;
        Ok(format!("Embedded {n} chunks from {} page(s)", p.urls.len()))
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
    /// upserted. Dispatches code files (by extension) through the language-
    /// specific chunkers; everything else goes through `document_loaders`
    /// then the text-splitter.
    async fn ingest_text_file_with_range(
        &self,
        path: &Path,
        collection: &str,
        repo_name: Option<&str>,
        options: &EmbeddingOptions,
        progress: &crate::worker::ProgressCallback,
        range_start: i32,
        range_end: i32,
    ) -> Result<usize> {
        if should_skip_ingest_path(path) {
            return Ok(0);
        }
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let display = strip_job_prefix(&file_name)
            .unwrap_or_else(|| file_name.clone());
        let doc_type = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("txt")
            .to_lowercase();

        // Load + chunk are CPU-bound and can take minutes on large PDFs.
        // Report distinct stages so the UI is not frozen on one percentage.
        let load_pct = (range_start.saturating_sub(5)).max(5).min(range_start);
        let chunk_pct = load_pct.saturating_add(2).min(range_start);
        progress(
            load_pct,
            Some(format!("Extracting text from {display}")),
        )
        .await;

        let path_buf = path.to_path_buf();
        let path_display = path.display().to_string();
        let is_code = ingestion::code_chunker::is_code_file(path);
        let chunks: Vec<ingestion::DocumentChunk> = if is_code {
            progress(chunk_pct, Some(format!("Chunking code in {display}"))).await;
            crate::write_ingest_breadcrumb("chunk_code", &path_display);
            let repo = repo_name.unwrap_or("");
            let name_override = strip_job_prefix(&file_name);
            let code_chunks = catch_sync_panic(&format!("code chunker {path_display}"), || {
                ingestion::code_chunker::chunk_file_to_documents(
                    &path_buf,
                    repo,
                    name_override.as_deref(),
                    &self.code_splitter,
                    CODE_CHUNK_SIZE,
                )
            })?;
            debug!(
                "ingest: chunked code {} -> {} chunk(s)",
                path_display,
                code_chunks.len()
            );
            enforce_max_chunk_chars(code_chunks)
        } else {
            crate::write_ingest_breadcrumb("load_document", &path_display);
            let path_for_load = path_buf.clone();
            let loaded = tokio::task::spawn_blocking(move || {
                catch_sync_panic(
                    &format!("document loader {}", path_for_load.display()),
                    || ingestion::document_loaders::load_document(&path_for_load),
                )
                .and_then(|r| {
                    r.with_context(|| format!("loading document {}", path_for_load.display()))
                })
            })
            .await
            .map_err(|e| {
                if e.is_panic() {
                    let msg = panic_payload_to_string(&e.into_panic());
                    anyhow!("load task panicked for {path_display}: {msg}")
                } else {
                    anyhow!("load task join error for {path_display}: {e}")
                }
            })??;

            progress(chunk_pct, Some(format!("Chunking text from {display}"))).await;
            crate::write_ingest_breadcrumb("chunk_text", &path_display);
            catch_sync_panic(&format!("text splitter {path_display}"), || {
                split_document_chunks(loaded, &self.splitter)
            })?
        };

        if chunks.is_empty() {
            debug!("ingest: no chunks for {path_display}");
            return Ok(0);
        }

        progress(
            range_start,
            Some(format!(
                "Embedding {} chunk(s) from {display}",
                chunks.len()
            )),
        )
        .await;
        crate::write_ingest_breadcrumb(
            "embed_start",
            &format!("path={path_display} chunks={}", chunks.len()),
        );

        let extra_meta = build_base_metadata(&file_name, &doc_type, repo_name, options);
        let n = self
            .embed_and_upsert_documents_inner(
                &chunks,
                collection,
                extra_meta,
                progress,
                range_start,
                range_end,
            )
            .await
            .with_context(|| format!("embed/upsert failed for {path_display}"))?;
        crate::write_ingest_breadcrumb(
            "embed_done",
            &format!("path={path_display} chunks={n}"),
        );
        Ok(n)
    }

    /// Embed a list of prebuilt `DocumentChunk`s into Qdrant. Used by the
    /// website embedder where the chunks come directly from
    /// `ingestion::website::process_website`. The progress range is mapped
    /// onto `[range_start, range_end]` percent.
    async fn embed_and_upsert_documents(
        &self,
        chunks: &[ingestion::DocumentChunk],
        collection: &str,
        _repo_name: Option<&str>,
        progress: &crate::worker::ProgressCallback,
        range_start: i32,
        range_end: i32,
    ) -> Result<usize> {
        let extra = serde_json::Value::Object(serde_json::Map::new());
        self.embed_and_upsert_documents_inner(chunks, collection, extra, progress, range_start, range_end)
            .await
    }

    async fn embed_and_upsert_documents_inner(
        &self,
        chunks: &[ingestion::DocumentChunk],
        collection: &str,
        extra_meta: serde_json::Value,
        progress: &crate::worker::ProgressCallback,
        range_start: i32,
        range_end: i32,
    ) -> Result<usize> {
        if chunks.is_empty() {
            return Ok(0);
        }
        info!(
            "embed_and_upsert: {} chunks → {collection} [{range_start}..{range_end}]",
            chunks.len()
        );

        let texts: Vec<String> = chunks
            .iter()
            .map(|c| c.chunk_embedding_text())
            .collect();
        let documents: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
        let chunk_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();

        let total_batches = (((chunks.len() + EMBED_BATCH_SIZE - 1) / EMBED_BATCH_SIZE) as i32).max(1);
        let mut embeddings_list: Vec<Vec<f32>> = Vec::with_capacity(chunks.len());
        for batch_index in 0..total_batches {
            let batch_start = (batch_index as usize) * EMBED_BATCH_SIZE;
            let batch_end = (batch_start + EMBED_BATCH_SIZE).min(chunks.len());
            let batch_texts: Vec<&str> = chunk_refs[batch_start..batch_end].to_vec();
            crate::write_ingest_breadcrumb(
                "embed_batch",
                &format!(
                    "collection={collection} batch={}/{} size={}",
                    batch_index + 1,
                    total_batches,
                    batch_texts.len()
                ),
            );
            let batch_embeddings = catch_sync_panic(
                &format!("dense encode batch {}/{}", batch_index + 1, total_batches),
                || {
                    self.embedders
                        .dense
                        .encode_documents(&batch_texts, EMBED_BATCH_SIZE)
                },
            )?
            .with_context(|| format!("encoding batch {}/{}", batch_index + 1, total_batches))?;
            if batch_embeddings.len() != batch_texts.len() {
                return Err(anyhow!(
                    "dense encode batch {}/{}: got {} vectors for {} texts",
                    batch_index + 1,
                    total_batches,
                    batch_embeddings.len(),
                    batch_texts.len()
                ));
            }
            for e in batch_embeddings {
                embeddings_list.push(e);
            }
            let pct = map_batch_progress(range_start, range_end, batch_index, total_batches);
            progress(
                pct,
                Some(format!(
                    "Embedding batches {}/{}",
                    batch_index + 1,
                    total_batches
                )),
            )
            .await;
        }

        let metadatas: Vec<serde_json::Value> = chunks
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mut meta = c.chunk_metadata();
                if let serde_json::Value::Object(extra_map) = &extra_meta {
                    for (k, v) in extra_map {
                        meta.insert(k.clone(), v.clone());
                    }
                }
                if !meta.contains_key("chunk_index") {
                    meta.insert("chunk_index".into(), serde_json::Value::from(i as i64));
                }
                serde_json::Value::Object(meta)
            })
            .collect();

        let ids: Vec<Uuid> = chunks
            .iter()
            .map(|c| Uuid::parse_str(&c.id).unwrap_or_else(|_| Uuid::new_v4()))
            .collect();

        progress(range_end, Some(format!("Upserting {} chunks to Qdrant", chunks.len()))).await;
        crate::write_ingest_breadcrumb(
            "upsert",
            &format!("collection={collection} chunks={}", chunks.len()),
        );

        self.qdrant
            .upsert_items(
                collection,
                &ids,
                &documents,
                &embeddings_list,
                &metadatas,
                &self.embedders.bm25,
                UPSERT_BATCH_SIZE,
            )
            .await
            .context("upserting chunks to Qdrant")?;
        Ok(chunks.len())
    }
}

/// Re-tokenize a list of `DocumentChunk`s using the doc text-splitter. Each
/// input chunk whose `content` exceeds the splitter's chunk size becomes a
/// sequence of smaller chunks inheriting the same metadata. Used by
/// `IngestionService::ingest_text_file` for non-code uploads.
///
/// After the token splitter, any remaining piece larger than
/// [`MAX_CHUNK_CHARS`] is hard-split by character count (minified one-liners
/// often don't break under the token splitter alone).
pub fn split_document_chunks(
    chunks: Vec<ingestion::DocumentChunk>,
    splitter: &TextSplitter<Tokenizer>,
) -> Vec<ingestion::DocumentChunk> {
    if chunks.is_empty() {
        return chunks;
    }
    let mut out: Vec<ingestion::DocumentChunk> = Vec::new();
    for chunk in chunks {
        let parts: Vec<String> = splitter.chunks(&chunk.content).map(String::from).collect();
        let parts = if parts.is_empty() {
            vec![chunk.content.clone()]
        } else {
            parts
        };
        let mut local_idx = 0i64;
        for sub in parts {
            for piece in hard_split_chars(&sub, MAX_CHUNK_CHARS) {
                let mut child = chunk.clone();
                child.id = Uuid::new_v4().to_string();
                child.content = piece;
                child.chunk_index = chunk.chunk_index + local_idx;
                local_idx += 1;
                out.push(child);
            }
        }
    }
    out
}

/// Force-split `text` into pieces of at most `max_chars` (char boundary safe).
fn hard_split_chars(text: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return vec![text.to_string()];
    }
    if text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut buf = String::with_capacity(max_chars);
    let mut n = 0usize;
    for ch in text.chars() {
        if n >= max_chars {
            out.push(std::mem::take(&mut buf));
            n = 0;
        }
        buf.push(ch);
        n += 1;
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

/// Paths we never want in the vector DB (vendor, maps, binaries, minified,
/// test fixtures). Used for zip walks and single-file guards.
pub fn should_skip_ingest_path(path: &Path) -> bool {
    if is_image_ext(path) {
        return true;
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    const SKIP_EXTS: &[&str] = &[
        "map", "css", "scss", "sass", "less", "woff", "woff2", "ttf", "otf", "eot",
        "ico", "svg", "gif", "webp", "bmp", "mp3", "mp4", "wav", "zip", "gz", "tar",
        "7z", "rar", "bin", "exe", "dll", "so", "dylib", "pdb", "lock", "sum",
    ];
    if SKIP_EXTS.contains(&ext.as_str()) {
        return true;
    }
    if name.ends_with(".min.js")
        || name.ends_with(".min.mjs")
        || name.ends_with(".min.cjs")
        || name.ends_with(".bundle.js")
        || name.ends_with(".min.css")
    {
        return true;
    }
    if matches!(
        name.as_str(),
        "package-lock.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "cargo.lock"
            | "composer.lock"
            | "go.sum"
            | ".ds_store"
            | "thumbs.db"
    ) {
        return true;
    }

    // Normalize separators so segment checks work on Windows and Unix paths.
    let path_l = path
        .to_string_lossy()
        .to_ascii_lowercase()
        .replace('\\', "/");
    let padded = format!("/{path_l}/");
    for seg in [
        "/node_modules/",
        "/.git/",
        "/dist/",
        "/build/",
        "/target/",
        "/.vs/",
        "/bin/",
        "/obj/",
        // Test / fixture noise dominates large library zips (e.g. Chart.js)
        // and is rarely useful for code-search RAG.
        "/test/",
        "/tests/",
        "/__tests__/",
        "/spec/",
        "/specs/",
        "/fixtures/",
        "/.github/",
        "/coverage/",
        "/.nyc_output/",
        "/vendor/",
        "/third_party/",
        "/third-party/",
    ] {
        if padded.contains(seg) {
            return true;
        }
    }
    false
}

/// Strip the job-id UUID prefix from a temp filename (mirrors the Python
/// `embedding_utils.strip_job_prefix`).
pub fn strip_job_prefix(name: &str) -> Option<String> {
    let re = regex::Regex::new(
        r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}_",
    )
    .ok()?;
    let stripped = re.replace(name, "").to_string();
    Some(stripped)
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
    } else if let Some(stripped) = zip_path.strip_suffix(".ZIP") {
        stripped.to_string()
    } else {
        zip_path.to_string()
    }
}

/// Default repo name for a codebase zip: basename of the zip file without the
/// trailing `.zip` (case-insensitive).
pub fn repo_name_from_zip_filename(zip_filename: &str) -> String {
    let base = std::path::Path::new(zip_filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(zip_filename);
    let lower = base.to_ascii_lowercase();
    if let Some(stripped) = lower.strip_suffix(".zip") {
        base[..stripped.len()].to_string()
    } else {
        base.to_string()
    }
}

fn is_image_ext(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|s| s.to_lowercase()).as_deref(),
        Some("png") | Some("jpg") | Some("jpeg")
    )
}

fn enforce_max_chunk_chars(
    chunks: Vec<ingestion::DocumentChunk>,
) -> Vec<ingestion::DocumentChunk> {
    let mut out = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        if chunk.content.chars().count() <= MAX_CHUNK_CHARS {
            out.push(chunk);
            continue;
        }
        for (i, piece) in hard_split_chars(&chunk.content, MAX_CHUNK_CHARS)
            .into_iter()
            .enumerate()
        {
            let mut child = chunk.clone();
            child.id = Uuid::new_v4().to_string();
            child.content = piece;
            child.chunk_index = chunk.chunk_index + i as i64;
            out.push(child);
        }
    }
    out
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

#[derive(Deserialize)]
struct ProcessWebsiteParams {
    urls: Vec<String>,
    group: String,
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
    fn repo_name_from_zip_filename_strips_extension() {
        assert_eq!(
            repo_name_from_zip_filename("OptionsPricing-main.zip"),
            "OptionsPricing-main"
        );
        assert_eq!(repo_name_from_zip_filename("MyRepo.ZIP"), "MyRepo");
        assert_eq!(
            repo_name_from_zip_filename("/path/to/repo.zip"),
            "repo"
        );
        assert_eq!(repo_name_from_zip_filename("noext"), "noext");
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
            metadata: Some(serde_json::json!({"source": "manual"})),
        };
        let meta = build_base_metadata("notes.txt", "txt", None, &opts);
        let map = meta.as_object().expect("metadata is object");
        assert_eq!(map.get("file_name").unwrap().as_str().unwrap(), "notes.txt");
        assert_eq!(map.get("doc_type").unwrap().as_str().unwrap(), "txt");
        assert_eq!(map.get("group").unwrap().as_str().unwrap(), "docs");
        assert_eq!(map.get("source").unwrap().as_str().unwrap(), "manual");
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

    #[test]
    fn map_batch_progress_stays_within_range_and_is_monotonic() {
        let start = 10;
        let end = 95;
        let mut prev = start - 1;
        for i in 0..5 {
            let pct = map_batch_progress(start, end, i, 5);
            assert!(pct >= start, "pct {pct} < start {start}");
            assert!(pct <= end, "pct {pct} > end {end}");
            assert!(pct >= prev, "progress decreased: {prev} -> {pct}");
            prev = pct;
        }
        assert_eq!(map_batch_progress(10, 95, 4, 5), 95);
    }

    #[test]
    fn map_batch_progress_never_resets_below_prior_milestone() {
        // Simulates document flow: load at 5/10, then embed 10..95.
        let milestones = [5, 10];
        let mut last = 0;
        for m in milestones {
            assert!(m >= last);
            last = m;
        }
        for i in 0..8 {
            let pct = map_batch_progress(10, 95, i, 8);
            assert!(pct >= 10, "embed batch dropped below 10%: {pct}");
            assert!(pct >= last);
            last = pct;
        }
        assert!(last <= 95);
    }

    #[test]
    fn zip_file_progress_bounds_cover_post_extract_band() {
        let (s0, e0) = zip_file_progress_bounds(0, 4);
        let (s3, e3) = zip_file_progress_bounds(3, 4);
        assert_eq!(s0, 20);
        assert!(e0 > s0);
        assert!(s3 >= e0 || s3 >= 20);
        assert!(e3 <= 99);
        assert!(s3 < e3);
    }

    fn test_doc_splitter() -> Option<TextSplitter<Tokenizer>> {
        let tok_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/models/arctic-embed-xs/tokenizer.json");
        if !tok_path.exists() {
            return None;
        }
        let tokenizer = Tokenizer::from_file(&tok_path).ok()?;
        let config = ChunkConfig::new(DEFAULT_CHUNK_SIZE)
            .with_sizer(tokenizer)
            .with_overlap(DEFAULT_CHUNK_OVERLAP)
            .ok()?;
        Some(TextSplitter::new(config))
    }

    /// Reproduces the OptionsPricing zip issue: source maps / minified assets
    /// were stored as ~1MB `document` payloads. After the fix, every chunk
    /// must stay under the hard char cap.
    #[test]
    fn split_document_chunks_caps_huge_minified_blob() {
        let Some(splitter) = test_doc_splitter() else {
            eprintln!("skipping: arctic tokenizer not present");
            return;
        };
        // Single-line ~900KB blob (like a .js.map) — no natural newlines.
        let huge = "a".repeat(900_000);
        let chunk = ingestion::DocumentChunk::new("id1", "chart.umd.min.js.map", huge, "map", 0);
        let split = split_document_chunks(vec![chunk], &splitter);
        assert!(
            !split.is_empty(),
            "expected at least one chunk after split"
        );
        let max_chars = split.iter().map(|c| c.content.chars().count()).max().unwrap_or(0);
        assert!(
            max_chars <= MAX_CHUNK_CHARS,
            "chunk still oversized: max_chars={max_chars} limit={MAX_CHUNK_CHARS} n_chunks={}",
            split.len()
        );
        let total: usize = split.iter().map(|c| c.content.len()).sum();
        assert!(total >= 900_000 - 100, "lost content during split: {total}");
    }

    #[test]
    fn should_skip_ingest_junk_assets() {
        assert!(should_skip_ingest_path(Path::new("wwwroot/chart.umd.min.js.map")));
        assert!(should_skip_ingest_path(Path::new("bootstrap.min.css.map")));
        assert!(should_skip_ingest_path(Path::new("open-iconic.woff")));
        assert!(should_skip_ingest_path(Path::new("favicon.ico")));
        assert!(should_skip_ingest_path(Path::new("lib/jquery.min.js")));
        assert!(should_skip_ingest_path(Path::new("styles.min.css")));
        assert!(!should_skip_ingest_path(Path::new("src/OptionsService.cs")));
        assert!(!should_skip_ingest_path(Path::new("README.md")));
        assert!(!should_skip_ingest_path(Path::new("app.js")));
    }

    #[test]
    fn should_skip_test_and_fixture_paths() {
        assert!(should_skip_ingest_path(Path::new(
            "Chart.js-4.5.1/test/fixtures/controller.radar/radius/indexable.js"
        )));
        assert!(should_skip_ingest_path(Path::new(
            "repo/tests/unit/foo.rs"
        )));
        assert!(should_skip_ingest_path(Path::new(
            "pkg/__tests__/button.tsx"
        )));
        assert!(should_skip_ingest_path(Path::new(
            "lib/spec/helpers.js"
        )));
        assert!(should_skip_ingest_path(Path::new(
            "proj/.github/workflows/ci.yml"
        )));
        assert!(should_skip_ingest_path(Path::new(
            r"repo\test\fixtures\a.js"
        )));
        // Real source must still ingest.
        assert!(!should_skip_ingest_path(Path::new(
            "Chart.js-4.5.1/src/core/core.controller.js"
        )));
        assert!(!should_skip_ingest_path(Path::new(
            "src/services/ingestion_service.rs"
        )));
    }

    #[test]
    fn catch_sync_panic_converts_panic_to_err() {
        let ok = catch_sync_panic("ok", || 42).unwrap();
        assert_eq!(ok, 42);
        let err = catch_sync_panic("boom", || -> i32 { panic!("kaboom") }).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("panicked"), "{msg}");
        assert!(msg.contains("kaboom"), "{msg}");
    }

    #[tokio::test]
    async fn catch_async_panic_converts_panic_to_err() {
        let ok = catch_async_panic("ok", async { Ok::<_, anyhow::Error>(7) })
            .await
            .unwrap();
        assert_eq!(ok, 7);
        let err = catch_async_panic("async-boom", async {
            panic!("async kaboom");
            #[allow(unreachable_code)]
            Ok::<_, anyhow::Error>(0)
        })
        .await
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("panicked"), "{msg}");
        assert!(msg.contains("async kaboom"), "{msg}");
    }

    #[test]
    fn hard_split_chars_respects_char_boundaries() {
        let s = "a😀b😀c";
        let parts = hard_split_chars(s, 2);
        assert!(parts.len() >= 2);
        let rejoined: String = parts.concat();
        assert_eq!(rejoined, s);
        for p in &parts {
            assert!(p.chars().count() <= 2, "piece too long: {p:?}");
        }
    }

    #[test]
    fn map_batch_progress_clamps_and_advances() {
        assert_eq!(map_batch_progress(0, 100, 0, 4), 25);
        assert_eq!(map_batch_progress(0, 100, 3, 4), 100);
        assert_eq!(map_batch_progress(50, 50, 0, 1), 50);
        assert_eq!(map_batch_progress(10, 20, 0, 0), 20); // total_batches max(1)
    }

    #[test]
    fn clamp_text_for_tokenize_limits_chars() {
        use crate::services::embedders::dense::{clamp_text_for_tokenize, MAX_TOKENIZE_CHARS};
        let long = "x".repeat(50_000);
        let clamped = clamp_text_for_tokenize(&long);
        assert!(clamped.chars().count() <= MAX_TOKENIZE_CHARS);
        assert_eq!(clamp_text_for_tokenize("short"), "short");
    }
}
