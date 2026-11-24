# PaperSharp AOT 动态库构建脚本（Windows / win-x64）。
# 产物：NativeHost.dll（含 PaperSharp 引擎，Native AOT）+ 随包依赖 libSkiaSharp.dll、libHarfBuzzSharp.dll。
# 构建后自动拷贝到 src/main/resources/native/windows-x86_64/，随 jar 分发。
#
# 本机工具链（2026-11-24 搭建）：
#   dotnet SDK 10.0.100  -> D:\apps\dotnet（便携解压，未装系统级）
#   MSVC 14.44 链接器    -> D:\VSBuildTools2（DevShell 提供 link.exe/LIB/INCLUDE）
# PaperSharp 包从 nuget.org 还原（直连可达）。
param(
    [string]$Configuration = "Release"
)
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot

$env:DOTNET_ROOT = "D:\apps\dotnet"
$env:DOTNET_MULTILEVEL_LOOKUP = "0"
# vswhere.exe 不在系统 PATH，但 ilcompiler 的链接目标解析依赖它，必须前置其安装目录
$env:PATH = "D:\apps\dotnet;${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer;$env:PATH"

Import-Module "D:\VSBuildTools2\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath "D:\VSBuildTools2" -DevCmdArguments "-arch=x64 -host_arch=x64" -SkipAutomaticLocation | Out-Null

dotnet publish "$root\NativeHost\NativeHost.csproj" -c $Configuration -r win-x64
if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed: $LASTEXITCODE" }

$pub = Join-Path $root "NativeHost\bin\$Configuration\net10.0\win-x64\publish"
# build.ps1 位于 src/main/native/，资源根是其上一级 src/main/resources/
$dest = Join-Path $root "..\resources\native\windows-x86_64"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
foreach ($dll in @("NativeHost.dll", "libSkiaSharp.dll", "libHarfBuzzSharp.dll")) {
    Copy-Item (Join-Path $pub $dll) $dest -Force
    Write-Output "copied $dll -> $dest"
}
Get-ChildItem $dest | Select-Object Name, Length | Format-Table -AutoSize
