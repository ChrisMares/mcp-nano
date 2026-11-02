# mcp-nano

Native desktop mcp server and data ingestion. Built with Tauri 2, Rust, React, Vite, and Tailwind CSS.

The full backend is implemented in Rust: ONNX Runtime ML models (dense embedding, BM25 sparse, cross-encoder reranker), tree-sitter code chunkers (11 languages), document loaders (PDF, DOCX, XLSX, HTML, CSV, XML, ODT, Markdown, plain text), SQLite persistence, Qdrant vector DB sidecar management, hybrid RAG query pipeline, MCP streamable-HTTP server on port 18651, background job worker, and website crawling/embedding — all running locally with zero Docker or Python dependencies.

## Prerequisites

- Node.js 22.13+ (22.x) or 24+
- npm 10 or later
- Rust stable via [rustup](https://rustup.rs/)
- Linux only: Tauri system dependencies

On Ubuntu 24.04, install the Linux dependencies:

```bash
sudo apt update
sudo apt install -y \
  build-essential curl wget file patchelf pkg-config libxdo-dev \
  libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev
```

## Install

```bash
npm install
```

No configuration file is needed; tuning values (chunk sizes, crawl limits,
concurrency) are constants in the Rust source. Three optional environment
variables are honored at runtime:

| Variable | Purpose |
| --- | --- |
| `MCP_NANO_DEVICE=cpu` | Force embedding inference onto the CPU. |
| `WEBSITE_BROWSER_PATH` | Chromium-family browser used for JavaScript rendering (auto-detected otherwise). |
| `RUST_LOG` | Log filter, e.g. `info,mcp_nano=debug`. |

## Run The Frontend

Starts Vite at `http://localhost:18674`

```bash
npm run dev
```

## Run The Desktop App

Starts Vite and opens the application in a native Tauri window:

```bash
npm run tauri dev
```

GPU Mode:

```bash
npm run tauri:dev:gpu
```

## Build

### Prerequisites (one-time)

Ensure model files and the Qdrant binary are downloaded:

```bash
bash src-tauri/scripts/download-models.sh
bash src-tauri/scripts/download-qdrant.sh
```

### Linux (Ubuntu/Debian)

```bash
# Install system dependencies
sudo apt update
sudo apt install -y build-essential curl wget file patchelf pkg-config \
  libxdo-dev libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev

# Build the .deb installer
npm install
npm run tauri build

# Output: src-tauri/target/release/bundle/deb/mcp-nano_*.deb
```

For an `.AppImage` instead of `.deb`, change `"targets"` in `tauri.conf.json` under `bundle` to `["appimage"]`.

### Windows

```powershell
# 1. Install Rust via rustup.msi (https://rustup.rs)
# 2. Install Node.js 22.13+ (22.x) or 24+ (https://nodejs.org)
# 3. Install Visual Studio Build Tools or VS 2022 with "Desktop development with C++"
# 4. Install Git for Windows so bash is available on PATH
# 5. WebView2 is included with current Windows / Microsoft Edge

# Build both MSI and NSIS installers. This downloads models, Qdrant,
# installs locked npm dependencies, and enables DirectML with CPU fallback.
.\build-windows.ps1

# Outputs:
# src-tauri/target/release/bundle/msi/mcp-nano_*.msi
# src-tauri/target/release/bundle/nsis/mcp-nano_*-setup.exe
```

The Windows package is a single adaptive build. At startup it attempts
DirectML GPU inference, verifies that the models can run, and reloads the CPU
execution provider if DirectML or the GPU driver is unavailable. Set
`MCP_NANO_DEVICE=cpu` to force CPU inference. Use
`.\build-windows.ps1 -SkipInstall` when dependencies are already installed.

### Frontend-only build

Build the production frontend bundle (no native app):

```bash
npm run build
```

Output is written to `dist/`. Tauri build artifacts are under `src-tauri/target/release/`.

## MCP Search Tools

Every tool you create in the app is exposed over MCP with the same arguments:

| Argument | Purpose |
| --- | --- |
| `query` | Natural-language search. Required unless `symbol` is given. |
| `limit` | Number of results, capped by the tool's configured maximum. |
| `symbol` | Function, method, or type name to look up in indexed code, e.g. `run_rag_query` or `RagService::run_rag_query`. |
| `relation` | With `symbol`: `definition` (default), `references` (code that calls it), `callees` (definitions of what it calls), or `members` (a class or interface and the methods defined in it). |
| `expand` | Text searches only, default `false`: also return the definitions of functions the results call. |

Code results include `File:`, `Symbol:` and `Calls:` lines so an agent can
follow references. A qualified symbol (`Type::name` or `Type.name`) matches
that exact member first, then the bare name in a file or folder with that
name (`public::router` finds `router` in `src/routes/public.rs`, `main::main`
the one in `src/main.rs`), and only then every definition of the bare name.
When a name is defined in several files the response includes a `Note:`
suggesting how to qualify it. In code searches, release notes, license files,
and data dumps (data files over 300 lines or with very long lines) rank below
code and docs; Markdown, READMEs, and small configs are not affected. Qt
translation files (`.ts` XML) are skipped at upload. Callees prefer definitions in the caller's own file. JSX elements
(`<Foo />`) count as calls, so component references work. Lookups match by
name, not by type, so calls through interfaces can match several
implementations. Repositories uploaded before these features must be
re-uploaded to get symbol, call, path, and JSX component data.

## Project Layout

```text
src/                 React application source
src-tauri/           Rust and Tauri application
vite.config.ts       Root Vite configuration for the ported frontend
```

## Useful Commands

```bash
npm run lint         # Run ESLint against the frontend
npm run test         # Run frontend tests
npm run preview      # Serve the production frontend bundle locally
```
