# Downloads/installs Git, Node LTS, Rust (rustup), VS 2022 Build Tools (C++), WebView2 if needed.
# Prefer running PowerShell as Administrator if winget/VS install fails without elevation.
# Usage: powershell -ExecutionPolicy Bypass -File documentation\install-mcp-nano-windows.ps1

#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

function Assert-Winget {
    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        throw "winget not found. Install 'App Installer' from Microsoft Store, then re-open PowerShell."
    }
}

function Install-WingetPackage {
    param(
        [Parameter(Mandatory)][string]$Id,
        [string]$Name = $Id
    )
    Write-Host "`n>>> Installing $Name ($Id) ..." -ForegroundColor Cyan
    winget install --id $Id -e --accept-package-agreements --accept-source-agreements --disable-interactivity
}

function Test-Cmd { param([string]$n) [bool](Get-Command $n -ErrorAction SilentlyContinue) }

Assert-Winget

if (-not (Test-Cmd 'git')) {
    Install-WingetPackage -Id 'Git.Git' -Name 'Git'
} else {
    Write-Host "Git already present: $(git --version)" -ForegroundColor Green
}

$needNode = $true
if (Test-Cmd 'node') {
    $major = [int](((node -v) -replace '^v', '').Split('.')[0])
    if ($major -ge 22) {
        $needNode = $false
        Write-Host "Node already OK: $(node -v)" -ForegroundColor Green
    } else {
        Write-Host "Node too old ($(node -v)); will install Node LTS." -ForegroundColor Yellow
    }
}
if ($needNode) {
    Install-WingetPackage -Id 'OpenJS.NodeJS.LTS' -Name 'Node.js LTS'
}

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$hasMsvc = $false
if (Test-Path $vswhere) {
    $p = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
    if ($p) { $hasMsvc = $true }
}
if (-not $hasMsvc) {
    Write-Host "`n>>> Installing VS 2022 Build Tools with C++ workload (large download) ..." -ForegroundColor Cyan
    winget install --id Microsoft.VisualStudio.2022.BuildTools -e --accept-package-agreements --accept-source-agreements --disable-interactivity `
        --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
} else {
    Write-Host "MSVC / VC Tools already detected." -ForegroundColor Green
}

if (-not (Test-Cmd 'rustup') -and -not (Test-Cmd 'cargo')) {
    Write-Host "`n>>> Installing Rust (rustup-init, MSVC toolchain) ..." -ForegroundColor Cyan
    $rustup = Join-Path $env:TEMP 'rustup-init.exe'
    Invoke-WebRequest -Uri 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile $rustup
    & $rustup -y --default-toolchain stable --default-host x86_64-pc-windows-msvc
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path $cargoBin) {
        $env:Path = "$cargoBin;$env:Path"
    }
} else {
    Write-Host "Rust tooling already present." -ForegroundColor Green
    if (Test-Cmd 'rustup') {
        rustup default stable
        rustup target add x86_64-pc-windows-msvc
    }
}

$wvOk = $false
foreach ($k in @(
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
        'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    )) {
    if ((Test-Path $k) -and (Get-ItemProperty $k -EA SilentlyContinue).pv) { $wvOk = $true; break }
}
if (-not $wvOk) {
    Install-WingetPackage -Id 'Microsoft.EdgeWebView2Runtime' -Name 'WebView2 Runtime'
} else {
    Write-Host "WebView2 already present." -ForegroundColor Green
}

Write-Host @"

=== Install script finished ===
1. CLOSE this PowerShell window and open a NEW one (PATH + VS env).
2. Re-run: powershell -ExecutionPolicy Bypass -File documentation\inspect-mcp-nano-windows.ps1
3. Follow documentation\windows_setup.md (clone, npm install, download assets, tauri dev).

Optional full VS 2022 Community instead of Build Tools:
  winget install Microsoft.VisualStudio.2022.Community --override "--wait --passive --add Microsoft.VisualStudio.Workload.NativeDesktop --includeRecommended"

"@ -ForegroundColor Green
