# mcp-nano — VectorFlow Rust Rewrite Plan

## Overview

Rewrite VectorFlow as a single native desktop application using **Tauri 2** (Rust + React).
Zero Docker, zero Python, zero network dependency after first download.
Qdrant runs as a bundled sidecar child process. Models ship as bundled resource
files next to the executable (not compiled into it).

**Target platforms:** Windows (x86_64), Linux (x86_64)

**Home:** this repository (`mcp-nano`), scaffolded from the official Tauri 2
react-ts template with the VectorFlowUI React app ported in.

---

## Architecture

```
┌─────────────────────────────────────────┐
│              Tauri Process               │
│                                          │
│  ┌────────────────────────────────────┐  │
│  │  OS Webview (webkit2gtk on Linux)  │  │
│  │  ┌──────────────────────────────┐  │  │
│  │  │ React/Vite/Tailwind (as-is)  │  │  │
│  │  │ talks to Rust via invoke()   │  │  │
│  │  └──────────────┬───────────────┘  │  │
│  └─────────────────┼──────────────────┘  │
│                    │ Tauri IPC           │
│  ┌─────────────────▼──────────────────┐  │
│  │  Controllers (#[tauri::command])   │  │
│  │  rag / jobs / data / mcpconfig /   │  │
│  │  website — thin wrappers over core │  │
│  └────────┬──────────────┬────────────┘  │
│           │              │               │
│     ┌─────▼─────┐  ┌────▼──────────────┐│
│     │ SQLite    │  │ Worker Poll Loop  ││
│     │ (sqlx)    │  │ async_runtime +   ││
│     │ app.db    │  │ Semaphore(2)      ││
│     └───────────┘  └────┬──────────────┘│
│                          │               │
│  ┌───────────────────────▼─────────────┐ │
│  │  Axum (one route only)             │ │
│  │  POST /mcp  →  rmcp handler        │ │
│  │  External AI tools connect here     │ │
│  └─────────────────────────────────────┘ │
│                                          │
│  ┌─────────────────────────────────────┐ │
│  │  Models (Tauri resources on disk)   │ │
│  │  arctic-embed-xs (86 MB)            │ │
│  │  MiniLM-L6-v2 reranker (87 MB)      │ │
│  │  BM25 tokenizer (~0.1 MB)           │ │
│  │  Loaded at startup via memmap       │ │
│  └─────────────────────────────────────┘ │
└──────────────────────────────────────────┘
                     │ localhost (REST/gRPC)
┌───────────────────▼─────────────────────┐
│  Qdrant (sidecar child process)          │
│  Spawned via tauri-plugin-shell          │
│  Configured via QDRANT__* env vars       │
│  Storage: ~/.local/share/mcp-nano/qdrant  │
└─────────────────────────────────────────┘
```

---

## Current State

Done so far:

- **Repo scaffolded** via `create-tauri-app` (react-ts template). VectorFlowUI
  ported in: `src/`, Tailwind/Vite/ESLint/tsconfig configs, merged `package.json`.
  `tauri.conf.json` has `identifier: com.mcpquick.mcp-nano` and
  `devUrl: http://localhost:18674` (port intentionally separate from VectorFlow).
- **Frontend API layer extracted.** Every backend call in the UI now lives in
  one file: `src/utils/apicalls.ts` — 27 typed functions calling Tauri
  `invoke()` directly. The old fetch wrapper (`src/utils/api.ts`) and
  `src/types/api.ts` were deleted; no `/api/` URLs remain in `src/`.
- **Rust controllers created.** `src-tauri/src/controllers/` mirrors the
  original FastAPI routers one module per router (rag, jobs, data, mcpconfig,
  website) — 27 `#[tauri::command]` functions registered in `lib.rs`. Each
  command currently logs its args with `println!` and returns a default-shaped
  response, so the UI works end-to-end over real IPC.
- **Rust models structured.** All types extracted from controllers into
  `src-tauri/src/models/` (see layout below).
- Dev environment installed (Rust toolchain, webkit2gtk-4.1, Tauri system deps).

