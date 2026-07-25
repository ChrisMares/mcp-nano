#!/usr/bin/env bash
# Download the embedder + reranker ONNX models + tokenizers into
# src-tauri/resources/models/. Re-runnable: skips files that already exist
# with non-zero size.
#
# Layout produced:
#   resources/models/arctic-embed-xs/{model.onnx,tokenizer.json,config.json}
#   resources/models/minilm-l6-v2/{model.onnx,tokenizer.json,config.json}
#
# BM25 stopwords are embedded directly in the Rust crate (no download).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODELS_DIR="${SCRIPT_DIR}/../resources/models"
mkdir -p "${MODELS_DIR}"

fetch() {
    local url="$1"
    local dest="$2"
    if [[ -s "${dest}" ]]; then
        echo "  [skip] ${dest} already exists"
        return
    fi
    echo "  [get]  ${dest}"
    mkdir -p "$(dirname "${dest}")"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "${url}" -o "${dest}.tmp" && mv "${dest}.tmp" "${dest}"
    else
        wget -q "${url}" -O "${dest}.tmp" && mv "${dest}.tmp" "${dest}"
    fi
}

echo "Dense embedder: Snowflake/snowflake-arctic-embed-xs (ONNX)"
DENSE_BASE="https://huggingface.co/Snowflake/snowflake-arctic-embed-xs/resolve/main"
fetch "${DENSE_BASE}/onnx/model.onnx" "${MODELS_DIR}/arctic-embed-xs/model.onnx"
fetch "${DENSE_BASE}/tokenizer.json"  "${MODELS_DIR}/arctic-embed-xs/tokenizer.json"
fetch "${DENSE_BASE}/config.json"     "${MODELS_DIR}/arctic-embed-xs/config.json"

echo "Reranker: cross-encoder/ms-marco-MiniLM-L6-v2 (ONNX)"
RERANKER_BASE="https://huggingface.co/cross-encoder/ms-marco-MiniLM-L6-v2/resolve/main"
fetch "${RERANKER_BASE}/onnx/model.onnx" "${MODELS_DIR}/minilm-l6-v2/model.onnx"
fetch "${RERANKER_BASE}/tokenizer.json"  "${MODELS_DIR}/minilm-l6-v2/tokenizer.json"
fetch "${RERANKER_BASE}/config.json"     "${MODELS_DIR}/minilm-l6-v2/config.json"

echo "Done."
ls -lh "${MODELS_DIR}"/*/
