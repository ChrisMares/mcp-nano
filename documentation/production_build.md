# Production Build Guide (Linux & Windows 11)

How to produce installers for **mcp-nano** with ONNX models baked into the package and **Qdrant** shipped as a Tauri sidecar. Also documents what lands on the user's machine and where runtime data lives.

Bundle identity (from `src-tauri/tauri.conf.json`):

| Field | Value |
| --- | --- |
| Product name | `mcp-nano` |
| Version | `0.1.0` |
| Identifier | `com.mcpquick.mcp-nano` |

After install there is **no Docker, no Python, and no first-run model download**. Everything needed to run is in the installer except optional GPU runtimes (CUDA/cuDNN on Linux).

---

## What Gets Baked Into The Build

Configured in `tauri.conf.json` under `bundle`:

```json
"externalBin": ["binaries/qdrant"],
"resources": {
  "resources/models/arctic-embed-xs/*": "models/arctic-embed-xs/",
  "resources/models/minilm-l6-v2/*": "models/minilm-l6-v2/"
}
```

| Asset | Build-time source (gitignored) | Role |
| --- | --- | --- |
| Dense embedder | `src-tauri/resources/models/arctic-embed-xs/` | Snowflake arctic-embed-xs ONNX + tokenizer (384-dim) |
| Reranker | `src-tauri/resources/models/minilm-l6-v2/` | ms-marco-MiniLM-L6-v2 ONNX + tokenizer |
| Qdrant sidecar | `src-tauri/binaries/qdrant-<target-triple>[.exe]` | Local vector DB process |

BM25 stopwords are compiled into the Rust binary (no separate file).

Approximate sizes:

| Component | Size |
| --- | --- |
| Models (both ONNX + tokenizers) | ~174 MB |
| Qdrant binary | ~85 MB |
| App binary + UI | ~40 MB |
| **Installed footprint (typical)** | **~300 MB** |
| Compressed installer | ~180–200 MB |

---

## Build Machine Prerequisites

### Shared (both OS)