```
src-tauri/src/
├── main.rs          (entry, calls run())
├── lib.rs           (tauri::Builder, invoke_handler — 27 commands registered)
├── controllers/     (#[tauri::command] fns — thin wrappers over core logic)
│   ├── rag.rs       (rag_query, get_metadata_values)
│   ├── jobs.rs      (upload_repo_zip/documents/code_files, get_active_jobs, get_job_status)
│   ├── data.rs      (get_files, get_websites, 7 delete commands)
│   ├── mcpconfig.rs (server/tool CRUD, connection-info)
│   └── website.rs   (crawl_website, embed_website)
├── models/
│   ├── mod.rs       (+ business models: RagResult, RepoItem, WebsiteItem)
│   ├── request/     (incoming UI payloads: RagQueryRequest, EmbeddingOptions, ToolPayload/ScopePayload)
│   ├── response/    (outgoing envelopes: *Response, ConnectionInfo, UploadJobEntry)
│   └── entities/    (future sqlite rows: JobStatus, FileMetadata, McpServer,
│                     ToolDefinition, ToolCodeSearchScope, ToolDocumentSearchScope)
├── embed/           (ONNX models, BM25, chunking)        — Phase 2
├── worker/          (job poll loop, tasks)               — Phase 3
├── qdrant/          (client, sidecar lifecycle)          — Phase 4
├── mcp/             (axum + rmcp server)                 — Phase 6
└── db/              (sqlx, migrations)                   — Phase 3
```

Entities are reused inside responses (`UserFilesResponse.documents:
Vec<FileMetadata>`, `ActiveJobsResponse.jobs: Vec<JobStatus>`,
`ServersResponse.servers: Vec<McpServer>`) so the sqlite layer flows straight
into responses when implemented.

**Verified:** `npm run build`, `npm run lint`, `cargo check` all pass.

### Deferred / known gaps

- **No browser-dev mocks yet.** `apicalls.ts` calls `invoke()` directly, which
  only exists inside the Tauri webview — `npm run dev` in a plain browser has
  no backend. A mock shim (`invokeCommand` + mock handlers) can be added later
  if browser dev is needed.
- **Frontend tests not updated.** `test/setup.ts` still mocks the deleted
  `@/utils/api`; `npm run test` fails until the suite is re-pointed at
  `@/utils/apicalls`.
- **Uploads are placeholder paths.** `uploadRepoZip/Documents/CodeFiles` send
  `File.name` strings as `paths`; real filesystem paths arrive with the native
  drag-drop swap (see "Uploads" below).
- Not yet created: `scripts/download-models.sh`,
  `scripts/download-qdrant.sh`, `src-tauri/resources/`, `src-tauri/binaries/`,
  `bundle.resources` / `bundle.externalBin` in `tauri.conf.json`.

---

## Dev Environment Setup (Linux)

Already done on the dev machine. Kept for reference / other machines.

```bash
# 1. Rust toolchain (no sudo)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env
rustup component add clippy rustfmt rust-analyzer

# 2. Tauri 2 system deps + build tools (sudo, Ubuntu 24.04)
sudo apt update
sudo apt install -y \
  build-essential curl wget file patchelf pkg-config libxdo-dev \
  libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev
```

| Package | Needed for |
|---------|-----------|
| `libwebkit2gtk-4.1-dev` | Tauri's webview on Linux (runtime + build) |
| `patchelf` | AppImage bundling |
| `libxdo-dev` | Tauri 2 prerequisite (global shortcut / window features) |
| `librsvg2-dev` | Icon rendering in the bundler |

The Tauri CLI is the npm package `@tauri-apps/cli` (pinned in `package.json`):

```bash
npm run tauri dev     # dev mode: vite + hot reload + native window
npm run tauri build   # release build + bundler (.deb/.AppImage/.msi)
```

**GPU note:** machine has an RTX 3090 + CUDA toolkit. ONNX Runtime (`ort`) has a
`cuda` feature that enables the CUDA execution provider. Default builds are
CPU-only (22M-param model is fast on CPU); GPU builds use
`npm run tauri:dev:gpu` / `tauri:build:gpu` and fall back to CPU at runtime.

---

## Component Sizes

**Main executable:**

| Component | Size |
|-----------|------|
| Rust app code + deps (ort, axum, tokio, sqlx, reqwest, scraper) | ~25 MB |
| Tauri shell + IPC | ~5 MB |
| Frontend (React/Vite/Tailwind compiled) | ~3 MB |
| Tree-sitter grammars (10 languages) | ~5 MB |
| **Exe subtotal** | **~40 MB** |

**Bundled resources (installed next to exe, read at runtime):**

