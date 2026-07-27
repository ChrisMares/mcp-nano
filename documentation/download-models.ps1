# Download ONNX embedder + reranker models into src-tauri/resources/models/.
# Skips files that already exist with non-zero size.
# Usage (from repo root or any cwd):
#   powershell -ExecutionPolicy Bypass -File documentation\download-models.ps1

$ErrorActionPreference = 'Stop'

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
$ModelsDir = Join-Path $RepoRoot 'src-tauri\resources\models'

function Fetch-File {
    param([string]$Url, [string]$Dest)
    if ((Test-Path $Dest) -and ((Get-Item $Dest).Length -gt 0)) {
        Write-Host "  [skip] $Dest"
        return
    }
    $dir = Split-Path $Dest -Parent
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Write-Host "  [get]  $Dest"
    $tmp = "$Dest.tmp"
    Invoke-WebRequest -Uri $Url -OutFile $tmp -UseBasicParsing
    Move-Item -Force $tmp $Dest
}

Write-Host "Dense embedder: Snowflake/snowflake-arctic-embed-xs"
$dense = 'https://huggingface.co/Snowflake/snowflake-arctic-embed-xs/resolve/main'
Fetch-File "$dense/onnx/model.onnx" (Join-Path $ModelsDir 'arctic-embed-xs\model.onnx')
Fetch-File "$dense/tokenizer.json" (Join-Path $ModelsDir 'arctic-embed-xs\tokenizer.json')
Fetch-File "$dense/config.json" (Join-Path $ModelsDir 'arctic-embed-xs\config.json')

Write-Host "Reranker: cross-encoder/ms-marco-MiniLM-L6-v2"
$rr = 'https://huggingface.co/cross-encoder/ms-marco-MiniLM-L6-v2/resolve/main'
Fetch-File "$rr/onnx/model.onnx" (Join-Path $ModelsDir 'minilm-l6-v2\model.onnx')
Fetch-File "$rr/tokenizer.json" (Join-Path $ModelsDir 'minilm-l6-v2\tokenizer.json')
Fetch-File "$rr/config.json" (Join-Path $ModelsDir 'minilm-l6-v2\config.json')

Get-ChildItem -Recurse $ModelsDir | Select-Object FullName, Length
Write-Host "Models done under $ModelsDir"
