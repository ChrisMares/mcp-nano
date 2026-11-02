use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use futures_util::{stream, FutureExt, StreamExt};
use qdrant_client::qdrant::{Condition, Filter};
use rayon::prelude::*;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use text_splitter::{ChunkConfig, MarkdownSplitter, TextSplitter};
use tokenizers::Tokenizer;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use tracing::{debug, error, info};

use crate::models::request::EmbeddingOptions;
use crate::services::embedder_state::EmbedderState;
use crate::services::embedders::EncodeDocuments;
use crate::services::file_filters::{self, IngestFilter};
use crate::services::ingestion;
use crate::services::ingestion::{
    CODE_CHUNK_OVERLAP, CODE_CHUNK_SIZE, DOC_CHUNK_OVERLAP, DOC_CHUNK_SIZE,
};
use crate::services::qdrant_service::QdrantService;
use crate::worker::{JobCancelled, ProgressCallback, TaskRegistry};

/// Hard ceiling on stored chunk text. Token splitters can still emit huge
/// pieces for single-line minified blobs; we force-split by characters after.
const MAX_CHUNK_CHARS: usize = 6_000;
/// Batch size for Qdrant upserts.
const UPSERT_BATCH_SIZE: usize = 250;
/// Batch size for dense embedding forward passes.
const EMBED_BATCH_SIZE: usize = 16;
/// How many zip files to ingest concurrently. Chunking (tree-sitter/text) and
/// sparse/dense encoding are the bulk of the work; overlapping a handful of
/// files keeps cores busy without oversubscribing the shared embedder sessions.
const ZIP_FILE_CONCURRENCY: usize = 4;

use crate::panic_payload_to_string;

fn record_caught_panic(label: &str, msg: &str) {
    error!("{label} panicked: {msg}");
    crate::write_ingest_breadcrumb("caught_panic", &format!("{label}: {msg}"));
    if let Some(dir) = crate::log_dir() {
        let path = dir.join("last-panic.log");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use std::io::Write;
            let _ = writeln!(
                f,
                "=== caught_panic ===\ntime_unix={}\nlabel={label}\npayload={msg}\n",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            );
            let _ = f.flush();
        }
    }
}

/// Run a sync closure, converting panics into `Err` so batch jobs can skip
/// one bad unit of work instead of aborting the whole task.
fn catch_sync_panic<T>(label: &str, f: impl FnOnce() -> T) -> Result<T> {
    std::panic::catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        let msg = panic_payload_to_string(&*payload);
        record_caught_panic(label, &msg);
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
            let msg = panic_payload_to_string(&*payload);
            record_caught_panic(label, &msg);
            Err(anyhow!("{label} panicked: {msg}"))
        }
    }
}

/// Convert a `spawn_blocking` join failure into an error, recording panics.
fn join_error(task: &str, path_display: &str, e: tokio::task::JoinError) -> anyhow::Error {
    if e.is_panic() {
        let msg = panic_payload_to_string(&e.into_panic());
        record_caught_panic(&format!("{task} task {path_display}"), &msg);
        anyhow!("{task} task panicked for {path_display}: {msg}")
    } else {
        anyhow!("{task} task join error for {path_display}: {e}")
    }
}

fn parse_params<T: DeserializeOwned>(params: serde_json::Value, task: &str) -> Result<T> {
    serde_json::from_value(params)
        .map_err(|e| {
            error!("{task}: invalid params: {e:#}");
            anyhow!("invalid params: {e:#}")
        })
        .with_context(|| format!("deserializing {task} params"))
}

/// Token-sized chunk config backed by the tokenizer at `tokenizer_path`.
fn chunk_config(
    tokenizer_path: &Path,
    size: usize,
    overlap: usize,
    what: &str,
) -> Result<ChunkConfig<Tokenizer>> {
    let tokenizer = Tokenizer::from_file(tokenizer_path)
        .map_err(|e| anyhow!("loading {what} tokenizer: {e}"))?;
    ChunkConfig::new(size)
        .with_sizer(tokenizer)
        .with_overlap(overlap)
        .with_context(|| format!("building {what} chunk config"))
}

fn file_name_or<'a>(path: &'a Path, fallback: &'a str) -> &'a str {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(fallback)
}

/// Report that an uploaded file is loading, failing when it no longer exists.
async fn start_upload(
    task: &str,
    path: &str,
    fallback_name: &str,
    progress: &ProgressCallback,
    cancel: &CancellationToken,
) -> Result<()> {
    ensure_not_cancelled(cancel)?;
    let display = file_name_or(Path::new(path), fallback_name);
    progress(5, Some(format!("Loading {display}"))).await;
    if !Path::new(path).exists() {
        error!("{task}: path does not exist: {path}");
        return Err(anyhow!("Path {path} does not exist"));
    }
    Ok(())
}

/// Remove an ingested upload and report completion.
async fn finish_upload(
    path: &str,
    chunks: usize,
    message: &str,
    progress: &ProgressCallback,
    cancel: &CancellationToken,
) -> Result<String> {
    ensure_not_cancelled(cancel)?;
    let _ = std::fs::remove_file(path);
    progress(100, Some(message.to_string())).await;
    Ok(format!("Processed {chunks} chunk(s)."))
}

/// Bail out of a task as soon as its job is cancelled.
fn ensure_not_cancelled(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(anyhow::Error::new(JobCancelled))
    } else {
        Ok(())
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
/// All four Python ingestion entry points (zip, documents, code files,
/// website embed) are wired end-to-end here.
pub struct IngestionService {
    embedders: Arc<EmbedderState>,
    qdrant: QdrantService,
    splitter: Arc<TextSplitter<Tokenizer>>,
    /// Second splitter used by `split_oversized_code_chunks`.
    code_splitter: Arc<TextSplitter<Tokenizer>>,
    /// Markdown-aware splitter used for `md`/`mdx` documents (CommonMark
    /// structure: headings, paragraphs, code blocks). Same size/overlap
    /// config as the plain text splitter.
    markdown_splitter: Arc<MarkdownSplitter<Tokenizer>>,
}

/// A file to ingest and where its chunks are stored.
struct FileTarget<'a> {
    path: &'a Path,
    /// Repo-relative path stored as the payload `path`; the file name when
    /// `None`.
    repo_path: Option<&'a str>,
    collection: &'a str,
    repo_name: Option<&'a str>,
    options: &'a EmbeddingOptions,
}

/// The `[start, end]` percent band a step reports progress into, and the
/// token that cancels it.
#[derive(Clone, Copy)]
struct ProgressBand<'a> {
    progress: &'a ProgressCallback,
    start: i32,
    end: i32,
    cancel: &'a CancellationToken,
}

