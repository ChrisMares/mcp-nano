param(
    [switch]$SkipInstall
)

$ErrorActionPreference = 'Stop'

Set-Location -LiteralPath $PSScriptRoot

function Require-Command {
    param([string]$Name)

    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required command '$Name' was not found in PATH."
    }
}

function Invoke-External {
    param(
        [string]$FilePath,
        [string[]]$Arguments
    )

    Write-Host "> $FilePath $($Arguments -join ' ')" -ForegroundColor DarkGray
    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
    }
}

Write-Host 'Checking build prerequisites...' -ForegroundColor Cyan
Require-Command 'npm'
Require-Command 'cargo'
Require-Command 'bash'

if (-not (Get-Command 'nasm' -ErrorAction SilentlyContinue)) {
    # aws-lc-sys provides compatible prebuilt objects for Windows x64.
    $env:AWS_LC_SYS_PREBUILT_NASM = '1'
    Write-Host 'NASM was not found; using AWS-LC prebuilt assembler objects.' -ForegroundColor Yellow
}

if (-not (Test-Path -LiteralPath 'package-lock.json')) {
    throw 'package-lock.json was not found. Run this script from the repository root.'
}

if (-not $SkipInstall) {
    Write-Host 'Installing locked npm dependencies...' -ForegroundColor Cyan
    Invoke-External 'npm' @('ci')
}

Write-Host 'Downloading required ONNX models...' -ForegroundColor Cyan
Invoke-External 'bash' @('src-tauri/scripts/download-models.sh')

Write-Host 'Building the adaptive Windows installer (DirectML GPU with CPU fallback)...' -ForegroundColor Cyan
Invoke-External 'npm' @('run', 'tauri:build:windows')

$BundlePath = Join-Path $PSScriptRoot 'src-tauri/target/release/bundle'
Write-Host "`nBuild complete. Installer files are in:" -ForegroundColor Green
Write-Host $BundlePath -ForegroundColor Green
