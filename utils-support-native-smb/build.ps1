# SMB Native Library Build Script
# Builds the Rust cdylib and copies to resources/native/<platform>/
#
# Usage: .\build.ps1 [-Target <target-triple>]
#   -Target: windows-msvc, windows-gnu, linux, macos (default: host target)

param(
    [ValidateSet('windows-msvc', 'windows-gnu', 'linux', 'macos')]
    [string]$Target = '')

$ErrorActionPreference = 'Stop'
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$rustDir = Join-Path $scriptDir 'src\main\rust'
$resourcesDir = Join-Path $scriptDir 'src\main\resources\native'

if (-not $Target) {
    $os = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    switch ($os) {
        'X64' { $Target = if ($IsWindows -or $env:OS -eq 'Windows_NT') { 'windows-msvc' } else { 'linux' } }
        'Arm64' { $Target = 'macos' }
    }
}

$targets = @{
    'windows-msvc' = @{
        RustTarget = 'x86_64-pc-windows-msvc'
        Dir        = 'windows-x86_64'
        LibName    = 'rust_smb_server.dll'
    }
    'windows-gnu'  = @{
        RustTarget = 'x86_64-pc-windows-gnu'
        Dir        = 'windows-x86_64'
        LibName    = 'rust_smb_server.dll'
    }
    'linux'        = @{
        RustTarget = 'x86_64-unknown-linux-gnu'
        Dir        = 'linux-x86_64'
        LibName    = 'librust_smb_server.so'
    }
    'macos'        = @{
        RustTarget = 'aarch64-apple-darwin'
        Dir        = 'darwin-aarch64'
        LibName    = 'librust_smb_server.dylib'
    }
}

$t = $targets[$Target]
Write-Output "Building for $Target ($($t.RustTarget))..."

Push-Location $rustDir
try {
    rustup target add $t.RustTarget 2>&1 | Out-Null
    cargo build --release --target $t.RustTarget 2>&1
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }

    $src = Join-Path $rustDir "target\$($t.RustTarget)\release\$($t.LibName)"
    $destDir = Join-Path $resourcesDir $t.Dir
    $dest = Join-Path $destDir $t.LibName

    if (-not (Test-Path $src)) { throw "Compiled library not found: $src" }

    New-Item -ItemType Directory -Force -Path $destDir | Out-Null
    Copy-Item -Path $src -Destination $dest -Force
    Write-Output "OK: $dest ($(Get-Item $dest).Length bytes)"

    # Verify exports
    if ($Target -like 'windows*') {
        $exports = dumpbin /exports $dest 2>&1 | Select-String 'smb_server'
        Write-Output "Exports: $($exports.Count) functions found"
    }
} finally {
    Pop-Location
}
