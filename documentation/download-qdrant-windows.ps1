# Download Qdrant Windows sidecar into src-tauri/binaries/ (pinned v1.18.3, SHA-256 verified).
# Usage:
#   powershell -ExecutionPolicy Bypass -File documentation\download-qdrant-windows.ps1
#   powershell -ExecutionPolicy Bypass -File documentation\download-qdrant-windows.ps1 -Force

param(
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$QdrantVersion = '1.18.3'
$Archive = 'qdrant-x86_64-pc-windows-msvc.zip'
$ExpectedSha = '984619bbd4032ace578656174c465c5d6b71d1267ecad5b7b4c21cc6549ca833'

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
$BinDir = Join-Path $RepoRoot 'src-tauri\binaries'
$DestExe = Join-Path $BinDir 'qdrant-x86_64-pc-windows-msvc.exe'

if (-not $Force -and (Test-Path $DestExe)) {
    Write-Host "Already present: $DestExe"
    exit 0
}

New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
$Work = Join-Path $env:TEMP ("qdrant-dl-" + [guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $Work | Out-Null
try {
    $Url = "https://github.com/qdrant/qdrant/releases/download/v$QdrantVersion/$Archive"
    $Zip = Join-Path $Work $Archive
    Write-Host "Downloading $Url"
    Invoke-WebRequest -Uri $Url -OutFile $Zip -UseBasicParsing

    $hash = (Get-FileHash -Algorithm SHA256 -Path $Zip).Hash.ToLowerInvariant()
    if ($hash -ne $ExpectedSha.ToLowerInvariant()) {
        throw "SHA256 mismatch: got $hash expected $ExpectedSha"
    }

    Expand-Archive -Path $Zip -DestinationPath (Join-Path $Work 'out') -Force
    $src = Join-Path $Work 'out\qdrant.exe'
    if (-not (Test-Path $src)) { throw "qdrant.exe missing inside zip" }
    Copy-Item -Force $src $DestExe
    Write-Host "Wrote $DestExe"
} finally {
    Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}
