# mcp-nano — Windows 11 Build & Run Plan

Native desktop app: **Tauri 2 + Rust + React/Vite**. No Docker, no Python.

| Item | Value |
| --- | --- |
| Repo | `mcp-nano` |
| Node | **22+** (npm 10+) |
| Rust | **stable** via rustup, target `x86_64-pc-windows-msvc` |
| C++ toolchain | **Visual Studio 2022 Build Tools** — workload *Desktop development with C++* |
| WebView2 | Preinstalled on Windows 11 (Edge) |
| Optional GPU | Cargo feature `directml` / `gpu` (no CUDA toolkit) |
| Dev UI | `http://localhost:18674` |
| MCP (when app runs) | `http://127.0.0.1:18651/mcp?server_id=<name>` |
| App data | `%LOCALAPPDATA%\com.mcpquick.mcp-nano\` |

**Build Windows packages on Windows.** Do not cross-compile from Linux for this plan.

## Scripts in this folder

| Script | Purpose |
| --- | --- |
| `inspect-mcp-nano-windows.ps1` | Read-only check of Git, Node, Rust, VS C++, WebView2, bash |
| `install-mcp-nano-windows.ps1` | Install missing tools via winget + rustup |
| `download-models.ps1` | Fetch ONNX models into `src-tauri/resources/models/` |
| `download-qdrant-windows.ps1` | Fetch Windows Qdrant sidecar into `src-tauri/binaries/` |

Run from a PowerShell prompt (ExecutionPolicy may require `-ExecutionPolicy Bypass`):

```powershell
cd C:\path\to\mcp-nano
powershell -ExecutionPolicy Bypass -File documentation\inspect-mcp-nano-windows.ps1
powershell -ExecutionPolicy Bypass -File documentation\install-mcp-nano-windows.ps1
```

---

## Order of operations

1. Clone the repo (Git) — or copy it over, then open PowerShell in the repo root.
2. Run **inspect** → see what is missing.
3. Run **install** (Administrator if winget/VS fails).
4. **Open a new PowerShell** after installing Rust / VS / Node so `PATH` updates.
5. Re-run inspect until checklist is green.
6. `npm install` from repo root.
7. Download **ONNX models** + **Qdrant Windows sidecar**.
8. Dev: `npm run tauri dev` — or release: `npm run tauri build`.
9. Artifacts under `src-tauri\target\release\bundle\msi\` and `nsis\`.

---

## 1. Inspect machine (read-only)

```powershell
powershell -ExecutionPolicy Bypass -File documentation\inspect-mcp-nano-windows.ps1
```

---

## 2. Install missing packages

Prefer **winget**. Elevate if needed.

```powershell
powershell -ExecutionPolicy Bypass -File documentation\install-mcp-nano-windows.ps1
```

Installs (when missing):

- Git for Windows (also provides `bash` for `npm run ensure:qdrant`)
- Node.js LTS (need major ≥ 22)
- VS 2022 Build Tools + `Microsoft.VisualStudio.Workload.VCTools`
- Rust stable via `rustup-init` (host `x86_64-pc-windows-msvc`)
- WebView2 Runtime if registry probe fails

### Manual fallbacks (if winget blocked)

| Tool | URL / note |
| --- | --- |
| Git | https://git-scm.com/download/win |
| Node 22+ | https://nodejs.org/ (LTS) |
| Rust | https://rustup.rs/ → `rustup-init.exe`, choose MSVC |
| VS Build Tools | https://visualstudio.microsoft.com/visual-cpp-build-tools/ → *Desktop development with C++* |
| WebView2 | https://developer.microsoft.com/microsoft-edge/webview2/ |

---

## 3. Clone and npm install

```powershell
$Root = 'C:\src'
New-Item -ItemType Directory -Force -Path $Root | Out-Null
Set-Location $Root

git clone <YOUR_REPO_URL> mcp-nano
Set-Location mcp-nano

node -v
npm -v
rustc -vV
cargo -vV

