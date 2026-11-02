//! Ingestion pipeline: code chunkers, document loaders, website crawler,
//! and the `DocumentChunk`/`CodeChunk` shared types.
//!
//! `IngestionService` (in `services::ingestion_service`) consumes these
//! helpers via [`chunk_file_to_documents`] (code) and [`document_loaders`]
//! (non-code). The website crawler builds its own `DocumentChunk` list.

pub mod code_chunker;
pub mod document_loaders;
pub mod types;
pub mod website;

pub use types::{CodeChunk, CodeChunkKind, DocumentChunk, Parameter};

/// Document/website chunk size in tokens. The dense embedder and reranker
/// truncate at 512 tokens, and the embedding text adds a short header
/// (file name, type, page), so chunks must stay below that limit or their
/// tail is never embedded.
pub const DOC_CHUNK_SIZE: usize = 448;
/// Token overlap between adjacent document chunks.
pub const DOC_CHUNK_OVERLAP: usize = 50;
/// Code chunk size in tokens. Code embedding text starts with a structural
/// header (repo, file, class, function, parameters, return type) of about
/// 35 tokens typically and at most ~130 (it is capped by
/// `MAX_CODE_HEADER_CHARS`), so code gets a smaller budget than documents.
/// Measured on this repository, 384 keeps all but a handful of chunks within
/// the 512-token window.
pub const CODE_CHUNK_SIZE: usize = 384;
/// Token overlap between adjacent oversized-code splits.
pub const CODE_CHUNK_OVERLAP: usize = 64;
