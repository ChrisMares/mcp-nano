//! Split oversized code chunks into smaller pieces preserving their
//! language-specific subtype and metadata. Direct port of
//! `embed_code.split_oversized_code_chunks`.
//!
//! Chunks whose `code` exceeds `max_chunk_tokens` (see
//! [`crate::services::ingestion::CODE_CHUNK_SIZE`]) are re-chunked by the shared text-splitter;
//! each child becomes a `CodeChunk::split` of the original, carrying the
//! same `kind`, `repo_name`, `file_name`, `dependencies`, and `created_at`.

use text_splitter::TextSplitter;
use tokenizers::Tokenizer;

use crate::services::ingestion::types::{hard_split_chars, CodeChunk};

/// Hard char ceiling when the token splitter cannot break minified one-liners.
const MAX_CODE_CHUNK_CHARS: usize = 6_000;

/// Split chunks that the splitter breaks up (or that exceed
/// [`MAX_CODE_CHUNK_CHARS`]) into sub-chunks, preserving the per-language
/// variant on each sub-chunk.
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
        let splits = if splits.is_empty() {
            vec![chunk.code.clone()]
        } else {
            splits
        };
        let keep_original_id = splits.len() == 1
            && splits[0].chars().count() == chunk.code.chars().count()
            && chunk.code.chars().count() <= MAX_CODE_CHUNK_CHARS;
        if keep_original_id {
            out.push(chunk);
            continue;
        }
        for new_code in splits {
            for piece in hard_split_chars(&new_code, MAX_CODE_CHUNK_CHARS) {
                let new_id = uuid::Uuid::new_v4().to_string();
                out.push(chunk.split(new_id, piece));
            }
        }
    }
    out
}
