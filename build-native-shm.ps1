# 重建 utils-support-native-shm-queue / utils-support-native-shm-queue-http 的构建脚本（Windows）
# 步骤：
#   1) MSVC/CMake 构建 C 共享内存队列库 → 拷贝到 resources/native/windows-x86_64/
#   2) cargo 构建 Rust HTTP 桥接库 → 拷贝到 resources/native/windows-x86_64/
#   3) mvn install 两个 Java 模块
# 说明：native 模块的 pom 已配置 maven-compiler-plugin 3.13.0，PMD 相关插件需跳过
#   （common-starter 的 PMD 插件有已知 bug: aktStatus is NULL）。

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$cmake = "D:\VS\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"

function Invoke-Step([string]$name, [scriptblock]$action) {
    Write-Host "===== $name =====" -ForegroundColor Cyan
    & $action
    if ($LASTEXITCODE -ne 0) { throw "$name failed (exit=$LASTEXITCODE)" }
}

# ---- 1) C 库 ----
$cmod = Join-Path $root "utils-support-native-shm-queue"
Invoke-Step "CMake configure (C library)" {
    & $cmake -S $cmod -B (Join-Path $cmod "build-vs") -G "Visual Studio 17 2022" -A x64
}
Invoke-Step "CMake build (C library)" {
    & $cmake --build (Join-Path $cmod "build-vs") --config Release
}
# CMake post-build 已把 shmqueue.dll 拷到 src/main/resources/native/windows-x86_64/

# ---- 2) Rust HTTP 桥接库 ----
$rmod = Join-Path $root "utils-support-native-shm-queue-http"
Invoke-Step "Cargo build (rust_http_bridge)" {
    & cargo build --release --manifest-path (Join-Path $rmod "src\main\rust\Cargo.toml")
}
$target = Join-Path $rmod "src\main\rust\target\release\rust_http_bridge.dll"
$destDir = Join-Path $rmod "src\main\resources\native\windows-x86_64"
New-Item -ItemType Directory -Path $destDir -Force | Out-Null
Copy-Item $target (Join-Path $destDir "rust_http_bridge.dll") -Force
Write-Host "rust_http_bridge.dll -> $destDir" -ForegroundColor Green

# ---- 3) Maven 安装 ----
Invoke-Step "Maven install (native modules)" {
    mvn install -pl utils-support-native-shm-queue,utils-support-native-shm-queue-http -DskipTests "-Dpmd.skip=true"
}

Write-Host "BUILD COMPLETE" -ForegroundColor Green