impl<'a> ProgressBand<'a> {
    /// The band a single-file upload ingests in, after its loading step.
    fn upload(progress: &'a ProgressCallback, cancel: &'a CancellationToken) -> Self {
        Self {
            progress,
            start: 10,
            end: 95,
            cancel,
        }
    }
}

impl IngestionService {
    pub fn new(
        embedders: Arc<EmbedderState>,
        qdrant: QdrantService,
        models_dir: &Path,
    ) -> Result<Self> {
        // Each splitter owns its own `Tokenizer` instance loaded from the
        // same file. The markdown splitter shares the doc size and overlap so
        // chunk budgets stay consistent.
        let tokenizer = models_dir.join("arctic-embed-xs/tokenizer.json");
        let splitter = TextSplitter::new(chunk_config(
            &tokenizer,
            DOC_CHUNK_SIZE,
            DOC_CHUNK_OVERLAP,
            "chunk",
        )?);
        let code_splitter = TextSplitter::new(chunk_config(
            &tokenizer,
            CODE_CHUNK_SIZE,
            CODE_CHUNK_OVERLAP,
            "code",
        )?);
        let markdown_splitter = MarkdownSplitter::new(chunk_config(
            &tokenizer,
            DOC_CHUNK_SIZE,
            DOC_CHUNK_OVERLAP,
            "markdown",
        )?);

        Ok(Self {
            embedders,
            qdrant,
            splitter: Arc::new(splitter),
            code_splitter: Arc::new(code_splitter),
            markdown_splitter: Arc::new(markdown_splitter),
        })
    }

