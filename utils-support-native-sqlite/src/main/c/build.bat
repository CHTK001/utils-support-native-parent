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

REM %~dp0 带尾部反斜杠。若直接拿去拼 /I"%SRC_DIR%"，展开后是
REM /I"D:\...\src\main\c\"，而 MSVC 的命令行解析把 \" 当作转义引号，
REM 于是这个引号不再闭合，参数会一路吞到下一个引号——连同
REM "%SRC_DIR%sqlite3_hook.c" 一起被吃进 /I 的值里，cl 最终一个输入文件都
REM 没收到，报 D8003 missing source filename。
REM 这个模块此前没有 CI，build.bat 从未在流水线里跑过，所以该缺陷一直没暴露。
REM 这里去掉尾部反斜杠，并在拼接处显式补上分隔符。
set "SRC_DIR=%~dp0"
if "%SRC_DIR:~-1%"=="\" set "SRC_DIR=%SRC_DIR:~0,-1%"
set "OUT_DIR=%SRC_DIR%\..\resources\native\windows-x86_64"
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

REM 把真正展开的命令行打出来。之前 cl 报 D8003 时日志里看不到命令行，
REM 无法判断是续行断了、引号被转义、还是路径不对，只能靠猜。
echo [sqlite3_hook] SRC_DIR = "%SRC_DIR%"
echo [sqlite3_hook] OUT_DIR = "%OUT_DIR%"
echo [sqlite3_hook] 命令行:
echo     cl /nologo %CFLAGS% /I"%SRC_DIR%" /LD /DBUILDING_DLL "%SRC_DIR%\sqlite3_hook.c" /Fe"%OUT_DIR%\sqlite3_hook.dll" /link /out:"%OUT_DIR%\sqlite3_hook.dll"

cl /nologo %CFLAGS% /I"%SRC_DIR%" /LD /DBUILDING_DLL ^
    "%SRC_DIR%\sqlite3_hook.c" ^
    /Fe"%OUT_DIR%\sqlite3_hook.dll" ^
    /link /out:"%OUT_DIR%\sqlite3_hook.dll"

if %errorlevel% neq 0 (
    echo [sqlite3_hook] 构建失败 (error=%errorlevel%)
    echo [sqlite3_hook] 上面那行是 cl 实际收到的参数，据此排查
    exit /b %errorlevel%
)

REM 清理多余的 .lib .exp 文件
if exist "%OUT_DIR%\sqlite3_hook.lib" del "%OUT_DIR%\sqlite3_hook.lib"
if exist "%OUT_DIR%\sqlite3_hook.exp" del "%OUT_DIR%\sqlite3_hook.exp"

echo [sqlite3_hook] 构建成功: %OUT_DIR%\sqlite3_hook.dll
for %%f in ("%OUT_DIR%\sqlite3_hook.dll") do echo     %%~zf 字节

exit /b 0
