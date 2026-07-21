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