- Node.js 22+
- npm 10+
- Rust stable via [rustup](https://rustup.rs/)
- Network access once to fetch models and Qdrant (not needed on end-user machines)

### Linux (Ubuntu 24.04 example)

```bash
sudo apt update
sudo apt install -y \
  build-essential curl wget file patchelf pkg-config libxdo-dev \
  libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev
```

For `.deb` / `.rpm` / `.AppImage` packaging, also ensure `dpkg`, `rpmbuild` (optional), and AppImage tooling that Tauri pulls in during bundle are available.

### Windows 11

1. Install Rust via `rustup-init.exe` (MSVC toolchain).
2. Install Node.js 22+.
3. Install **Visual Studio 2022 Build Tools** (or full VS) with workload **Desktop development with C++**.
4. WebView2 is present on Windows 11 by default (Edge).
5. Git Bash or WSL is convenient for the download scripts (they are bash). PowerShell alternatives: run the same `curl`/`tar`/`Expand-Archive` steps manually if needed.

Cross-compiling Windows installers from Linux is not covered here; build Windows packages on Windows 11.

---

## One-Time Asset Download (Before Every Clean Build)

Models and Qdrant binaries are **not in git**. Fetch them on the build machine:

```bash
# From repo root
bash src-tauri/scripts/download-models.sh
bash src-tauri/scripts/download-qdrant.sh
# or: npm run ensure:qdrant
```

### Models (`download-models.sh`)

Writes:

```text
src-tauri/resources/models/
├── arctic-embed-xs/
│   ├── model.onnx
│   ├── tokenizer.json
│   └── config.json
└── minilm-l6-v2/
    ├── model.onnx
    ├── tokenizer.json
    └── config.json
```

Sources (Hugging Face):

- Dense: `Snowflake/snowflake-arctic-embed-xs` → `onnx/model.onnx`
- Reranker: `cross-encoder/ms-marco-MiniLM-L6-v2` → `onnx/model.onnx`

Re-runnable: skips files that already exist with non-zero size.

### Qdrant sidecar (`download-qdrant.sh`)

Pinned version: **Qdrant v1.18.3** (SHA-256 verified).

Writes both platform binaries so a single CI checkout can build either OS later:

```text
src-tauri/binaries/
├── qdrant-x86_64-unknown-linux-gnu
└── qdrant-x86_64-pc-windows-msvc.exe
```

Tauri renames the matching triple to plain `qdrant` / `qdrant.exe` next to the app binary at bundle time (`externalBin: ["binaries/qdrant"]`).

Force re-download:

```bash
npm run download:qdrant
# or
bash src-tauri/scripts/download-qdrant.sh --force
```

`npm run tauri build` already runs `beforeBuildCommand`: `npm run ensure:qdrant && npm run build`. **Models are not auto-downloaded** — always run `download-models.sh` first or the package will ship empty `models/`.

---

## Build Commands

### Frontend + full native package

```bash
npm install
bash src-tauri/scripts/download-models.sh
bash src-tauri/scripts/download-qdrant.sh

# CPU default (both platforms)
npm run tauri build
```

`targets` is `"all"` in `tauri.conf.json`, so Linux produces deb/rpm/AppImage when tooling allows; Windows produces MSI/NSIS per Tauri defaults.

### GPU-enabled builds (optional)

Cargo features in `src-tauri/Cargo.toml`:

| Feature | OS | Runtime requirement |
| --- | --- | --- |
| `cuda` | Linux | CUDA ≥ 12.8 + cuDNN ≥ 9 on the **user** machine |
| `directml` | Windows | DirectML / recent GPU drivers (no CUDA toolkit) |
| `gpu` | both | Enables both EPs; runtime picks per OS, falls back to CPU |

```bash
# Convenience script
npm run tauri:build:gpu
# equivalent:
npm run tauri -- build --features gpu
```

Force CPU at runtime even on a GPU build:

```bash
export MCP_NANO_DEVICE=cpu   # Linux
set MCP_NANO_DEVICE=cpu      # Windows cmd
```

### Output artifacts

| Platform | Typical path |
| --- | --- |
| Linux `.deb` | `src-tauri/target/release/bundle/deb/mcp-nano_0.1.0_amd64.deb` |
| Linux `.AppImage` | `src-tauri/target/release/bundle/appimage/mcp-nano_0.1.0_amd64.AppImage` |
| Linux `.rpm` | `src-tauri/target/release/bundle/rpm/mcp-nano-0.1.0-1.x86_64.rpm` |
| Windows `.msi` | `src-tauri/target/release/bundle/msi/mcp-nano_0.1.0_x64_en-US.msi` |
| Windows NSIS | `src-tauri/target/release/bundle/nsis/mcp-nano_0.1.0_x64-setup.exe` |

Staging before packaging also places next to the release binary:

```text
src-tauri/target/release/
├── mcp-nano[.exe]
├── qdrant[.exe]          # sidecar (triple stripped)
└── models/
    ├── arctic-embed-xs/
    └── minilm-l6-v2/
```

---

## How Installation Works On The User's Machine

### Linux — `.deb` (recommended for Ubuntu/Debian)

```bash
sudo apt install ./mcp-nano_0.1.0_amd64.deb
# or
sudo dpkg -i mcp-nano_0.1.0_amd64.deb
```

Typical install layout (Tauri 2 Linux bundle):

```text
/usr/bin/mcp-nano              # main app
/usr/bin/qdrant                # sidecar binary (same directory as exe)
/usr/lib/mcp-nano/models/      # resource_dir()/models (ONNX + tokenizers)
/usr/share/applications/mcp-nano.desktop
/usr/share/icons/hicolor/.../apps/mcp-nano.png
```

Runtime resolution:

- Executable parent → finds `qdrant` next to `mcp-nano`
- `app.path().resource_dir()/models` → bundled ONNX trees

Uninstall:

```bash
sudo apt remove mcp-nano
# App data under ~/.local/share/ is NOT removed automatically
```

### Linux — `.AppImage`

```bash
chmod +x mcp-nano_0.1.0_amd64.AppImage
./mcp-nano_0.1.0_amd64.AppImage
```

No system install. Binary, `qdrant`, and `models/` live inside the image and are mounted when it runs. User data still goes to the normal app data directory (below).

### Linux — `.rpm`

```bash
sudo rpm -i mcp-nano-0.1.0-1.x86_64.rpm
# or dnf/zypper install ./mcp-nano-....rpm
```

Same conceptual layout as deb (`/usr/bin` + resources under `/usr/lib/mcp-nano`).

### Windows 11 — `.msi` or NSIS setup

1. Double-click the `.msi` or `*-setup.exe`.
2. Accept UAC if prompted.
3. Default install root: `C:\Program Files\mcp-nano\` (or x86 equivalent).

Typical install layout:

```text
C:\Program Files\mcp-nano\
├── mcp-nano.exe
├── qdrant.exe
├── models\
│   ├── arctic-embed-xs\
│   │   ├── model.onnx
│   │   ├── tokenizer.json
│   │   └── config.json
│   └── minilm-l6-v2\
│       ├── model.onnx
│       ├── tokenizer.json
│       └── config.json
└── (icons / uninstall metadata)
```

WebView2 is required (preinstalled on Windows 11). GPU builds use DirectML; no separate CUDA install for end users.

Uninstall via **Settings → Apps → mcp-nano**, or the MSI uninstaller. **AppData is not deleted** on uninstall.

---

## App Data Directory (Created At First Run)

All mutable state uses Tauri `app.path().app_local_data_dir()`, which follows the bundle identifier `com.mcpquick.mcp-nano`.

| OS | Path |
| --- | --- |
| **Linux** | `~/.local/share/com.mcpquick.mcp-nano/` |
| **Windows 11** | `%LOCALAPPDATA%\com.mcpquick.mcp-nano\` → typically `C:\Users\<user>\AppData\Local\com.mcpquick.mcp-nano\` |

### Layout (both platforms)

```text
com.mcpquick.mcp-nano/
├── app.db                      # SQLite (jobs, MCP config, file registry)
├── app.db-wal                  # SQLite WAL (when present)
├── app.db-shm
├── data_schema_version         # stamp; mismatch wipes local vector/DB data
├── qdrant.pid                  # sidecar PID while running
├── qdrant/                     # Qdrant storage_path
│   ├── collections/
│   │   ├── codebase/           # code embeddings (dense 384 + sparse BM25)
│   │   └── general/            # documents / websites
│   ├── snapshots/
│   └── ...
├── uploads/                    # copies of ingested zips/files for jobs
│   └── <job_uuid>_<filename>
├── logs/
│   ├── mcp-nano.log            # release rotating logs (flexi_logger)
│   ├── mcp-nano-debug.log      # debug builds
│   ├── qdrant-sidecar.log      # Qdrant stdout/stderr
│   ├── ingest-current.txt      # last ingestion breadcrumb
│   └── last-panic.log          # panic dumps
└── (WebKit/WebView cache dirs managed by the shell)
```

### What is *not* in app data

| Item | Location |
| --- | --- |
| ONNX models | Install / resource dir only (read-only at runtime) |
| Qdrant binary | Next to the main executable |
| App UI assets | Inside the installed package / AppImage |

Models are loaded from disk via ONNX Runtime (`ort`); they are **not** copied into app data and **not** compiled into the executable.

---

## Runtime Startup Sequence

When the user launches mcp-nano:

1. **Logging** → `logs/` under app local data.
2. **Schema check** → if `data_schema_version` ≠ current stamp, wipe `app.db*`, `qdrant/`, and `uploads/`, then restamp.
3. **Spawn Qdrant sidecar** next to the exe, with env:
   - `QDRANT__SERVICE__HOST=127.0.0.1`
   - `QDRANT__SERVICE__HTTP_PORT` / `GRPC_PORT` → ephemeral free ports
   - `QDRANT__STORAGE__STORAGE_PATH` → `<app_local_data>/qdrant`
   - `QDRANT__STORAGE__SNAPSHOTS_PATH` → `<app_local_data>/qdrant/snapshots`
4. Wait for `http://127.0.0.1:<http>/readyz`, connect gRPC, ensure collections `codebase` + `general` and payload indexes.
5. **SQLite** → open/migrate `<app_local_data>/app.db`.
6. **Embedders** → load `resource_dir()/models/{arctic-embed-xs,minilm-l6-v2}` (GPU EP if built-in and available, else CPU).
7. **MCP HTTP** → `http://127.0.0.1:18651/mcp` (fallback to an ephemeral port if busy). Clients use `?server_id=<name>`.
8. **Background worker** → polls jobs (max 2 concurrent).
9. On app exit → cancel worker, kill Qdrant, clear `qdrant.pid`.

Nothing binds on public interfaces; Qdrant and MCP are localhost-only.

---

## End-User Checklist

**After installing the package, the user should:**

1. Launch **mcp-nano** from the app menu / Start menu / AppImage.
2. Wait until backend status shows Qdrant, DB, embedders, and worker ready (first launch creates the data dir and collections).
3. Point MCP clients (e.g. OpenCode, Claude Desktop) at:
   ```text
   http://127.0.0.1:18651/mcp?server_id=<their_server_name>
   ```
4. Optional: set `MCP_NANO_DEVICE=cpu` if a GPU build misbehaves.

**They do not need to:**

- Install Docker, Python, Node, or Rust
- Download models or Qdrant
- Open firewall ports
- Configure a Qdrant URL

**GPU (optional):**

- Linux CUDA build: user must have compatible NVIDIA driver + CUDA 12.8+ and cuDNN 9+ libraries on `LD_LIBRARY_PATH` / system paths.
- Windows DirectML build: current GPU drivers are enough; falls back to CPU automatically on failure.

---

## Full Clean Build Recipe (Copy-Paste)

### Linux

```bash
git clone <repo-url> mcp-nano && cd mcp-nano
npm install
bash src-tauri/scripts/download-models.sh
bash src-tauri/scripts/download-qdrant.sh

# CPU
npm run tauri build

# or GPU (CUDA EP compiled in)
npm run tauri:build:gpu

# Artifacts
ls src-tauri/target/release/bundle/deb/
ls src-tauri/target/release/bundle/appimage/
```

### Windows 11 (Git Bash or similar)

```powershell
git clone <repo-url> mcp-nano
cd mcp-nano
npm install
bash src-tauri/scripts/download-models.sh
bash src-tauri/scripts/download-qdrant.sh

npm run tauri build
# or: npm run tauri:build:gpu

dir src-tauri\target\release\bundle\msi
dir src-tauri\target\release\bundle\nsis
```

---

## Wiping User Data (Dev / Support)

Stop the app first, then:

```bash
# Linux
rm -rf ~/.local/share/com.mcpquick.mcp-nano
```

```powershell
# Windows PowerShell
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\com.mcpquick.mcp-nano"
```

Next launch recreates DB, Qdrant storage, and empty collections. Installed binaries and models are untouched.

---

## Troubleshooting Packaging

| Symptom | Likely cause |
| --- | --- |
| App starts but embedders fail | Forgot `download-models.sh` before build; empty `models/` in package |
| “bundled Qdrant binary not found” | Forgot `download-qdrant.sh` / wrong triple in `binaries/` |
| Huge link times / OOM | Do not embed models with `include_bytes!`; keep resource bundling |
| Schema wipe on upgrade | Intentional when `DATA_SCHEMA_VERSION` changes in `qdrant.rs` |
| Port 18651 in use | MCP falls back to ephemeral port; check UI connection info |
| GPU build crashes on user PC | Missing CUDA/cuDNN (Linux) or driver issue; set `MCP_NANO_DEVICE=cpu` |

Check logs:

- Linux: `~/.local/share/com.mcpquick.mcp-nano/logs/`
- Windows: `%LOCALAPPDATA%\com.mcpquick.mcp-nano\logs\`

Especially `qdrant-sidecar.log` and `mcp-nano.log` / `mcp-nano-debug.log`.