npm install
```

First `cargo` / `tauri` build downloads crates (needs network; can take a while).

---

## 4. Download build assets (models + Qdrant)

Models and Qdrant are **gitignored**. Without them embedders fail and the Qdrant sidecar is missing.

### PowerShell (no bash required for assets)

```powershell
# From repo root
powershell -ExecutionPolicy Bypass -File documentation\download-models.ps1
powershell -ExecutionPolicy Bypass -File documentation\download-qdrant-windows.ps1
# Force re-download Qdrant:
# powershell -ExecutionPolicy Bypass -File documentation\download-qdrant-windows.ps1 -Force
```

### Git Bash (stock repo scripts)

```powershell
bash src-tauri/scripts/download-models.sh
bash src-tauri/scripts/download-qdrant.sh
```

### `ensure:qdrant` and bash

`package.json` runs `bash src-tauri/scripts/download-qdrant.sh` on `tauri dev` / `tauri build`.

Install **Git for Windows** so `bash` is on PATH. If you already ran `download-qdrant-windows.ps1`, the Windows binary is present; `ensure:qdrant` still needs bash to succeed (or no-op after both platforms exist via the bash script).

```powershell
$env:Path = "C:\Program Files\Git\bin;$env:Path"
```

---

## 5. Run (dev)

```powershell
Set-Location C:\src\mcp-nano

npm run dev
# -> http://localhost:18674

npm run tauri dev

# Optional DirectML GPU EP (falls back to CPU)
npm run tauri:dev:gpu
```

Force CPU at runtime:

```powershell
$env:MCP_NANO_DEVICE = 'cpu'
npm run tauri dev
```

---

## 6. Production build (MSI / NSIS)

```powershell
Set-Location C:\src\mcp-nano

# Models MUST already be present (not auto-fetched by tauri build)
npm run tauri build
# npm run tauri:build:gpu

Get-ChildItem src-tauri\target\release\bundle\msi
Get-ChildItem src-tauri\target\release\bundle\nsis
```

| Artifact | Path |
| --- | --- |
| MSI | `src-tauri\target\release\bundle\msi\mcp-nano_0.1.0_x64_en-US.msi` |
| NSIS | `src-tauri\target\release\bundle\nsis\mcp-nano_0.1.0_x64-setup.exe` |

Installer layout:

```text
C:\Program Files\mcp-nano\
  mcp-nano.exe
  qdrant.exe
  models\arctic-embed-xs\...
  models\minilm-l6-v2\...
```

---

## 7. Post-run / debug paths

| What | Where |
| --- | --- |
| App data | `%LOCALAPPDATA%\com.mcpquick.mcp-nano\` |
| Logs | `%LOCALAPPDATA%\com.mcpquick.mcp-nano\logs\` |
| SQLite | `...\app.db` |
| Qdrant storage | `...\qdrant\` |

```powershell
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\com.mcpquick.mcp-nano"
```

---

## 8. Troubleshooting

| Symptom | Fix |
| --- | --- |
| `link.exe` / `MSVC` not found | Install VS Build Tools + C++ workload; new terminal; or “x64 Native Tools Command Prompt for VS 2022” |
| `rustup`/`cargo` not found | Restart shell; `%USERPROFILE%\.cargo\bin` on PATH |
| `bash` not found on `npm run tauri dev` | Install Git for Windows; put `Git\bin` on PATH |
| Embedders fail / empty models | Run `documentation\download-models.ps1` |
| Qdrant sidecar missing | Run `documentation\download-qdrant-windows.ps1` |
| WebView2 errors | Install Evergreen WebView2 Runtime |
| Node too old | Node 22+ |
| Slow first build | Several GB free under `%USERPROFILE%\.cargo` and `src-tauri\target` |
| GPU issues | `$env:MCP_NANO_DEVICE='cpu'` |

---

## 9. Minimal happy-path checklist

```text
[ ] winget available (admin if needed)
[ ] documentation\inspect-mcp-nano-windows.ps1
[ ] documentation\install-mcp-nano-windows.ps1
[ ] New PowerShell — inspect all [OK]
[ ] git clone + cd mcp-nano
[ ] npm install
[ ] documentation\download-models.ps1
[ ] documentation\download-qdrant-windows.ps1
[ ] npm run tauri dev
[ ] (later) npm run tauri build → MSI/NSIS
```

```powershell
cd C:\src\mcp-nano
npm install
powershell -ExecutionPolicy Bypass -File documentation\download-models.ps1
powershell -ExecutionPolicy Bypass -File documentation\download-qdrant-windows.ps1
npm run tauri dev
```

---

## Related docs

- `README.md` — quick prereqs
- `documentation/production_build.md` — packaging, bundle layout, app data
