# mcp-nano Agent Instructions

`mcp-nano` is the native desktop Rust rewrite of VectorFlow. It is a Tauri 2
application with a React/TypeScript frontend and a Rust backend. The goal is a
local-only application with no Docker, Python, or runtime network dependency
after installation.

## Sources Of Truth

- Implementation architecture and migration decisions:
  `documentation/rust_rewrite.md`.
- Behavioral reference for features being ported:
  `/home/chris/git/VectorFlowDocker`.
- Do not copy the original Python/FastAPI deployment architecture into this
  repository. Translate its observable behavior into the native architecture
  described in the rewrite plan.
- When the two sources differ, follow `documentation/rust_rewrite.md` for
  architecture and use VectorFlow to determine feature semantics, validation,
  response shapes, data relationships, and edge cases.

## Project Layout

| Path | Purpose |
| --- | --- |
| `src/` | Ported React 19, TypeScript, Vite, and Tailwind frontend. |
| `src/utils/apicalls.ts` | The sole UI-to-backend boundary: typed Tauri `invoke()` calls. |
| `src/types/` | TypeScript request and response contracts used by the UI. |
| `src-tauri/src/controllers/` | Thin `#[tauri::command]` wrappers over Rust core services. |
| `src-tauri/src/models/` | Serde request, response, business, and future SQLite entity types. |
| `src-tauri/src/embed/` | Planned embedding, reranking, BM25, and chunking implementation. |
| `src-tauri/src/db/` | SQLite pool setup and access; migrations in `src-tauri/migrations/`. |
| `src-tauri/src/worker/` | Planned background job worker and task execution. |
| `src-tauri/src/qdrant.rs` | Qdrant sidecar lifecycle, client connect/retry, and startup collection/index init. |
| `src-tauri/src/mcp/` | Planned localhost Axum and rmcp endpoint. |

## Current Status

- The frontend and its 27-command IPC surface are ported.
- Qdrant startup is wired: `setup()` spawns the sidecar, then a background task
  connects with retry, ensures the `codebase`/`general` collections (dense 384
  Cosine + sparse BM25 vectors) and payload indexes exist, and registers
  `QdrantState` in Tauri managed state.
- SQLite is wired: `setup()` spawns `db::init`, which opens
  `app_local_data_dir()/app.db` (foreign keys on), runs the sqlx migrations in
  `src-tauri/migrations/`, and registers `DbState` in Tauri managed state. The
  schema mirrors the VectorFlow tables (`file_metadata`, `job_status`,
  `mcp_servers`, `tool_definitions`, `tool_code_search`,
  `tool_document_search`) with all `user_id` columns and indexes dropped for
  single-user local mode.
- Rust controllers currently return placeholder responses. Models and required
  Cargo dependencies are scaffolded, but core backend behavior is not yet
  implemented.
- Implement the rewrite in the phases documented in `rust_rewrite.md`; avoid
  building later layers before their dependencies are in place.
- Known frontend gaps are documented in the rewrite plan: browser-only dev has
  no `invoke()` mock, tests still mock the removed HTTP API module, and upload
  controls still pass file names rather than native paths.

## Architecture Rules

- The React application communicates with Rust only through
  `src/utils/apicalls.ts` and Tauri `invoke()`. Do not add a local REST API for
  UI operations or reintroduce `/api/` fetch calls.
- Keep controller commands thin. Put reusable business logic in the relevant
  core module and have Tauri commands call it.
- Preserve the established IPC contract: command names are snake_case; JS
  invokes them with camelCase arguments; Rust responses serialize snake_case
  fields matching the existing TypeScript interfaces.
- Tauri commands should return `Result<T, String>` so UI failures reject with a
  useful message.
- Use SQLite for metadata, jobs, MCP configuration, and file registry data.
  Use Qdrant only for vectors and vector payloads.
- Persist application data only in the OS-standard app data directory, never in
  the repository or temporary working directory.
- Qdrant is a bundled Tauri sidecar configured with `QDRANT__*` environment
  variables. Do not depend on an externally installed Qdrant server.
- The only HTTP listener is the localhost-bound MCP endpoint at `/mcp`.
  It must bind to `127.0.0.1`, not a public interface.
- MCP tools are dynamic per `server_id`; expose only active tools belonging to
  an active server, and execute them through the shared RAG pipeline.

## Behavior Parity

- Treat the original app as a behavioral specification, especially its
  `vector-flow/src/webapi/routers/`, `src/query/`, `src/embedding/`,
  `src/data/`, and `src/mcp/protocol.py` modules.
- Preserve user-visible request validation, default values, job states,
  metadata/filter semantics, MCP tool scope behavior, and response envelopes
  unless the rewrite plan explicitly changes them.
- The desktop app is single-user local mode. Do not add remote authentication,
  billing, legacy user-ID route aliases, or unused FastAPI endpoints.
- Do not transfer Python implementation details directly. Use idiomatic Rust,
  `serde`, `sqlx`, `tokio`, and the crates already selected in `Cargo.toml`.

## Implementation Notes

- Load bundled model resources from disk and memory-map safetensors; do not use
  `include_bytes!` or compile model data into the executable.
- Keep embedding and reranking CPU-first until profiling justifies the optional
  CUDA feature.
- The worker claims SQLite jobs with `BEGIN IMMEDIATE` and `RETURNING`, limits
  concurrency to two jobs, and resets interrupted running jobs to pending on
  shutdown as specified in the rewrite plan.
- For Tauri uploads, use native drag-drop paths or the dialog plugin. Do not
  serialize file contents over IPC.
- Keep external resource directories and platform-specific Qdrant binaries out
  of git as specified by the plan.

## Development And Verification

```bash
npm run lint
npm run build
npm run test
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri dev
```

- Run the focused checks affected by a change. Run both frontend and Rust checks
  for cross-boundary contract changes.
- Frontend tests live in `src/test/` (vitest). `tsconfig.json` excludes
  `src/test` from `tsc -b` type-checking, matching the pre-move behavior;
  several test files have latent type errors if that exclusion is ever
  removed.
- Do not modify the original VectorFlow repository while using it as reference.
- Keep changes small, idiomatic, and focused. Avoid speculative compatibility
  layers, unnecessary abstractions, and comments that merely restate code.
