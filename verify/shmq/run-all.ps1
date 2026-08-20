# utils-support-native-shm-queue 全功能测试运行器（Windows）
# 依赖：
#   - common-starter / shm-queue / shm-queue-http 已 mvn install 到 D:\maven-repo
#   - C 库 shmqueue_test.exe 已构建（build-vs/Release）
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$parent = Split-Path (Split-Path $here -Parent) -Parent

Write-Host "===== 0) 构建 C 库单测 =====" -ForegroundColor Cyan
$cmake = "D:\VS\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
$cmod = Join-Path $parent "utils-support-native-shm-queue"
& $cmake --build (Join-Path $cmod "build-vs") --config Release | Out-Null
$env:PATH = (Join-Path $cmod "build-vs\Release") + ";" + $env:PATH
& (Join-Path $cmod "build-vs\Release\shmqueue_test.exe")
if ($LASTEXITCODE -ne 0) { throw "C unit tests failed" }

# ---- Java 依赖 classpath ----
Push-Location $parent
mvn org.apache.maven.plugins:maven-dependency-plugin:3.6.1:build-classpath -pl utils-support-native-shm-queue-http "-Dmdep.outputFile=$env:TEMP\shm_cp.txt" -q | Out-Null
Pop-Location
$deps = (Get-Content "$env:TEMP\shm_cp.txt").Trim()
$base = "D:\maven-repo\com\chua"
$cp = "$here;$base\utils-support-common-starter\4.0.0.42\utils-support-common-starter-4.0.0.42.jar" +
      ";$base\utils-support-native-shm-queue\4.0.0.42\utils-support-native-shm-queue-4.0.0.42.jar" +
      ";$base\utils-support-native-shm-queue-http\4.0.0.42\utils-support-native-shm-queue-http-4.0.0.42.jar;" +
      $deps

Write-Host "`n===== 1) 编译 Java 测试 =====" -ForegroundColor Cyan
javac --release 25 --enable-preview -cp $cp -d $here (Get-ChildItem $here -Filter *.java).FullName
if ($LASTEXITCODE -ne 0) { throw "javac failed" }

Write-Host "`n===== 2) ShmQueue 全功能测试 =====" -ForegroundColor Cyan
java --enable-preview --enable-native-access=ALL-UNNAMED -cp $cp ShmQueueFunctionTest
if ($LASTEXITCODE -ne 0) { throw "ShmQueueFunctionTest failed" }

Write-Host "`n===== 3) ShmHttpServer 全功能测试 =====" -ForegroundColor Cyan
java --enable-preview --enable-native-access=ALL-UNNAMED -cp $cp ShmHttpFunctionTest
if ($LASTEXITCODE -ne 0) { throw "ShmHttpFunctionTest failed" }

Write-Host "`n===== 4) C 吞吐基准 =====" -ForegroundColor Cyan
$env:PATH = (Join-Path $cmod "build-vs\Release") + ";" + $env:PATH
& (Join-Path $cmod "build-vs\Release\shmqueue_bench.exe")

Write-Host "`n===== 5) Java 队列吞吐基准 =====" -ForegroundColor Cyan
java --enable-preview --enable-native-access=ALL-UNNAMED -cp $cp ShmQueueBench

Write-Host "`n===== 6) HTTP 吞吐/延迟基准 =====" -ForegroundColor Cyan
java --enable-preview --enable-native-access=ALL-UNNAMED -cp $cp ShmHttpBench
if ($LASTEXITCODE -ne 0) { throw "ShmHttpBench failed" }

Write-Host "`n===== ALL FUNCTIONAL TESTS PASSED =====" -ForegroundColor Green