@echo off
setlocal EnableExtensions
REM ============================================================================
REM CUDA 运行时库一键准备脚本（Windows）
REM 自动: 探测驱动 CUDA 版本 -> 读取 scripts\cuda.env 配置 -> pip 下载 nvidia-*-cu12
REM       -> 拷贝 DLL 到 TARGET_DIR -> 校验关键库 -> 可选加入 PATH
REM 用法: setup-cuda.bat [目标目录] [CUDA主版本]
REM ============================================================================
set "SCRIPT_DIR=%~dp0"
set "ENV_FILE=%SCRIPT_DIR%cuda.env"

REM ---------- 读取 cuda.env 配置（禁止硬编码） ----------
for /f "usebackq tokens=1,* delims==" %%a in ("%ENV_FILE%") do (
    if not "%%a"=="" if not "%%a:~0,1%"=="#" set "%%a=%%b"
)

if "%~1"=="" (set "TARGET=%TARGET_DIR%") else (set "TARGET=%~1")
if "%~2"=="" (set "CUDA_VER=%CUDA_MAJOR%") else (set "CUDA_VER=%~2")
set "ROOT=%SCRIPT_DIR%.."
set "FULL_TARGET=%ROOT%\%TARGET%"
if not exist "%FULL_TARGET%" mkdir "%FULL_TARGET%"

echo ====================================================
echo  CUDA 运行库准备 (Windows, CUDA %CUDA_VER%)
echo  目标目录: %FULL_TARGET%
echo ====================================================

REM ---------- 1. 探测 NVIDIA 驱动 ----------
nvidia-smi >nul 2>&1
if errorlevel 1 (
    echo [ERROR] 未检测到 NVIDIA 驱动，请先安装: https://www.nvidia.com/Download/index.aspx
    exit /b 1
)
for /f "tokens=3" %%v in ('nvidia-smi --query-gpu=driver_version --format=csv,noheader') do set "DRIVER=%%v"
echo  驱动版本: %DRIVER%

REM ---------- 2. 关键库已存在则跳过下载 ----------
set "NEED=0"
if not exist "%FULL_TARGET%\cudart64_%CUDA_VER%.dll" set "NEED=1"
if not exist "%FULL_TARGET%\cublas64_%CUDA_VER%.dll" set "NEED=1"
if not exist "%FULL_TARGET%\cudnn64_9.dll" set "NEED=1"
if "%NEED%"=="0" (
    echo [SKIP] 关键库已就绪，无需下载
    goto :verify
)

REM ---------- 3. venv 隔离安装 nvidia 运行时包 ----------
set "VENV=%ROOT%\target\cuda-venv"
if not exist "%VENV%\Scripts\python.exe" python -m venv "%VENV%" >nul 2>&1
if not exist "%VENV%\Scripts\pip.exe" (
    echo [ERROR] venv 创建失败，请确认 python 可用
    exit /b 1
)
for %%p in (%PIP_RUNTIME% %PIP_CUBLAS% %PIP_CUDNN%) do (
    echo   pip install %%p ...
    "%VENV%\Scripts\pip.exe" install --quiet --disable-pip-version-check %%p
    if errorlevel 1 (
        echo [WARN] %%p 安装失败，尝试直接下载兜底...
        powershell -Command "Invoke-WebRequest -Uri '%CUDART_URL%' -OutFile '%FULL_TARGET%\cuda_installer.exe' -TimeoutSec %DOWNLOAD_TIMEOUT_SEC%"
    )
)

REM ---------- 4. 拷贝 DLL 到目标目录 ----------
echo  拷贝 DLL 到 %FULL_TARGET% ...
for /r "%VENV%\Lib\site-packages" %%f in (*.dll) do (
    copy /y "%%f" "%FULL_TARGET%\" >nul 2>&1
)

:verify
REM ---------- 5. 校验 ----------
set "MISSING="
if not exist "%FULL_TARGET%\cudart64_%CUDA_VER%.dll" set "MISSING=%MISSING% cudart64_%CUDA_VER%.dll"
if not exist "%FULL_TARGET%\cublas64_%CUDA_VER%.dll" set "MISSING=%MISSING% cublas64_%CUDA_VER%.dll"
if not exist "%FULL_TARGET%\cudnn64_9.dll" set "MISSING=%MISSING% cudnn64_9.dll"
if not "%MISSING%"=="" (
    echo [ERROR] 缺失库:%MISSING%
    exit /b 1
)

REM ---------- 6. 可选加入 PATH ----------
if /i "%AUTO_PATH%"=="true" (
    echo   AUTO_PATH=true: 请将以下目录加入系统 PATH:
    echo     %FULL_TARGET%
)

echo [OK] CUDA 运行库就绪: %FULL_TARGET%
exit /b 0
