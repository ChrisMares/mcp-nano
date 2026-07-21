# mcp-nano

Native desktop rewrite of VectorFlow, built with Tauri 2, Rust, React, Vite, and Tailwind CSS.

The frontend is currently ported as-is. Backend functionality is not yet implemented in Rust.

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

Build the production frontend bundle:

```bash
npm run build
```

Build the native application and platform package:

```bash
npm run tauri build
```

The frontend output is written to `dist/`. Tauri build artifacts are written under `src-tauri/target/release/`.

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
