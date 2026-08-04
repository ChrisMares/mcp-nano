# mcp-nano

Native desktop rewrite of VectorFlow, built with Tauri 2, Rust, React, Vite, and Tailwind CSS.

The full backend is implemented in Rust: ONNX Runtime ML models (dense embedding, BM25 sparse, cross-encoder reranker), tree-sitter code chunkers (11 languages), document loaders (PDF, DOCX, XLSX, HTML, CSV, XML, ODT, Markdown, plain text), SQLite persistence, Qdrant vector DB sidecar management, hybrid RAG query pipeline, MCP streamable-HTTP server on port 18651, background job worker, and website crawling/embedding — all running locally with zero Docker or Python dependencies.

## Prerequisites

- Node.js 22 or later
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

## Run The Frontend

Starts Vite at `http://localhost:18674`. This port is intentionally separate from the original VectorFlow project.

```bash
npm run dev
```

## Run The Desktop App

Starts Vite and opens the application in a native Tauri window:

```bash
npm run tauri dev
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
# 2. Install Node.js 22+ (https://nodejs.org)
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
