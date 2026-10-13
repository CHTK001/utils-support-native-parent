<#
.SYNOPSIS
  Build the chua_native_imageopt cdylib on Windows.
.DESCRIPTION
  Unlike the libjpeg-turbo module this needs no CMake/nasm/vendor tarball:
  libdeflate and libimagequant C sources ship inside the libdeflater / imagequant
  crates and are compiled by the `cc` crate at cargo build time. So this script
  is a thin wrapper around cargo that also stages the artifact into
  src/main/resources/native/<platform>/.

  By default it builds for the host target with rustup's default toolchain, which
  is what CI wants. Pass -RustTarget / -Toolchain only when cross-compiling, e.g. on a
  workstation whose default toolchain cannot link (no Visual Studio Build Tools):
    -RustTarget x86_64-pc-windows-gnu -Toolchain stable-x86_64-pc-windows-gnu
.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1
#>
param(
    [string]$Platform = '',
    # 空 = 不传 --target，直接构建宿主目标
    [string]$RustTarget = '',
    # 空 = 用 rustup 默认工具链
    [string]$Toolchain = '',
    [string]$LibraryName = 'chua_native_imageopt'
)

$ErrorActionPreference = 'Stop'
$RustDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$NativeDir = Join-Path (Split-Path -Parent $RustDir) 'resources/native'
if (-not $Platform) {
    # build.ps1 只在 Windows 上跑，宿主就是 windows-x86_64
    $Platform = 'windows-x86_64'
}
$NativeOut = Join-Path $NativeDir $Platform

function Step($msg) { Write-Host "[build] $msg" }

# 只有显式给了工具链才覆盖，否则清空 RUSTUP_TOOLCHAIN 会让 cargo 找不到默认工具链。
if ($Toolchain) {
    Step "toolchain $Toolchain"
    $env:RUSTUP_TOOLCHAIN = $Toolchain
} else {
    Remove-Item Env:RUSTUP_TOOLCHAIN -ErrorAction SilentlyContinue
}

if ($RustTarget) {
    Step "cargo build --release --target $RustTarget"
    & cargo build --release --target $RustTarget
} else {
    Step "cargo build --release (host target)"
    & cargo build --release
}
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$ReleaseDir = if ($RustTarget) { Join-Path $RustDir "target\$RustTarget\release" } else { Join-Path $RustDir 'target\release' }
$Built = Join-Path $ReleaseDir "$LibraryName.dll"
if (-not (Test-Path $Built)) { throw "expected artifact missing: $Built" }
New-Item -ItemType Directory -Force -Path $NativeOut | Out-Null
Copy-Item $Built (Join-Path $NativeOut "$LibraryName.dll") -Force
Step ("artifact: " + (Join-Path $NativeOut "$LibraryName.dll"))
Step 'done'