    /// Process a zip upload: unzip, walk extracted files, dispatch each to
    /// document or code ingestion based on `embedding_options.collection`.
    pub async fn process_zip_upload(
        &self,
        params: serde_json::Value,
        progress: ProgressCallback,
        cancel: CancellationToken,
    ) -> Result<String> {
        let p: ProcessZipParams = parse_params(params, "process_zip_upload")?;
        info!(
            "process_zip_upload: zip={} collection={:?}",
            p.zip_path, p.embedding_options.collection
        );
        progress(5, Some("Starting zip extraction".to_string())).await;
        ensure_not_cancelled(&cancel)?;

        let extract_dir = PathBuf::from(format!("{}_extracted", strip_zip_ext(&p.zip_path)));
        tokio::task::block_in_place(|| unzip_to(&p.zip_path, &extract_dir))
            .with_context(|| format!("unzipping {}", p.zip_path))?;
        info!("extracted zip to {}", extract_dir.display());
        ensure_not_cancelled(&cancel)?;
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
        let is_codebase = collection == "codebase";
        let needs_default = embedding_options
            .repo_name
            .as_ref()
            .is_none_or(|s| s.trim().is_empty());
        if is_codebase && needs_default {
            if let Some(zf) = p.zip_filename.as_deref().filter(|s| !s.is_empty()) {
                let default_name = repo_name_from_zip_filename(zf);
                if !default_name.is_empty() {
                    info!("process_zip_upload: defaulting repo_name to {default_name} from zip");
                    embedding_options.repo_name = Some(default_name);
                }
            }
        }
        let repo_name_for_dispatch = if is_codebase {
            embedding_options.repo_name.as_deref()
        } else {
            None
        };
        let (prior_key, prior_value, prior_kind) = if is_codebase {
            ("repo_name", repo_name_for_dispatch, "codebase")
        } else {
            ("zip_filename", p.zip_filename.as_deref(), "document")
        };
        if let Some(value) = prior_value.filter(|name| !name.is_empty()) {
            let filter = Filter::must([Condition::matches(prior_key, value.to_string())]);
            self.qdrant
                .delete_items(&collection, None, Some(filter))
                .await
                .with_context(|| format!("deleting prior {prior_kind} vectors for zip"))?;
        }

        let filter = IngestFilter::new(
            embedding_options.code_files_only.unwrap_or(true),
            embedding_options.filter_test_files.unwrap_or(true),
        );
        let mut file_paths: Vec<PathBuf> = walkdir::WalkDir::new(&extract_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .filter(|p| filter.allows(p, p.strip_prefix(&extract_dir).unwrap_or(p)))
            .collect();
        file_paths.sort();
        let strip_root = single_root_dir(&extract_dir);

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

        // Files are independent, so ingest several concurrently; concurrency
        // overlaps file I/O, chunking (tree-sitter / text splitting) and
        // encoding across files. Shared counters are atomic so progress stays
        // correct even though files finish out of order.
        let concurrency = ZIP_FILE_CONCURRENCY.min(total_files);
        let processed = AtomicUsize::new(0);
        let errors: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        // Per-file progress bands overlap when files run concurrently. Clamp
        // the percentage to a monotonic high-water mark so the bar never moves
        // backwards, but still forward every update so the current stage
        // message is always shown.
        let max_pct = AtomicI32::new(19);
        let file_progress: ProgressCallback = {
            let outer = progress.clone();
            Arc::new(move |pct, msg| -> crate::worker::progress::ProgressFuture {
                let high = max_pct.fetch_max(pct, Ordering::SeqCst);
                // Never let the bar regress, but always deliver the message:
                // the frontend clamps the percentage itself, so suppressing
                // the event here would only hide the current stage.
                outer(pct.max(high), msg)
            })
        };

        let file_futures = file_paths
            .iter()
            .enumerate()
            .map(|(idx, path)| {
                let ingest = self;
                let file_progress = file_progress.clone();
                let processed = &processed;
                let errors = &errors;
                let collection = &collection;
                let embedding_options = &embedding_options;
                let extract_dir = &extract_dir;
                let repo_name = repo_name_for_dispatch;
                let cancel = cancel.clone();
                let strip_root = strip_root.as_deref();
                async move {
                    // A cancelled zip stops before the next file starts; the
                    // worker deletes the partially written data afterwards.
                    if cancel.is_cancelled() {
                        return;
                    }
                    let file_name = file_name_or(path, "unknown");
                    let rel = path
                        .strip_prefix(extract_dir.as_path())
                        .unwrap_or(path.as_path())
                        .display()
                        .to_string();
                    let repo_path = repo_relative_path(path, extract_dir, strip_root);
                    let (file_start, file_end) = zip_file_progress_bounds(idx, total_files);
                    debug!(
                        "process_zip_upload: file {}/{} pct={}-{} path={}",
                        idx + 1,
                        total_files,
                        file_start,
                        file_end,
                        rel
                    );
                    file_progress(
                        file_start,
                        Some(format!(
                            "Processing {} ({}/{})",
                            file_name,
                            idx + 1,
                            total_files
                        )),
                    )
                    .await;

                    let file_t0 = std::time::Instant::now();
                    // One bad file must not abort the whole zip job.
                    let target = FileTarget {
                        path,
                        repo_path: Some(&repo_path),
                        collection,
                        repo_name,
                        options: embedding_options,
                    };
                    let band = ProgressBand {
                        progress: &file_progress,
                        start: file_start,
                        end: file_end,
                        cancel: &cancel,
                    };
                    let result = catch_async_panic(
                        &format!("ingest {rel}"),
                        ingest.ingest_file(target, &band),
                    )
                    .await;
                    let file_ms = file_t0.elapsed().as_millis();
                    match result {
                        Ok(n) => {
                            let total = processed.fetch_add(n, Ordering::SeqCst) + n;
                            if file_ms >= 2_000 {
                                info!(
                                    "process_zip_upload: slow file={rel} chunks={n} elapsed_ms={file_ms}"
                                );
                            }
                            debug!(
                                "process_zip_upload: ok file={rel} chunks={n} running_total={total} elapsed_ms={file_ms}"
                            );
                        }
                        Err(e) => {
                            if crate::worker::is_job_cancelled(&e) || cancel.is_cancelled() {
                                return;
                            }
                            error!(
                                "process_zip_upload: skip file={rel} elapsed_ms={file_ms}: {e:#}"
                            );
                            crate::write_ingest_breadcrumb(
                                "zip_file_error",
                                &format!("{rel} ms={file_ms}: {e:#}"),
                            );
                            if let Ok(mut guard) = errors.lock() {
                                guard.push(format!("{rel}: {e:#}"));
                            }
                            file_progress(
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
            })
            .collect::<Vec<_>>();

        stream::iter(file_futures)
            .buffer_unordered(concurrency)
            .for_each(|_| async {})
            .await;

        // Stop before reporting success: the worker deletes the archive and
        // extraction dir, then marks the job CANCELLED.
        ensure_not_cancelled(&cancel)?;

        let processed = processed.load(Ordering::SeqCst);
        let errors = errors.into_inner().unwrap_or_default();
        crate::write_ingest_breadcrumb(
            "zip_complete",
            &format!("processed_chunks={processed} errors={}", errors.len()),
        );
        progress(100, Some("Processing complete".to_string())).await;

        // Cleanup extraction dir + zip.
        let _ = std::fs::remove_dir_all(&extract_dir);
        let _ = std::fs::remove_file(&p.zip_path);

        if errors.is_empty() {
            Ok(format!("Processed {processed} chunk(s) from zip."))
        } else {
            Ok(format!(
                "Processed {processed} chunk(s); {} error(s): {}",
                errors.len(),
                errors.join("; ")
            ))
        }
    }

    /// Process a single document upload: read, chunk, embed, upsert.
    pub async fn process_documents_upload(
        &self,
        params: serde_json::Value,
        progress: ProgressCallback,
        cancel: CancellationToken,
    ) -> Result<String> {
        let task = "process_documents_upload";
        let p: ProcessDocParams = parse_params(params, task)?;
        info!("{task}: path={} group={:?}", p.path, p.group);
        start_upload(task, &p.path, "document", &progress, &cancel).await?;
        let collection = p.collection.unwrap_or_else(|| "general".to_string());
        self.qdrant.ensure_collection(&collection).await?;
        let delete_conditions = upload_conditions(
            &p.path,
            UploadScope::Group(p.group.as_deref()),
            metadata_user_id(p.metadata.as_ref()),
        );
        self.qdrant
            .delete_items(&collection, None, Some(Filter::must(delete_conditions)))
            .await
            .context("deleting prior document vectors")?;

        let options = EmbeddingOptions {
            collection: Some(collection.clone()),
            repo_name: None,
            group: p.group,
            filter_test_files: None,
            code_files_only: None,
            metadata: p.metadata,
        };
        let target = FileTarget {
            path: Path::new(&p.path),
            repo_path: None,
            collection: &collection,
            repo_name: None,
            options: &options,
        };
        let n = catch_async_panic(
            &format!("document {}", p.path),
            self.ingest_file(target, &ProgressBand::upload(&progress, &cancel)),
        )
        .await?;
        finish_upload(
            &p.path,
            n,
            "Document processing complete",
            &progress,
            &cancel,
        )
        .await
    }

    /// Process a single code file upload: read, chunk, embed, upsert.
    pub async fn process_code_file_upload(
        &self,
        params: serde_json::Value,
        progress: ProgressCallback,
        cancel: CancellationToken,
    ) -> Result<String> {
        let task = "process_code_file_upload";
        let p: ProcessCodeFileParams = parse_params(params, task)?;
        info!("{task}: path={} collection={}", p.path, p.collection);
        start_upload(task, &p.path, "code", &progress, &cancel).await?;
        self.qdrant.ensure_collection(&p.collection).await?;
        let delete_conditions = upload_conditions(
            &p.path,
            UploadScope::Repo(p.repo_name.as_deref()),
            metadata_user_id(p.metadata.as_ref()),
        );
        self.qdrant
            .delete_items(&p.collection, None, Some(Filter::must(delete_conditions)))
            .await
            .context("deleting prior code file vectors")?;

        let options = EmbeddingOptions {
            collection: Some(p.collection.clone()),
            repo_name: p.repo_name.clone(),
            group: None,
            filter_test_files: None,
            code_files_only: None,
            metadata: p.metadata,
        };
        let target = FileTarget {
            path: Path::new(&p.path),
            repo_path: None,
            collection: &p.collection,
            repo_name: p.repo_name.as_deref(),
            options: &options,
        };
        let n = catch_async_panic(
            &format!("code file {}", p.path),
            self.ingest_file(target, &ProgressBand::upload(&progress, &cancel)),
        )
        .await?;
        finish_upload(
            &p.path,
            n,
            "Code file processing complete",
            &progress,
            &cancel,
        )
        .await
    }

    /// Website embedding: scrape + chunk + embed + upsert. Mirrors the
    /// Python `process_website_embed`.
    pub async fn process_website_embed(
        &self,
        params: serde_json::Value,
        progress: ProgressCallback,
        cancel: CancellationToken,
    ) -> Result<String> {
        let p: ProcessWebsiteParams = parse_params(params, "process_website_embed")?;
        info!(
            "process_website_embed: {} urls, group={}",
            p.urls.len(),
            p.group
        );

        let mut base_metadata = serde_json::Map::new();
        base_metadata.insert("group".into(), serde_json::Value::String(p.group.clone()));
        for (k, v) in p
            .metadata
            .unwrap_or_default()
            .as_object()
            .into_iter()
            .flatten()
        {
            base_metadata.insert(k.clone(), v.clone());
        }

        struct HtmlCacheClearGuard;
        impl Drop for HtmlCacheClearGuard {
            fn drop(&mut self) {
                ingestion::website::clear_html_cache();
            }
        }
        let _cache_guard = HtmlCacheClearGuard;

        ensure_not_cancelled(&cancel)?;
        progress(5, Some(format!("Scraping {} page(s)", p.urls.len()))).await;
        let chunks = catch_async_panic("website scrape", async {
            ingestion::website::process_website_with_progress(
                &p.urls,
                &base_metadata,
                Some(&self.splitter),
                p.render_javascript.unwrap_or(false),
                Some(&progress),
                Some(&cancel),
            )
            .await
            .context("scraping + chunking website pages")
        })
        .await?;
        ensure_not_cancelled(&cancel)?;
        if chunks.is_empty() {
            return Err(anyhow!("No content extracted from any URL"));
        }
        self.qdrant.ensure_collection("general").await?;
        progress(30, Some(format!("Embedding {} chunks", chunks.len()))).await;
        let band = ProgressBand {
            progress: &progress,
            start: 30,
            end: 100,
            cancel: &cancel,
        };
        let n = catch_async_panic(
            "website embed",
            self.embed_and_upsert_documents(&chunks, "general", serde_json::Map::new(), &band),
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
        reg.register("process_zip", move |params, progress, cancel| {
            let ingest = ingest.clone();
            async move { ingest.process_zip_upload(params, progress, cancel).await }
        });
        let ingest = self.clone();
        reg.register(
            "process_documents_upload",
            move |params, progress, cancel| {
                let ingest = ingest.clone();
                async move {
                    ingest
                        .process_documents_upload(params, progress, cancel)
                        .await
                }
            },
        );
        let ingest = self.clone();
        reg.register(
            "process_code_file_upload",
            move |params, progress, cancel| {
                let ingest = ingest.clone();
                async move {
                    ingest
                        .process_code_file_upload(params, progress, cancel)
                        .await
                }
            },
        );
        let ingest = self.clone();
        reg.register("process_website_scrape", move |params, progress, cancel| {
            let ingest = ingest.clone();
            async move { ingest.process_website_embed(params, progress, cancel).await }
        });
        reg
    }

    /// Shared chunk + embed + upsert path. Returns the number of chunks
    /// upserted. Dispatches code files (by extension) through the language-
    /// specific chunkers; everything else goes through `document_loaders`
    /// then the text-splitter.
    async fn ingest_file(&self, target: FileTarget<'_>, band: &ProgressBand<'_>) -> Result<usize> {
        let FileTarget {
            path,
            repo_path,
            collection,
            repo_name,
            options,
        } = target;
        let ProgressBand {
            progress,
            start: range_start,
            cancel,
            ..
        } = *band;
        if file_filters::is_junk(path) {
            return Ok(0);
        }
        ensure_not_cancelled(cancel)?;
        let file_name = strip_job_prefix(file_name_or(path, "unknown"));
        let display = file_name.clone();
        let doc_type = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("txt")
            .to_lowercase();

        // Load + chunk are CPU-bound and can take minutes on large PDFs.
        // Report distinct stages so the UI is not frozen on one percentage.
        let load_pct = (range_start.saturating_sub(5)).max(5).min(range_start);
        let chunk_pct = load_pct.saturating_add(2).min(range_start);
        progress(load_pct, Some(format!("Extracting text from {display}"))).await;

        let path_buf = path.to_path_buf();
        let path_display = path.display().to_string();
        // Match Python ZIP routing: general uploads treat every file as a
        // document; only codebase uploads use language-specific chunkers.
        let is_code = collection == "codebase" && ingestion::code_chunker::is_code_file(path);
        let chunks: Vec<ingestion::DocumentChunk> = if is_code {
            progress(chunk_pct, Some(format!("Chunking code in {display}"))).await;
            let repo = repo_name.unwrap_or("");
            let name_override = Some(file_name.clone());
            let t0 = std::time::Instant::now();
            let code_chunks = catch_sync_panic(&format!("code chunker {path_display}"), || {
                ingestion::code_chunker::chunk_file_to_documents(
                    &path_buf,
                    repo,
                    name_override.as_deref(),
                    &self.code_splitter,
                    CODE_CHUNK_SIZE,
                )
            })?;
            let chunk_ms = t0.elapsed().as_millis();
            debug!(
                "ingest: chunked code {} -> {} chunk(s) elapsed_ms={chunk_ms}",
                path_display,
                code_chunks.len()
            );
            if chunk_ms >= 2_000 {
                info!(
                    "ingest: slow code chunk {} chunks={} elapsed_ms={chunk_ms}",
                    path_display,
                    code_chunks.len()
                );
            }
            enforce_max_chunk_chars(code_chunks)
        } else {
            let path_for_load = path_buf.clone();
            let load_t0 = std::time::Instant::now();
            let mut loaded = tokio::task::spawn_blocking(move || {
                catch_sync_panic(
                    &format!("document loader {}", path_for_load.display()),
                    || ingestion::document_loaders::load_document(&path_for_load),
                )
                .and_then(|r| {
                    r.with_context(|| format!("loading document {}", path_for_load.display()))
                })
            })
            .await
            .map_err(|e| join_error("load", &path_display, e))??;
            for chunk in &mut loaded {
                chunk.file_name = file_name.clone();
                chunk.metadata.insert(
                    "source".into(),
                    serde_json::Value::String(file_name.clone()),
                );
            }
            let load_ms = load_t0.elapsed().as_millis();
            debug!("ingest: loaded document path={} ms={load_ms}", path_display);

            let source_pages = loaded.len();
            let source_chars: usize = loaded.iter().map(|chunk| chunk.content.len()).sum();
            progress(
                chunk_pct,
                Some(format!(
                    "Chunking text from {display} ({source_pages} source document(s))"
                )),
            )
            .await;
            let split_t0 = std::time::Instant::now();
            debug!(
                "ingest: splitting document path={} source_documents={} source_chars={}",
                path_display, source_pages, source_chars
            );
            let splitter = self.splitter.clone();
            let markdown_splitter = self.markdown_splitter.clone();
            let split_path_display = path_display.clone();
            let (split_progress_tx, mut split_progress_rx) =
                tokio::sync::mpsc::channel::<ChunkingProgress>(8);
            let split_task = tokio::task::spawn_blocking(move || {
                catch_sync_panic(&format!("text splitter {split_path_display}"), || {
                    split_document_chunks_with_progress(
                        loaded,
                        &splitter,
                        &markdown_splitter,
                        |snapshot| {
                            let _ = split_progress_tx.blocking_send(snapshot);
                        },
                    )
                })
            });
            while let Some(snapshot) = split_progress_rx.recv().await {
                let pct = chunking_progress_percent(
                    chunk_pct,
                    range_start,
                    snapshot.completed,
                    snapshot.total,
                );
                progress(
                    pct,
                    Some(format!(
                        "Chunking text: {}/{} source document(s), {} chunks",
                        snapshot.completed, snapshot.total, snapshot.output_chunks
                    )),
                )
                .await;
            }
            let split_chunks = split_task
                .await
                .map_err(|e| join_error("split", &path_display, e))??;
            let split_ms = split_t0.elapsed().as_millis();
            debug!(
                "ingest: split document path={} source_documents={} source_chars={} chunks={} elapsed_ms={split_ms}",
                path_display, source_pages, source_chars, split_chunks.len()
            );
            split_chunks
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
        let extra_meta = build_base_metadata(
            &file_name,
            repo_path.unwrap_or(&file_name),
            &doc_type,
            repo_name,
            options,
            file_filters::data_file_stats(path),
        );
        self.embed_and_upsert_documents(&chunks, collection, extra_meta, band)
            .await
            .with_context(|| format!("embed/upsert failed for {path_display}"))
    }

    /// Embed a list of prebuilt `DocumentChunk`s into Qdrant, merging
    /// `extra_meta` into every chunk's metadata. Progress is mapped onto
    /// `band`.
    async fn embed_and_upsert_documents(
        &self,
        chunks: &[ingestion::DocumentChunk],
        collection: &str,
        extra_meta: serde_json::Map<String, serde_json::Value>,
        band: &ProgressBand<'_>,
    ) -> Result<usize> {
        let ProgressBand {
            progress,
            start: range_start,
            end: range_end,
            cancel,
        } = *band;
        if chunks.is_empty() {
            return Ok(0);
        }
        debug!(
            "embed_and_upsert: {} chunks → {collection} [{range_start}..{range_end}]",
            chunks.len()
        );

        let texts: Vec<String> = chunks.iter().map(|c| c.chunk_embedding_text()).collect();
        let documents: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();

        // Each forward pass pads to its longest input, so batching texts of
        // similar length avoids wasting compute on padding tokens. Results are
        // written back to their original positions.
        let order = order_by_length(&texts);

        let total_batches = (chunks.len().div_ceil(EMBED_BATCH_SIZE) as i32).max(1);
        let mut embeddings_list: Vec<Vec<f32>> = vec![Vec::new(); chunks.len()];
        for batch_index in 0..total_batches {
            ensure_not_cancelled(cancel)?;
            let batch_start = (batch_index as usize) * EMBED_BATCH_SIZE;
            let batch_end = (batch_start + EMBED_BATCH_SIZE).min(chunks.len());
            let batch_positions = &order[batch_start..batch_end];
            let mut batch_texts: Vec<&str> = Vec::with_capacity(batch_positions.len());
            for &position in batch_positions {
                batch_texts.push(texts[position].as_str());
            }
            let batch_embeddings = tokio::task::block_in_place(|| {
                catch_sync_panic(
                    &format!("dense encode batch {}/{}", batch_index + 1, total_batches),
                    || {
                        self.embedders
                            .dense
                            .encode_documents(&batch_texts, EMBED_BATCH_SIZE)
                    },
                )
            })?
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
            for (&position, embedding) in batch_positions.iter().zip(batch_embeddings) {
                embeddings_list[position] = embedding;
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
                for (k, v) in &extra_meta {
                    meta.insert(k.clone(), v.clone());
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

        progress(
            range_end,
            Some(format!("Upserting {} chunks to Qdrant", chunks.len())),
        )
        .await;

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

/// Re-tokenize a list of `DocumentChunk`s. Each input chunk whose `content`
/// exceeds the splitter's chunk size becomes a sequence of smaller chunks
/// inheriting the same metadata. Used by
/// `IngestionService::ingest_file` for non-code uploads. `md`/`mdx`
/// chunks are split by the markdown-aware splitter (structure-aligned);
/// everything else uses the plain text splitter.
///
/// After the token splitter, any remaining piece larger than
/// [`MAX_CHUNK_CHARS`] is hard-split by character count (minified one-liners
/// often don't break under the token splitter alone).
pub fn split_document_chunks(
    chunks: Vec<ingestion::DocumentChunk>,
    splitter: &TextSplitter<Tokenizer>,
    markdown_splitter: &MarkdownSplitter<Tokenizer>,
) -> Vec<ingestion::DocumentChunk> {
    split_document_chunks_with_progress(chunks, splitter, markdown_splitter, |_| {})
}

#[derive(Debug, Clone, Copy)]
struct ChunkingProgress {
    completed: usize,
    total: usize,
    output_chunks: usize,
}

fn chunking_progress_percent(start: i32, end: i32, completed: usize, total: usize) -> i32 {
    let span = (end - start).max(0);
    let total = total.max(1) as i32;
    (start + span * completed as i32 / total).clamp(0, 100)
}

fn split_document_chunks_with_progress(
    chunks: Vec<ingestion::DocumentChunk>,
    splitter: &TextSplitter<Tokenizer>,
    markdown_splitter: &MarkdownSplitter<Tokenizer>,
    report: impl Fn(ChunkingProgress) + Sync,
) -> Vec<ingestion::DocumentChunk> {
    if chunks.is_empty() {
        return chunks;
    }
    let total = chunks.len();
    // Source documents are independent, so split them in parallel. Progress is
    // driven by monotonic atomic counters rather than a loop index: each
    // `fetch_add` returns a unique consecutive value, so every `% 100` /
    // `== total` milestone fires exactly once, from exactly one worker, and the
    // reported count never moves backwards.
    let completed = AtomicUsize::new(0);
    let output_chunks = AtomicUsize::new(0);
    let per_document: Vec<Vec<ingestion::DocumentChunk>> = chunks
        .into_par_iter()
        .map(|chunk| {
            let parts: Vec<String> = if matches!(chunk.doc_type.as_str(), "md" | "mdx") {
                markdown_splitter
                    .chunks(&chunk.content)
                    .map(String::from)
                    .collect()
            } else {
                splitter.chunks(&chunk.content).map(String::from).collect()
            };
            let parts = if parts.is_empty() {
                vec![chunk.content.clone()]
            } else {
                parts
            };
            let mut doc_out: Vec<ingestion::DocumentChunk> = Vec::new();
            for sub in parts {
                for piece in ingestion::types::hard_split_chars(&sub, MAX_CHUNK_CHARS) {
                    let mut child = chunk.clone();
                    child.id = Uuid::new_v4().to_string();
                    child.content = piece;
                    doc_out.push(child);
                }
            }
            output_chunks.fetch_add(doc_out.len(), Ordering::SeqCst);
            // Avoid one database/event write per PDF page while preserving
            // useful visibility for very large documents.
            let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
            if done == total || done.is_multiple_of(100) {
                report(ChunkingProgress {
                    completed: done,
                    total,
                    output_chunks: output_chunks.load(Ordering::SeqCst),
                });
            }
            doc_out
        })
        .collect();

    // Python assigns indexes after splitting every source document. `collect`
    // preserves source order, so flattening here yields the same index
    // sequence as the previous sequential assignment.
    let mut out: Vec<ingestion::DocumentChunk> = Vec::new();
    for mut child in per_document.into_iter().flatten() {
        child.chunk_index = out.len() as i64;
        out.push(child);
    }
    // Guarantee a final update with the exact totals so the bar lands on the
    // band boundary even when `total` is not a multiple of 100.
    report(ChunkingProgress {
        completed: total,
        total,
        output_chunks: out.len(),
    });
    out
}

/// Indices of `texts` ordered from shortest to longest (stable for ties).
fn order_by_length(texts: &[String]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..texts.len()).collect();
    order.sort_by_key(|&i| texts[i].len());
    order
}

/// Strip the `<job uuid>_` prefix that uploads receive in the temp
/// directory. Accepts both hyphenated and simple (32 hex) UUID forms, since
/// older builds wrote the simple form. Names without a UUID prefix are
/// returned unchanged.
pub fn strip_job_prefix(name: &str) -> String {
    if let Some((prefix, rest)) = name.split_once('_') {
        if !rest.is_empty() && Uuid::try_parse(prefix).is_ok() {
            return rest.to_string();
        }
    }
    name.to_string()
}

/// The directory GitHub-style archives wrap everything in (`repo-main/`),
/// when `extract_dir` holds exactly one entry and it is a directory.
fn single_root_dir(extract_dir: &Path) -> Option<PathBuf> {
    let mut entries = std::fs::read_dir(extract_dir).ok()?;
    let first = entries.next()?.ok()?;
    if entries.next().is_some() || !first.file_type().ok()?.is_dir() {
        return None;
    }
    Some(first.path())
}

/// Path of `file` inside the uploaded repository, with `/` separators and
/// without the archive's wrapping directory, e.g. `src/Fl_Button.cxx`.
fn repo_relative_path(file: &Path, extract_dir: &Path, root: Option<&Path>) -> String {
    let base = root.unwrap_or(extract_dir);
    let relative = file
        .strip_prefix(base)
        .or_else(|_| file.strip_prefix(extract_dir))
        .unwrap_or(file);
    let mut parts: Vec<String> = Vec::new();
    for component in relative.components() {
        parts.push(component.as_os_str().to_string_lossy().into_owned());
    }
    parts.join("/")
}

/// What a single-file upload is scoped by besides its file name: documents
/// by `group`, code files by `repo_name`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum UploadScope<'a> {
    Group(Option<&'a str>),
    Repo(Option<&'a str>),
}

/// `user_id` from an upload's user metadata.
pub(crate) fn metadata_user_id(metadata: Option<&serde_json::Value>) -> Option<&str> {
    metadata
        .and_then(|m| m.pointer("/user_id"))
        .and_then(|v| v.as_str())
}

/// Qdrant conditions matching the chunks of a single uploaded file: its
/// `file_name` (job prefix stripped), plus its non-empty [`UploadScope`]
/// value and `user_id` when given. A re-upload replaces exactly these chunks.
pub(crate) fn upload_conditions(
    path: &str,
    scope: UploadScope<'_>,
    user_id: Option<&str>,
) -> Vec<Condition> {
    let file_name = strip_job_prefix(file_name_or(Path::new(path), "unknown"));
    let mut conditions = vec![Condition::matches("file_name", file_name)];
    let (key, value) = match scope {
        UploadScope::Group(group) => ("group", group),
        UploadScope::Repo(repo_name) => ("repo_name", repo_name),
    };
    if let Some(value) = value.filter(|v| !v.is_empty()) {
        conditions.push(Condition::matches(key, value.to_string()));
    }
    if let Some(user_id) = user_id {
        conditions.push(Condition::matches("user_id", user_id.to_string()));
    }
    conditions
}

/// Build the base metadata payload: file_name, path, doc_type, repo_name,
/// group, user-provided metadata, and `file_lines`/`file_bytes` for data
/// files. Chunk-specific fields (chunk_index, content) are added by the
/// caller.
fn build_base_metadata(
    file_name: &str,
    repo_path: &str,
    doc_type: &str,
    repo_name: Option<&str>,
    options: &EmbeddingOptions,
    data_stats: Option<file_filters::DataFileStats>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    map.insert("file_name".into(), file_name.into());
    map.insert("path".into(), repo_path.into());
    map.insert("doc_type".into(), doc_type.into());
    if let Some(rn) = repo_name {
        map.insert("repo_name".into(), rn.into());
    }
    if let Some(group) = &options.group {
        map.insert("group".into(), group.as_str().into());
    }
    if let Some(serde_json::Value::Object(user_meta)) = &options.metadata {
        for (k, v) in user_meta {
            map.insert(k.clone(), v.clone());
        }
    }
    if let Some(stats) = data_stats {
        map.insert("file_lines".into(), stats.lines.into());
        map.insert("file_bytes".into(), stats.bytes.into());
    }
    map.insert(
        "created_at".into(),
        crate::worker::progress::now_iso().into(),
    );
    map
}

pub(crate) fn strip_zip_ext(zip_path: &str) -> String {
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

fn enforce_max_chunk_chars(chunks: Vec<ingestion::DocumentChunk>) -> Vec<ingestion::DocumentChunk> {
    // Chunks are independent; splitting oversized ones is CPU-bound, so run it
    // in parallel. `flat_map` preserves input order, and each child's
    // `chunk_index` is derived locally from its parent, so the output matches
    // the previous sequential loop exactly.
    chunks
        .into_par_iter()
        .map(|chunk| {
            if chunk.content.chars().count() <= MAX_CHUNK_CHARS {
                return vec![chunk];
            }
            ingestion::types::hard_split_chars(&chunk.content, MAX_CHUNK_CHARS)
                .into_iter()
                .enumerate()
                .map(|(i, piece)| {
                    let mut child = chunk.clone();
                    child.id = Uuid::new_v4().to_string();
                    child.content = piece;
                    child.chunk_index = chunk.chunk_index + i as i64;
                    child
                })
                .collect()
        })
        .flatten()
        .collect()
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
    render_javascript: Option<bool>,
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
    fn strip_job_prefix_accepts_both_uuid_forms() {
        assert_eq!(
            strip_job_prefix("123e4567-e89b-12d3-a456-426614174000_report.pdf"),
            "report.pdf"
        );
        assert_eq!(
            strip_job_prefix("123e4567e89b12d3a456426614174000_report.pdf"),
            "report.pdf"
        );
        assert_eq!(strip_job_prefix("my_report.pdf"), "my_report.pdf");
        assert_eq!(strip_job_prefix("report.pdf"), "report.pdf");
    }

    #[test]
    fn order_by_length_sorts_shortest_first_and_keeps_every_index() {
        let texts = vec!["ccc".to_string(), "a".to_string(), "bb".to_string()];
        assert_eq!(order_by_length(&texts), vec![1, 2, 0]);
    }

    #[test]
    fn repo_name_from_zip_filename_strips_extension() {
        assert_eq!(
            repo_name_from_zip_filename("OptionsPricing-main.zip"),
            "OptionsPricing-main"
        );
        assert_eq!(repo_name_from_zip_filename("MyRepo.ZIP"), "MyRepo");
        assert_eq!(repo_name_from_zip_filename("/path/to/repo.zip"), "repo");
        assert_eq!(repo_name_from_zip_filename("noext"), "noext");
    }

    #[test]
    fn repo_relative_path_strips_the_archive_wrapping_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("fltk-master");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/Fl.cxx"), "").unwrap();

        let wrapper = single_root_dir(dir.path());
        assert_eq!(wrapper.as_deref(), Some(root.as_path()));
        assert_eq!(
            repo_relative_path(&root.join("src/Fl.cxx"), dir.path(), wrapper.as_deref()),
            "src/Fl.cxx"
        );
    }

    #[test]
    fn repo_relative_path_keeps_top_level_when_archive_has_several_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("README.md"), "").unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "").unwrap();

        let wrapper = single_root_dir(dir.path());
        assert!(wrapper.is_none());
        assert_eq!(
            repo_relative_path(&dir.path().join("src/main.rs"), dir.path(), None),
            "src/main.rs"
        );
    }

    #[test]
    fn build_base_metadata_includes_required_fields() {
        let opts = EmbeddingOptions {
            collection: Some("general".to_string()),
            repo_name: None,
            group: Some("docs".to_string()),
            filter_test_files: None,
            code_files_only: None,
            metadata: Some(serde_json::json!({"source": "manual"})),
        };
        let map = build_base_metadata("notes.txt", "notes.txt", "txt", None, &opts, None);
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
            filter_test_files: None,
            code_files_only: None,
            metadata: None,
        };
        let map = build_base_metadata("main.py", "src/main.py", "py", Some("my_repo"), &opts, None);
        assert_eq!(map.get("repo_name").unwrap().as_str().unwrap(), "my_repo");
        assert_eq!(map.get("path").unwrap().as_str().unwrap(), "src/main.py");
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
    fn chunking_progress_stays_within_its_reserved_band() {
        let updates = [
            chunking_progress_percent(7, 10, 1, 4),
            chunking_progress_percent(7, 10, 2, 4),
            chunking_progress_percent(7, 10, 3, 4),
            chunking_progress_percent(7, 10, 4, 4),
        ];
        assert_eq!(updates.last(), Some(&10));
        assert!(updates.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(updates.iter().all(|pct| (7..=10).contains(pct)));
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

    fn test_splitters() -> Option<(TextSplitter<Tokenizer>, MarkdownSplitter<Tokenizer>)> {
        let tok_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/models/arctic-embed-xs/tokenizer.json");
        if !tok_path.exists() {
            return None;
        }
        let config = || chunk_config(&tok_path, DOC_CHUNK_SIZE, DOC_CHUNK_OVERLAP, "test").ok();
        Some((
            TextSplitter::new(config()?),
            MarkdownSplitter::new(config()?),
        ))
    }

    fn assert_valid_split(split: &[ingestion::DocumentChunk]) {
        assert!(!split.is_empty(), "expected at least one chunk after split");
        assert!(
            split
                .iter()
                .map(|c| c.content.chars().count())
                .max()
                .unwrap_or(0)
                <= MAX_CHUNK_CHARS,
            "chunk exceeds hard char cap {MAX_CHUNK_CHARS}"
        );
        assert!(
            split
                .iter()
                .enumerate()
                .all(|(index, chunk)| chunk.chunk_index == index as i64),
            "split chunks must have globally sequential indexes"
        );
        assert!(
            split.iter().all(|c| !c.id.is_empty()),
            "split chunks must have fresh ids"
        );
    }

    fn code_sample(file: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/test_data/code_samples")
            .join(file)
    }

    /// Reproduces the OptionsPricing zip issue: source maps / minified assets
    /// were stored as ~1MB `document` payloads. After the fix, every chunk
    /// must stay under the hard char cap.
    #[test]
    fn split_document_chunks_caps_huge_minified_blob() {
        let Some((splitter, markdown_splitter)) = test_splitters() else {
            eprintln!("skipping: arctic tokenizer not present");
            return;
        };
        // Single-line ~900KB blob (like a .js.map) — no natural newlines.
        let huge = "a".repeat(900_000);
        let chunk = ingestion::DocumentChunk::new("id1", "chart.umd.min.js.map", huge, "map", 0);
        let split = split_document_chunks(vec![chunk], &splitter, &markdown_splitter);
        assert!(!split.is_empty(), "expected at least one chunk after split");
        let max_chars = split
            .iter()
            .map(|c| c.content.chars().count())
            .max()
            .unwrap_or(0);
        assert!(
            max_chars <= MAX_CHUNK_CHARS,
            "chunk still oversized: max_chars={max_chars} limit={MAX_CHUNK_CHARS} n_chunks={}",
            split.len()
        );
        let total: usize = split.iter().map(|c| c.content.len()).sum();
        assert!(total >= 900_000 - 100, "lost content during split: {total}");
        assert!(
            split
                .iter()
                .enumerate()
                .all(|(index, chunk)| chunk.chunk_index == index as i64),
            "split chunks must have globally sequential indexes"
        );
    }

    #[test]
    fn split_document_chunks_keeps_page_metadata() {
        let Some((splitter, markdown_splitter)) = test_splitters() else {
            eprintln!("skipping: arctic tokenizer not present");
            return;
        };
        let mut first = ingestion::DocumentChunk::new("id1", "guide.pdf", "first page", "pdf", 0);
        first.page = Some(0);
        let mut second = ingestion::DocumentChunk::new("id2", "guide.pdf", "second page", "pdf", 1);
        second.page = Some(1);

        let split = split_document_chunks(vec![first, second], &splitter, &markdown_splitter);
        assert_eq!(split.len(), 2);
        assert_eq!(split[0].page, Some(0));
        assert_eq!(split[1].page, Some(1));
        assert_eq!(split[0].chunk_index, 0);
        assert_eq!(split[1].chunk_index, 1);
    }

    /// Oversized synthetic markdown: headings must survive the split intact
    /// (never cut mid-line) and both `#` sections end up represented.
    #[test]
    fn split_markdown_chunks_align_to_headings() {
        let Some((splitter, markdown_splitter)) = test_splitters() else {
            eprintln!("skipping: arctic tokenizer not present");
            return;
        };
        let filler = |word: &str, count: usize| {
            std::iter::repeat_n(
                format!("The {word} section discusses semantic chunking boundaries in detail. "),
                count,
            )
            .collect::<Vec<_>>()
            .join("\n\n")
        };
        let content = format!(
            "# Alpha Overview\n\n{}\n\n# Beta Reference\n\n{}",
            filler("alpha", 60),
            filler("beta", 60)
        );
        let chunk = ingestion::DocumentChunk::new("id1", "guide.md", content.clone(), "md", 0);

        let split = split_document_chunks(vec![chunk], &splitter, &markdown_splitter);
        assert!(
            split.len() >= 2,
            "expected multiple chunks, got {}",
            split.len()
        );
        assert_valid_split(&split);
        assert!(split.iter().all(|c| c.doc_type == "md"));
        assert!(split.iter().all(|c| c.file_name == "guide.md"));
        for heading in ["# Alpha Overview", "# Beta Reference"] {
            assert!(
                split.iter().any(|c| c.content.contains(heading)),
                "heading {heading:?} missing from all chunks"
            );
        }
        // Headings must never be split mid-line: any chunk mentioning the
        // section name carries the full heading line.
        for c in &split {
            if c.content.contains("Overview") {
                assert!(c.content.contains("# Alpha Overview"));
            }
            if c.content.contains("Reference") {
                assert!(c.content.contains("# Beta Reference"));
            }
        }
        let combined: String = split.iter().map(|c| c.content.as_str()).collect();
        assert!(combined.contains("semantic chunking boundaries"));
    }

    /// Real-world MDX fixture (LangChain docs index): frontmatter, directive
    /// containers (`:::python`), JSX blocks (`<CodeGroup>`, `<Tip>`), fenced
    /// code, and `##` sections must all survive chunking.
    #[test]
    fn split_mdx_index_fixture() {
        let Some((splitter, markdown_splitter)) = test_splitters() else {
            eprintln!("skipping: arctic tokenizer not present");
            return;
        };
        let loaded = ingestion::document_loaders::load_document(&code_sample("index.mdx"))
            .expect("loading index.mdx fixture");
        assert_eq!(loaded.len(), 1);

        let split = split_document_chunks(loaded, &splitter, &markdown_splitter);
        assert_valid_split(&split);
        assert!(split.iter().all(|c| c.doc_type == "mdx"));
        assert!(split.iter().all(|c| c.file_name == "index.mdx"));
        let combined: String = split.iter().map(|c| c.content.as_str()).collect();
        for heading in [
            "## Text structure-based",
            "## Length-based",
            "## Document structure-based",
        ] {
            assert!(
                combined.contains(heading),
                "heading {heading:?} lost during mdx split"
            );
        }
        assert!(combined.contains("title: \"Text splitter integrations\""));
        assert!(combined.contains("pip install -U langchain-text-splitters"));
        assert!(combined.contains("RecursiveCharacterTextSplitter"));
        assert!(combined.contains("<CodeGroup>"));
    }

    /// Real-world MDX fixture (LangChain split-by-token guide): guaranteed
    /// multi-chunk at this size; section headings, JSX, `@[Component]` refs
    /// and UTF-8 Korean text must survive.
    #[test]
    fn split_mdx_split_by_token_fixture() {
        let Some((splitter, markdown_splitter)) = test_splitters() else {
            eprintln!("skipping: arctic tokenizer not present");
            return;
        };
        let loaded = ingestion::document_loaders::load_document(&code_sample("split_by_token.mdx"))
            .expect("loading split_by_token.mdx fixture");
        assert_eq!(loaded.len(), 1);

        let split = split_document_chunks(loaded, &splitter, &markdown_splitter);
        assert!(
            split.len() >= 2,
            "expected multiple chunks, got {}",
            split.len()
        );
        assert_valid_split(&split);
        assert!(split.iter().all(|c| c.doc_type == "mdx"));
        let combined: String = split.iter().map(|c| c.content.as_str()).collect();
        for heading in [
            "## tiktoken",
            "## js-tiktoken",
            "## spaCy",
            "## SentenceTransformers",
            "## NLTK",
            "## KoNLPY",
            "## Hugging Face tokenizer",
        ] {
            assert!(
                combined.contains(heading),
                "heading {heading:?} lost during mdx split"
            );
        }
        assert!(combined.contains("@[CharacterTextSplitter]"));
        assert!(combined.contains("춘향전"));
        assert!(combined.contains("<Note>"));
    }

    /// Plain `.md` regression: the committed sample flows through the
    /// markdown-aware splitter without losing content.
    #[test]
    fn split_markdown_sample_md() {
        let Some((splitter, markdown_splitter)) = test_splitters() else {
            eprintln!("skipping: arctic tokenizer not present");
            return;
        };
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test_data/documents/sample.md");
        let loaded =
            ingestion::document_loaders::load_document(&path).expect("loading sample.md fixture");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].doc_type, "md");

        let split = split_document_chunks(loaded, &splitter, &markdown_splitter);
        assert_valid_split(&split);
        assert!(split.iter().all(|c| c.doc_type == "md"));
        let combined: String = split.iter().map(|c| c.content.as_str()).collect();
        assert!(combined.contains("Markdown Sample"));
        assert!(combined.contains("Bullet one"));
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
        let parts = ingestion::types::hard_split_chars(s, 2);
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
