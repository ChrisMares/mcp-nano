//! Split oversized code chunks into smaller pieces preserving their
//! language-specific subtype and metadata. Direct port of
//! `embed_code.split_oversized_code_chunks`.
//!
//! Chunks whose `code` exceeds `max_chunk_tokens` (default 1024, the Python
//! `CODE_CHUNK_SIZE` env var) are re-chunked by the shared text-splitter;
//! each child becomes a `CodeChunk::split` of the original, carrying the
//! same `kind`, `repo_name`, `file_name`, `dependencies`, and `created_at`.

use text_splitter::TextSplitter;
use tokenizers::Tokenizer;

use crate::services::ingestion::types::CodeChunk;

/// Split chunks that exceed `max_tokens` into sub-chunks of <=`max_tokens`,
/// preserving the per-language variant on each sub-chunk.
///
/// Mirrors `split_oversized_code_chunks`. The single-chunk passthrough
/// keeps the original `id` when no split is needed.
pub fn split_oversized_code_chunks(
    chunks: Vec<CodeChunk>,
    splitter: &TextSplitter<Tokenizer>,
) -> Vec<CodeChunk> {
    let mut out = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        let splits: Vec<String> = splitter.chunks(&chunk.code).map(String::from).collect();
        if splits.len() <= 1 {
            out.push(chunk);
            continue;
        }
        for new_code in splits {
            let new_id = uuid::Uuid::new_v4().to_string();
            out.push(chunk.split(new_id, new_code));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ingestion::types::CodeChunkKind;

    #[test]
    fn chunk_split_preserves_kind() {
        let chunk = CodeChunk::generic("orig", "r", "f.txt", "x = 1\n".repeat(20));
        let split = chunk.split("new", "y = 2\n");
        assert!(matches!(split.kind, CodeChunkKind::Generic));
        assert_eq!(split.id, "new");
    }
}