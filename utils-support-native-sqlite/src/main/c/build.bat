@echo off
REM ============================================================
REM  sqlite3_hook.dll 构建脚本
REM  运行时动态加载 winsqlite3.dll（Windows 10/11 内置）或 sqlite3.dll
REM  无需 SQLite 合并包（C 源码使用 LoadLibrary 动态绑定）
REM
REM  依赖：
REM    1. Visual Studio 2022 BuildTools（含 C++ 工作负载）
REM
REM  用法：
REM    build.bat                  — 编译 Release 版本
REM    build.bat debug            — 编译 Debug 版本
REM ============================================================

setlocal enabledelayedexpansion

set BUILD_TYPE=%1
if "%BUILD_TYPE%"=="" set BUILD_TYPE=release

set SRC_DIR=%~dp0
set OUT_DIR=%SRC_DIR%..\resources\native\windows-x86_64

if not exist "%OUT_DIR%" mkdir "%OUT_DIR%"

REM ==================== 1. 设置 Visual Studio 环境 ====================

echo [sqlite3_hook] 设置 Visual Studio 编译环境 ...

set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" set "VSWHERE=%ProgramFiles%\Microsoft Visual Studio\Installer\vswhere.exe"

if exist "%VSWHERE%" (
    for /f "tokens=*" %%i in ('"%VSWHERE%" -latest -property installationPath') do set VS_PATH=%%i
) else (
    set "VS_PATH=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"
)

set "VCVARS=%VS_PATH%\VC\Auxiliary\Build\vcvarsall.bat"
if not exist "%VCVARS%" (
    echo [sqlite3_hook] 错误: 未找到 vcvarsall.bat
    echo   请确保已安装 Visual Studio 2022 BuildTools（含 C++ 工作负载）
    exit /b 1
)

call "%VCVARS%" x64

REM ==================== 2. 编译 DLL ====================

echo [sqlite3_hook] 构建 %BUILD_TYPE% 版本 ...

if /i "%BUILD_TYPE%"=="debug" (
    set CFLAGS=/Od /Zi /MDd
) else (
    set CFLAGS=/O2 /MD
)

cl /nologo %CFLAGS% /I"%SRC_DIR%" /LD /DBUILDING_DLL ^
    "%SRC_DIR%sqlite3_hook.c" ^
    /Fe"%OUT_DIR%\sqlite3_hook.dll" ^
    /link /out:"%OUT_DIR%\sqlite3_hook.dll"

if %errorlevel% neq 0 (
    echo [sqlite3_hook] 构建失败 (error=%errorlevel%)
    exit /b %errorlevel%
)

REM 清理多余的 .lib .exp 文件
if exist "%OUT_DIR%\sqlite3_hook.lib" del "%OUT_DIR%\sqlite3_hook.lib"
if exist "%OUT_DIR%\sqlite3_hook.exp" del "%OUT_DIR%\sqlite3_hook.exp"

echo [sqlite3_hook] 构建成功: %OUT_DIR%\sqlite3_hook.dll
for %%f in ("%OUT_DIR%\sqlite3_hook.dll") do echo     %%~zf 字节

exit /b 0
