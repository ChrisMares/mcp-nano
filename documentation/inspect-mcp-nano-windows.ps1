# Read-only inventory of tools needed to build mcp-nano on Windows.
# Usage: powershell -ExecutionPolicy Bypass -File documentation\inspect-mcp-nano-windows.ps1

$ErrorActionPreference = 'Continue'
Write-Host "=== mcp-nano Windows environment inspect ===" -ForegroundColor Cyan
Write-Host "Time: $(Get-Date -Format o)"
Write-Host "User: $env:USERNAME  Host: $env:COMPUTERNAME"
Write-Host "OS: $([System.Environment]::OSVersion.VersionString)"
Write-Host "Arch: $env:PROCESSOR_ARCHITECTURE"
Write-Host ""

function Test-Cmd {
    param([string]$Name, [string]$VersionArgs = '--version')
    $cmd = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $cmd) {
        [pscustomobject]@{ Tool = $Name; Status = 'MISSING'; Path = ''; Version = '' }
        return
    }
    $ver = ''
    try {
        $ver = (& $Name $VersionArgs.Split(' ') 2>&1 | Out-String).Trim()
        if ($ver.Length -gt 200) { $ver = $ver.Substring(0, 200) + '...' }
    } catch {
        $ver = '(could not read version)'
    }
    [pscustomobject]@{ Tool = $Name; Status = 'OK'; Path = $cmd.Source; Version = $ver }
}

$rows = @()
$rows += Test-Cmd 'git'
$rows += Test-Cmd 'node'
$rows += Test-Cmd 'npm'
$rows += Test-Cmd 'rustc'
$rows += Test-Cmd 'cargo'
$rows += Test-Cmd 'rustup'
$rows += Test-Cmd 'winget'
$rows += Test-Cmd 'choco' ''
$rows += Test-Cmd 'bash'
$rows += Test-Cmd 'curl.exe' '--version'
$rows += Test-Cmd 'tar' '--version'

Write-Host "--- Commands on PATH ---" -ForegroundColor Yellow
$rows | Format-Table -AutoSize -Wrap

$nodeOk = $false
if (Get-Command node -ErrorAction SilentlyContinue) {
    $nv = (node -v) -replace '^v', ''
    $major = [int]($nv.Split('.')[0])
    $nodeOk = $major -ge 22
    Write-Host "Node.js version parse: $nv  (need >= 22) -> $(if ($nodeOk) {'OK'} else {'TOO OLD'})"
}

Write-Host ""
Write-Host "--- Rust ---" -ForegroundColor Yellow
if (Get-Command rustup -ErrorAction SilentlyContinue) {
    rustup show
    Write-Host ""
    rustup target list --installed
    $msvc = rustup target list --installed 2>$null | Select-String 'x86_64-pc-windows-msvc'
    if (-not $msvc) {
        Write-Host "WARN: x86_64-pc-windows-msvc not installed. Run: rustup default stable-x86_64-pc-windows-msvc" -ForegroundColor Red
    }
} else {
    Write-Host "rustup not found" -ForegroundColor Red
}

Write-Host ""
Write-Host "--- Visual C++ / VS Build Tools ---" -ForegroundColor Yellow
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$hasVs = $false
if (Test-Path $vswhere) {
    $inst = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
    if ($inst) {
        $hasVs = $true
        Write-Host "MSVC tools found at: $inst" -ForegroundColor Green
        & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -format json |
            ConvertFrom-Json |
            Select-Object installationName, installationVersion, installationPath, productId |
            Format-List
    } else {
        Write-Host "vswhere present but Desktop C++ / VC Tools workload not detected." -ForegroundColor Red
    }
} else {
    Write-Host "vswhere.exe not found — Visual Studio Installer likely missing." -ForegroundColor Red
}

$sdk = Get-ChildItem 'HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots' -ErrorAction SilentlyContinue
if ($sdk) {
    Write-Host "Windows Kits registry key present (SDK likely installed)."
}

Write-Host ""
Write-Host "--- WebView2 Runtime ---" -ForegroundColor Yellow
$wvKeys = @(
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
    'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
    'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
)
$wv = $false
foreach ($k in $wvKeys) {
    if (Test-Path $k) {
        $pv = (Get-ItemProperty $k -ErrorAction SilentlyContinue).pv
        if ($pv) {
            Write-Host "WebView2 detected: $pv ($k)" -ForegroundColor Green
            $wv = $true
            break
        }
    }
}
if (-not $wv) {
    Write-Host "WebView2 runtime not detected in registry (unusual on Win11)." -ForegroundColor Red
}

Write-Host ""
Write-Host "--- Disk free ---" -ForegroundColor Yellow
Get-PSDrive -PSProvider FileSystem |
    Where-Object { $_.Used -ne $null } |
    Select-Object Name, @{N = 'FreeGB'; E = { [math]::Round($_.Free / 1GB, 1) } }, @{N = 'UsedGB'; E = { [math]::Round($_.Used / 1GB, 1) } } |
    Format-Table -AutoSize

Write-Host ""
Write-Host "=== Checklist ===" -ForegroundColor Cyan
function Mark($ok, $label) {
    if ($ok) { Write-Host "[OK]  $label" -ForegroundColor Green }
    else { Write-Host "[!!]  $label" -ForegroundColor Red }
}
Mark [bool](Get-Command git -ErrorAction SilentlyContinue) 'Git'
Mark $nodeOk 'Node.js >= 22'
Mark [bool](Get-Command npm -ErrorAction SilentlyContinue) 'npm'
Mark [bool](Get-Command rustc -ErrorAction SilentlyContinue) 'rustc'
Mark [bool](Get-Command cargo -ErrorAction SilentlyContinue) 'cargo'
Mark [bool](Get-Command rustup -ErrorAction SilentlyContinue) 'rustup'
Mark $hasVs 'VS Build Tools with VC++ x64'
Mark $wv 'WebView2'
Mark [bool](Get-Command bash -ErrorAction SilentlyContinue) 'bash (Git for Windows; needed for npm ensure:qdrant)'
Write-Host ""
Write-Host "If any [!!], run: powershell -ExecutionPolicy Bypass -File documentation\install-mcp-nano-windows.ps1"
Write-Host "After installs, close and reopen PowerShell so PATH refreshes."
