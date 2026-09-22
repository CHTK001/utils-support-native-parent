<#
.SYNOPSIS
  Build the libjpeg-turbo static core and the chua_native_turbojpeg cdylib on Windows.
.DESCRIPTION
  Downloads the pinned upstream source, configures it with CMake + NASM (SIMD on),
  builds a static libturbojpeg, then links the Rust facade against it and copies the
  resulting DLL into src/main/resources/native/<platform>/.
  Requirements on PATH: cmake, ninja (or mingw32-make), nasm, cargo.
.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1
#>
param(
    [string]$Version = '3.1.2',
    [string]$Platform = 'windows-x86_64',
    [string]$RustTarget = 'x86_64-pc-windows-gnu',
    [string]$CCompiler = '',
    [switch]$SkipFetch
)

$ErrorActionPreference = 'Stop'
$RustDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Vendor = Join-Path $RustDir 'vendor'
$Src = Join-Path $Vendor "libjpeg-turbo-$Version"
$LibDir = Join-Path $Src 'build'
$NativeDir = Join-Path (Split-Path -Parent $RustDir) 'resources\native'
$NativeOut = Join-Path $NativeDir $Platform

function Step($msg) { Write-Host "[build] $msg" }

New-Item -ItemType Directory -Force -Path $Vendor | Out-Null

if (-not $SkipFetch -and -not (Test-Path (Join-Path $LibDir 'libturbojpeg.a')) -and
    -not (Test-Path (Join-Path $LibDir 'libturbojpeg.lib'))) {
    $Tarball = Join-Path $Vendor "libjpeg-turbo-$Version.tar.gz"
    if (-not (Test-Path $Tarball)) {
        Step "downloading libjpeg-turbo $Version source"
        curl.exe -sSL --ssl-no-revoke -o $Tarball `
            "https://github.com/libjpeg-turbo/libjpeg-turbo/archive/refs/tags/$Version.tar.gz"
        if ($LASTEXITCODE -ne 0) { throw 'source download failed' }
    }
    if (-not (Test-Path $Src)) {
        Step 'extracting source'
        & 'C:\Windows\System32\tar.exe' xzf $Tarball -C $Vendor
        if ($LASTEXITCODE -ne 0) { throw 'extraction failed' }
    }
}

if (-not (Test-Path (Join-Path $LibDir 'libturbojpeg.a')) -and
    -not (Test-Path (Join-Path $LibDir 'libturbojpeg.lib'))) {
    Step 'configuring libjpeg-turbo (static, SIMD)'
    $cmakeArgs = @(
        '-S', $Src, '-B', $LibDir, '-G', 'Ninja',
        '-DCMAKE_BUILD_TYPE=Release',
        '-DENABLE_SHARED=OFF', '-DENABLE_STATIC=ON',
        '-DWITH_JPEG8=ON', '-DWITH_SIMD=ON', '-DWITH_TURBOJPEG=ON',
        '-DWITH_TOOLS=OFF', '-DWITH_TESTS=OFF', '-DWITH_FUZZ=OFF'
    )
    if ($CCompiler -ne '') { $cmakeArgs += @("-DCMAKE_C_COMPILER=$CCompiler") }
    & cmake @cmakeArgs
    if ($LASTEXITCODE -ne 0) { throw 'cmake configure failed' }
    Step 'building libjpeg-turbo'
    & cmake --build $LibDir
    if ($LASTEXITCODE -ne 0) { throw 'cmake build failed' }
}

Step "building rust facade for $RustTarget"
$env:TURBOJPEG_LIB_DIR = $LibDir
Push-Location $RustDir
try {
    & cargo build --release --target $RustTarget
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
} finally {
    Pop-Location
}

$Built = Join-Path $RustDir "target\$RustTarget\release\chua_native_turbojpeg.dll"
if (-not (Test-Path $Built)) { throw "expected artifact missing: $Built" }
New-Item -ItemType Directory -Force -Path $NativeOut | Out-Null
Copy-Item $Built (Join-Path $NativeOut 'chua_native_turbojpeg.dll') -Force
Step ("artifact: " + (Join-Path $NativeOut 'chua_native_turbojpeg.dll'))
Step 'done'