| Component | Size |
|-----------|------|
| `arctic-embed-xs/model.onnx` + `tokenizer.json` | ~87 MB |
| `minilm-l6-v2/model.onnx` + `tokenizer.json` | ~87 MB |
| BM25 stopwords (in-crate) | ~0.1 MB |
| **Resources subtotal** | **~174 MB** |

**Sidecar (bundled alongside exe):**

| Component | Size |
|-----------|------|
| Qdrant binary (uncompressed) | ~83 MB |

**Final Package:**

| Metric | Value |
|--------|-------|
| Uncompressed (installed on disk) | ~295 MB |
| Compressed download (.deb / .msi / .AppImage) | ~180–200 MB |
| Runtime memory (models mmap'd + Qdrant) | ~400 MB |

Models are loaded as ONNX graphs via ONNX Runtime (`ort`); weights stay on disk
and are mapped by the runtime as needed.

---

## Models as Tauri Resources

Models ship as **resource files inside the installer**, not compiled into the
binary. (Original plan used `include_bytes!` — rejected: multi-minute link
times, multi-GB rustc memory, and full rebuilds to swap a model.)

The user never sees a bare exe or loose model files — Tauri's bundler packs
`resources/models/` into the installer, and the installer lays it down:

| Package | Models installed to |
|---------|---------------------|
| `.deb` | `/usr/lib/mcp-nano/models/` |
| `.AppImage` | Inside the image, mounted transparently at runtime |
| `.msi` (Windows) | `C:\Program Files\mcp-nano\models\` |

### Repo layout (to be created in Phase 2)

```
src-tauri/
├── resources/
│   └── models/                     (git-ignored)
│       ├── arctic-embed-xs/
│       │   ├── model.onnx
│       │   ├── tokenizer.json
│       │   └── config.json
│       └── minilm-l6-v2/
│           ├── model.onnx
│           ├── tokenizer.json
│           └── config.json
└── scripts/download-models.sh      (one-time fetch from HF)
```

### Runtime resolution

```rust
use tauri::Manager;

fn models_dir(app: &tauri::AppHandle) -> PathBuf {
    if cfg!(debug_assertions) {
        // dev: read straight from the repo, bundler not involved
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/models")
    } else {
        app.path().resource_dir().unwrap().join("models")
    }
}
```

In release builds `resource_dir()` resolves the per-OS install location above.
No hardcoded paths anywhere.

---

## Model Decisions

| Role | Model | Architecture | Why |
|------|-------|-------------|-----|
| Dense embedding | `Snowflake/snowflake-arctic-embed-xs` | BERT ONNX (22M params), 512-token ctx | Tops MTEB for sub-50M models. Official HF `onnx/model.onnx` |
| Reranker | `cross-encoder/ms-marco-MiniLM-L6-v2` | BERT SeqClass ONNX, 512-token ctx | 85M downloads, battle-tested. Official HF `onnx/model.onnx` |
| Sparse (BM25) | Hand-rolled BM25 | Alphanumeric split + stopwords | ~100 lines of Rust; see note below |
| Loading | Tauri resources + ONNX Runtime (`ort`) | Disk | Zero first-run download; CUDA EP optional |

**BM25 note:** original plan used the `fastembed` crate — rejected for bundling
its own models and download path. BM25 is hand-rolled (simple split + English
stopwords + TF saturation; IDF left to Qdrant). Dense/rerank inference uses
`ort` directly with our bundled ONNX exports.

**Context window note:** Both models have a 512-token limit. Chunks from the
`text-splitter` crate (default 768 tokens) that exceed this will be split into
overlapping windows, embedded separately, and mean-pooled.

---

## Frontend ↔ Backend Contract

All UI→Rust calls go through `src/utils/apicalls.ts`. One typed async function
per operation, each a one-liner over `invoke()` from `@tauri-apps/api/core`.
The old HTTP fetch layer is gone.

Conventions:

- Command names are snake_case and match the Rust function names.
- JS passes camelCase argument keys (`{ serverId, toolData }`); Tauri converts
  them to the snake_case Rust parameters automatically.
- Response structs serialize snake_case JSON identical to the old FastAPI
  shapes, matching the existing TS interfaces (`RagResponse`, `JobStatus`,
  `McpServer`, …). Rust request/response/entity types live in
  `src-tauri/src/models/`.
- Rust commands return `Result<T, String>`; a rejected promise carries the
  error string (no HTTP status codes — UI handles errors as messages).

### Implemented command map (27 commands)

Source of truth: `src/utils/apicalls.ts` ↔ `src-tauri/src/controllers/`.
Original FastAPI paths shown for reference only — no HTTP server serves these.

#### RAG (`controllers/rag.rs`)

| Original (FastAPI) | Tauri Command | Args |
|--------------------|---------------|------|
| `POST /api/rag/query` | `rag_query` | `payload: RagQueryRequest` |
| `GET /api/rag/metadata/values` | `get_metadata_values` | `collection_name, key` |

#### Jobs (`controllers/jobs.rs`)

| Original | Tauri Command | Args |
|----------|---------------|------|
| `POST /api/jobs/upload-repo-zip` | `upload_repo_zip` | `paths: string[], embedding_options` |
| `POST /api/jobs/upload-documents` | `upload_documents` | `paths: string[], embedding_options` |
| `POST /api/jobs/upload-code-files` | `upload_code_files` | `paths: string[], embedding_options` |
| `GET /api/jobs/active` | `get_active_jobs` | none |
| `GET /api/jobs/status/{job_id}` | `get_job_status` | `job_id` |

#### Data (`controllers/data.rs`)

| Original | Tauri Command | Args |
|----------|---------------|------|
| `GET /api/data/files` | `get_files` | none |
| `DELETE /api/data/repo/{repo_name}` | `delete_repo` | `repo_name` |
| `DELETE /api/data/document/{filename}` | `delete_document` | `filename` |
| `DELETE /api/data/group/{group_name}` | `delete_group` | `group_name` |
| `DELETE /api/data/clear-user-collection` | `clear_user_collection` | `collection_name` |
| `GET /api/data/websites` | `get_websites` | none |
| `DELETE /api/data/website` | `delete_website` | `url` |
| `DELETE /api/data/website-group/{group_name}` | `delete_website_group` | `group_name` |
| `DELETE /api/data/websites/clear` | `clear_websites` | none |

#### MCP Config (`controllers/mcpconfig.rs`)

| Original | Tauri Command | Args |
|----------|---------------|------|
| `GET /api/mcp/config/servers` | `get_mcp_servers` | none |
| `POST /api/mcp/config/servers` | `create_mcp_server` | `name, description` |
| `GET /api/mcp/config/servers/{id}` | `get_mcp_server` | `server_id` |
| `DELETE /api/mcp/config/servers/{id}` | `delete_mcp_server` | `server_id` |
| `POST /api/mcp/config/servers/{id}/tools` | `create_mcp_tool` | `server_id, tool_data` |
| `PUT /api/mcp/config/servers/{id}/tools/{tid}` | `update_mcp_tool` | `server_id, tool_id, tool_data` |
| `DELETE /api/mcp/config/servers/{id}/tools/{tid}` | `delete_mcp_tool` | `server_id, tool_id` |
| `PATCH /api/mcp/config/servers/{id}/tools/{tid}/active` | `toggle_mcp_tool` | `server_id, tool_id, active` |
| `GET /api/mcp/config/servers/{id}/connection-info` | `get_mcp_connection_info` | `server_id` |

#### Website (`controllers/website.rs`)

| Original | Tauri Command | Args |
|----------|---------------|------|
| `POST /api/website/crawl` | `crawl_website` | `url, depth, same_domain_only` |
| `POST /api/website/embed` | `embed_website` | `urls, group` |

### Deliberately not carried over from the FastAPI surface

Scope is what the UI actually calls. Excluded (add later if needed):

- `billing/*` — SaaS-only, no billing in the desktop app
- Legacy `{user_id}` path-param aliases (`/api/jobs/active/{user_id}`,
  `/api/data/files/{user_id}`, `/api/data/websites/{user_id}`) — server ignored
  them anyway
- Public ping routes (`/api/rag/ping`, `/api/vector/ping`, `/api/mcp/config/ping`)
- LLM-dependent endpoints (`/api/rag/query_with_answer` SSE, `quick_answer`,
  `tool-description-rewrite`)
- Unused by the UI today: health/app-info, `rag/collections`, `metadata/keys`,
  embedders status/preload, jobs all/retry/start/logs/delete-pending/deleteall/
  worker-status, `vector/*`, MCP server update/toggle, list-tools, `jobs/help`,
  `process-pending`

### Uploads — Drag & Drop in Tauri 2 (pending)

**Important Tauri 2 behavior:** with `dragDropEnabled` (default `true`) on the
webview window, HTML5 drag-drop events are intercepted by Tauri and **never
reach the DOM** — `react-dropzone` stops working inside the native window.
The replacement is Tauri's native event:

```typescript
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

await getCurrentWebviewWindow().onDragDropEvent((event) => {
  if (event.payload.type === 'drop') {
    // payload.paths: string[] — real filesystem paths
    uploadCodeFiles(event.payload.paths, embeddingOptions)
  }
})
```

Rust then reads files directly from disk by path — no bytes over IPC (serializing
a 100 MB zip through `invoke` is slow). A Browse button fallback uses
`@tauri-apps/plugin-dialog` (`open({ multiple: true })` → paths). The
`upload_*` commands and apicalls functions already take `paths: string[]`;
the swap is confined to the dropzone hook and those functions.

---

## MCP Endpoint (Axum — one route only)

External AI tools (Claude, Cursor, opencode) connect to the MCP endpoint over
HTTP. This is the **only** thing Axum does. It binds `127.0.0.1` only.

Tauri 2's async runtime is tokio-backed, so `tauri::async_runtime::spawn` gives
us a runtime context where tokio/axum can run:

```rust
// Illustrative — verify exact API names against the current rmcp release.
// Spawned from setup():

tauri::async_runtime::spawn(async move {
    // rmcp streamable-HTTP service; factory closure builds a handler per session.
    // An axum middleware extracts ?server_id= from the query string into
    // request extensions; the rmcp handler reads it from there.
    let service = StreamableHttpService::new(
        move || Ok(McpHandler::new(state.clone())),
        LocalSessionManager::default().into(),
        Default::default(),
    );
    let app = Router::new()
        .nest_service("/mcp", service)
        .layer(from_fn(extract_server_id));
    let listener = TcpListener::bind(("127.0.0.1", MCP_PORT)).await.unwrap();
    axum::serve(listener, app).await.unwrap();
});
```

Tools are dynamically listed per `server_id` from SQLite, same as the current
Python `dynamic_list_tools`/`dynamic_call_tool` pattern. Tool execution calls
the same RAG pipeline used internally by Tauri commands.

Default port: `18651` (same as today), with fallback to an ephemeral port if
taken; the effective port is shown in the connection-info UI.

User AI tool config (identical to current):

```json
{
  "mcpServers": {
    "my-code-search": {
      "url": "http://localhost:18651/mcp?server_id=abc123"
    }
  }
}
```

---

## SQLite + Background Worker

### Schema (identical to current)

6 tables via `sqlx` migrations at startup. The matching Rust structs already
exist in `src-tauri/src/models/entities/`:

```
job_status           — background job tracking (PENDING → RUNNING → FINISHED/FAILED)  → JobStatus
file_metadata        — uploaded file registry                                          → FileMetadata
mcp_servers          — MCP server definitions                                          → McpServer
tool_definitions     — MCP tool definitions                                            → ToolDefinition
tool_code_search     — code collection scopes per tool                                 → ToolCodeSearchScope
tool_document_search — document collection scopes per tool                             → ToolDocumentSearchScope
```

### Worker

Same DB-driven poll loop, translated to async Rust. **SQLite has no
`SELECT ... FOR UPDATE SKIP LOCKED`** (that's Postgres) — job claiming uses an
immediate transaction with `RETURNING` (SQLite 3.35+, bundled by sqlx):

```sql
BEGIN IMMEDIATE;
UPDATE job_status
SET status = 'RUNNING', started_at = CURRENT_TIMESTAMP
WHERE id IN (
  SELECT id FROM job_status WHERE status = 'PENDING' ORDER BY created_at LIMIT 2
)
RETURNING *;
COMMIT;
```

Loop:

```
Every 2 seconds:
  1. Reclaim stale RUNNING jobs (>15 min no progress → FAILED)
  2. Claim PENDING jobs via the transaction above
  3. Spawn each job on tauri::async_runtime (max 2 concurrent via Semaphore(2))
  4. Task calls progress_callback closure → updates job_status row
```

A single write connection (dedicated sqlx connection for the worker) makes the
claim race-free in this single-process app.

Shutdown: `CancellationToken` (tokio-util) propagates, worker stops polling,
resets RUNNING → PENDING.

### Data Directory

All persistent data lives in the OS-standard app data dir:

```
~/.local/share/mcp-nano/                 (Linux)
C:\Users\<user>\AppData\Local\mcp-nano\  (Windows)
├── app.db           SQLite database
├── qdrant/          Qdrant storage
└── uploads/         Temporary file uploads
```

Resolved at startup via the `dirs` crate. Created if missing.

---

## Qdrant Sidecar Management

The `qdrant` binary is bundled via Tauri 2's sidecar feature (`bundle.externalBin`).
**The binary filename must carry the Rust target-triple suffix** — this is how
Tauri picks the right per-platform binary:

```
src-tauri/binaries/
├── qdrant-x86_64-unknown-linux-gnu      (git-ignored, from download-qdrant.sh)
└── qdrant-x86_64-pc-windows-msvc.exe
```

### Startup lifecycle

**The qdrant binary does not accept `--uri` / `--storage-path` CLI flags.**
It is configured via environment variables (double-underscore nesting):

```rust
use tauri_plugin_shell::ShellExt;

async fn spawn_qdrant(app: &tauri::AppHandle, data_dir: &Path) -> Result<QdrantHandle> {
    let storage = data_dir.join("qdrant");
    std::fs::create_dir_all(&storage)?;

    let http_port = find_free_port(18633);
    let grpc_port = find_free_port(18634);

    let (mut rx, child) = app.shell()
        .sidecar("qdrant")?
        .env("QDRANT__SERVICE__HTTP_PORT", http_port.to_string())
        .env("QDRANT__SERVICE__GRPC_PORT", grpc_port.to_string())
        .env("QDRANT__STORAGE__STORAGE_PATH", storage.to_string_lossy())
        .spawn()?;

    wait_for_healthz(http_port).await;  // poll GET http://127.0.0.1:{port}/healthz
    Ok(QdrantHandle { child, http_port })
}
```

Sidecars spawned through the shell plugin are killed automatically when the app
exits. For graceful shutdown (let Qdrant flush), hook `RunEvent::ExitRequested`
and send SIGTERM first.

The app talks to Qdrant via the `qdrant-client` crate over gRPC. The pinned
`qdrant-client` 1.18 release ships generated bindings, so no system `protoc`
installation is required. A REST-only client via `reqwest` remains a fallback.

---

## Implementation Phases

### Phase 0: Machine Setup + Repo Scaffold — ✅ done

- Dev environment installed (rustup + apt deps)
- `mcp-nano` scaffolded via create-tauri-app; VectorFlowUI ported in
- `identifier` + `devUrl` set in `tauri.conf.json`
- Still to do when needed: `scripts/download-models.sh` (Phase 2),
  `scripts/download-qdrant.sh` + `externalBin`/`resources` bundle config (Phase 8)

### Phase 1: Frontend Port + Command Surface — ✅ done

- VectorFlowUI ported into the repo (src, configs, package.json merge)
- All API calls extracted into `src/utils/apicalls.ts` (27 typed functions over `invoke()`)
- 27 `#[tauri::command]` stubs in `src-tauri/src/controllers/`, registered in `lib.rs`
- Types structured into `src-tauri/src/models/{request,response,entities}`
- Polish deferred: browser-dev mock shim, frontend test suite update,
  native drag-drop upload swap (see "Uploads" above)

### Phase 2: Rust Core — Models + Embedding

- `scripts/download-models.sh`; models into `src-tauri/resources/models/`
- Wire up ONNX Runtime (`ort`): load `model.onnx`, tokenizer integration, mean pooling
- Implement `EncodeQuery` trait (embed a query string into a vector)
- Implement `EncodeDocuments` trait (embed batch documents)
- Implement reranker as cross-encoder
- Implement hand-rolled BM25 sparse embedding
- **Goal:** `cargo test` passes: `encode("hello world")` returns expected-dimension vector

### Phase 3: SQLite + Worker

- Set up `sqlx` with migration scripts (CREATE TABLE IF NOT EXISTS) —
  table shapes already defined as structs in `models/entities/`
- Implement worker poll loop with `BEGIN IMMEDIATE` + `RETURNING` claiming
- Implement task registry + task execution
- Wire progress callback to `job_status` updates
- **Goal:** `cargo test`: insert PENDING job → worker claims → task runs → status updates

### Phase 4: Qdrant Integration

- Implement `QdrantService` via `qdrant-client` crate
- Sidecar spawn/health/teardown with `QDRANT__*` env config
- Collection CRUD, upsert with hybrid vectors (dense + sparse), query with RRF fusion
- Payload index management
- **Goal:** Integration test: spawn sidecar → embed text → upsert → query → verify results

### Phase 5: Ingestion Pipeline

- Port tree-sitter code chunkers (10 languages) — native Rust crates
- Port document loaders (PDF, docx, xlsx, HTML, etc.)
- Port `text-splitter` chunking pipeline
- Port website crawler (`reqwest` + `scraper`)
- Wire the `upload_*` commands to real file reads from `paths`
- **Goal:** Upload a zip → unzip → chunk → embed → searchable in Qdrant

### Phase 6: MCP Endpoint

- Set up Axum with single `/mcp` route on 127.0.0.1:18651
- Mount `rmcp` streamable-HTTP service; `server_id` query param via middleware
- Dynamic tool listing from SQLite per `server_id`
- Tool call → RAG query → formatted response
- **Goal:** Claude Desktop connects to `localhost:18651/mcp?server_id=xxx` and queries work

### Phase 7: Tauri Integration

- Implement the 27 controller bodies over the core modules (registration already done)
- Qdrant sidecar lifecycle in `setup()`
- Axum MCP server spawn in `setup()`
- Worker poll loop spawn in `setup()`
- Frontend test suite re-pointed at `@/utils/apicalls`; drag-drop upload swap
- **Goal:** Full app runs. UI → invoke → Rust → Qdrant → results in UI

### Phase 8: Packaging

- Tauri bundler: `.deb` + `.AppImage` (Linux), `.msi` (Windows)
- `bundle.resources` for models, `bundle.externalBin` for Qdrant in `tauri.conf.json`
- Qdrant per-platform binaries via `externalBin` target-triple naming
- **Windows builds happen on Windows (or GitHub Actions)** — cross-compiling
  a Tauri app from Linux to Windows is not supported
- Evaluate `ort` CUDA execution provider for batch embedding acceleration
- OS code signing if desired
- **Goal:** User downloads one file, double-clicks, it works

---

## Key Crates

| Crate | Purpose |
|-------|---------|
| `tauri` | Desktop app shell, IPC |
| `tauri-plugin-shell` | Sidecar spawn (Qdrant) |
| `tauri-plugin-dialog` + `@tauri-apps/plugin-dialog` (npm) | File picker fallback for uploads |
| `tauri-plugin-updater` | Signed application updates |
| `tokio` | Async primitives (Tauri 2's async runtime is tokio-backed) |
| `tokio-util` | `CancellationToken` for worker shutdown |
| `axum` + `tower-http` | HTTP server and CORS/middleware — **only** for MCP endpoint |
| `rmcp` | MCP protocol implementation |
| `sqlx` | Async SQLite (compile-time checked queries) |
| `qdrant-client` | Vector DB client (gRPC; generated bindings ship in the pinned release) |
| `ort` | ONNX Runtime: dense embedding + cross-encoder rerank (optional CUDA EP) |
| `tokenizers` (HF) | Tokenizer loading + encoding (dense/rerank models) |
| `serde` + `serde_json` | Serialization (replaces Pydantic) |
| `tree-sitter` + lang grammars | Code chunking (10 languages) |
| `text-splitter` | Token-aware text chunking |
| `reqwest` | HTTP client (website crawler; health checks) |
| `scraper` | HTML parsing (crawler) |
| `dirs` | Platform standard directory paths |
| `uuid` | UUID generation |
| `sha2` | SHA256 hashing |
| `zip` + `walkdir` + `globset` | Archive extraction and recursive ingestion with ignore rules |
| `pdf-extract` | PDF text extraction |
| `docx-lite` | DOCX text extraction |
| `calamine` | XLS, XLSX, XLSB, and ODS spreadsheet reading |
| `libchm` | CHM document extraction |
| `quick-xml` + `csv` | ODT/XML and CSV document loading primitives |
| `url` | URL resolution and normalization for crawling |
| `dotenvy` | Development environment loading |
| `anyhow` + `thiserror` | Application and typed error handling |
| `tracing` + `tracing-subscriber` | Structured application logging |
| `tempfile` (dev) | Isolated filesystem fixtures for Rust tests |

Removed from the original plan: `fastembed` (drags in ONNX Runtime; BM25 is
hand-rolled with `tokenizers` instead).
