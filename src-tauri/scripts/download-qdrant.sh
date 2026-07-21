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

cleanup() {
  rm -rf "$DOWNLOAD_DIR"
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
  unzip -q -o "$DOWNLOAD_DIR/$WINDOWS_ARCHIVE" -d "$DOWNLOAD_DIR/windows"

  windows_binary="$DOWNLOAD_DIR/windows/qdrant.exe"
  if [[ ! -f "$windows_binary" ]]; then
    printf 'Could not find qdrant.exe in %s\n' "$WINDOWS_ARCHIVE" >&2
    exit 1
  fi
  install -m 644 "$windows_binary" "$windows_destination"
fi

printf 'Qdrant v%s sidecars are ready in %s\n' "$QDRANT_VERSION" "$BINARIES_DIR"
