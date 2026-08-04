#!/usr/bin/env bash
set -euo pipefail

QDRANT_VERSION="1.18.3"
LINUX_ARCHIVE="qdrant-x86_64-unknown-linux-gnu.tar.gz"
LINUX_SHA256="60663a254cf421dba4db45710872895cd3a714fe1e6978f7927923b5cfae4718"
WINDOWS_ARCHIVE="qdrant-x86_64-pc-windows-msvc.zip"
WINDOWS_SHA256="984619bbd4032ace578656174c465c5d6b71d1267ecad5b7b4c21cc6549ca833"

if [[ "${1:-}" == "--force" ]]; then
  force_download=true
elif [[ $# -eq 0 ]]; then
  force_download=false
else
  printf 'Usage: %s [--force]\n' "$0" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAURI_DIR="$(dirname "$SCRIPT_DIR")"
BINARIES_DIR="$TAURI_DIR/binaries"
DOWNLOAD_DIR="$(mktemp -d)"
WINDOWS_ARCHIVE_PATH="$TAURI_DIR/.qdrant-windows.zip"
WINDOWS_EXTRACT_DIR="$TAURI_DIR/.qdrant-windows"

cleanup() {
  rm -rf "$DOWNLOAD_DIR"
  rm -rf "$WINDOWS_ARCHIVE_PATH" "$WINDOWS_EXTRACT_DIR"
}
trap cleanup EXIT

download_and_verify() {
  local archive="$1"
  local checksum="$2"
  local url="https://github.com/qdrant/qdrant/releases/download/v${QDRANT_VERSION}/${archive}"
  local destination="$DOWNLOAD_DIR/$archive"

  curl --fail --location --retry 3 --output "$destination" "$url"
  printf '%s  %s\n' "$checksum" "$destination" | sha256sum --check --status
}

windows_path() {
  local path="$1"
  if [[ "$path" =~ ^/mnt/([a-zA-Z])/(.*)$ ]]; then
    local drive="${BASH_REMATCH[1]^^}"
    local rest="${BASH_REMATCH[2]//\//\\}"
    printf '%s:\\%s' "$drive" "$rest"
  else
    printf '%s' "$path"
  fi
}

mkdir -p "$BINARIES_DIR"

linux_destination="$BINARIES_DIR/qdrant-x86_64-unknown-linux-gnu"
windows_destination="$BINARIES_DIR/qdrant-x86_64-pc-windows-msvc.exe"

if [[ "$force_download" == false && -x "$linux_destination" && -f "$windows_destination" ]]; then
  printf 'Qdrant v%s sidecars already exist in %s\n' "$QDRANT_VERSION" "$BINARIES_DIR"
  exit 0
fi

if [[ "$force_download" == true || ! -x "$linux_destination" ]]; then
  download_and_verify "$LINUX_ARCHIVE" "$LINUX_SHA256"
  tar --extract --gzip --file "$DOWNLOAD_DIR/$LINUX_ARCHIVE" --directory "$DOWNLOAD_DIR"

  linux_binary="$DOWNLOAD_DIR/qdrant"
  if [[ ! -f "$linux_binary" ]]; then
    printf 'Could not find qdrant in %s\n' "$LINUX_ARCHIVE" >&2
    exit 1
  fi
  install -m 755 "$linux_binary" "$linux_destination"
fi

if [[ "$force_download" == true || ! -f "$windows_destination" ]]; then
  download_and_verify "$WINDOWS_ARCHIVE" "$WINDOWS_SHA256"
  cp "$DOWNLOAD_DIR/$WINDOWS_ARCHIVE" "$WINDOWS_ARCHIVE_PATH"
  rm -rf "$WINDOWS_EXTRACT_DIR"

  if command -v unzip >/dev/null 2>&1; then
    unzip -q -o "$WINDOWS_ARCHIVE_PATH" -d "$WINDOWS_EXTRACT_DIR"
  elif command -v powershell.exe >/dev/null 2>&1; then
    windows_archive_path="$(windows_path "$WINDOWS_ARCHIVE_PATH")"
    windows_extract_dir="$(windows_path "$WINDOWS_EXTRACT_DIR")"
    powershell.exe -NoProfile -Command \
      "Expand-Archive -LiteralPath '$windows_archive_path' -DestinationPath '$windows_extract_dir' -Force"
  else
    printf 'Could not extract %s: install unzip or use Windows PowerShell\n' "$WINDOWS_ARCHIVE" >&2
    exit 1
  fi

  windows_binary="$WINDOWS_EXTRACT_DIR/qdrant.exe"
  if [[ ! -f "$windows_binary" ]]; then
    printf 'Could not find qdrant.exe in %s\n' "$WINDOWS_ARCHIVE" >&2
    exit 1
  fi
  install -m 644 "$windows_binary" "$windows_destination"
fi

printf 'Qdrant v%s sidecars are ready in %s\n' "$QDRANT_VERSION" "$BINARIES_DIR"
