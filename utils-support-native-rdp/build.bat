@echo off
REM
REM 构建 RDP 桥接库 rdpj（FreeRDP 3.x + MSVC），输出到
REM src\main\resources\native\windows-x86_64\rdpj.dll
REM
REM   build.bat                    # 构建当前平台
REM   build.bat --sdk C:\sdk       # 使用已有的 FreeRDP SDK 目录
REM   build.bat --clean            # 删除已放入 native\ 的产物
REM   build.bat --offline          # 不自动拉取 FreeRDP 原（必须 --sdk）
REM
setlocal enabledelayedexpansion
set NAME=rdpj
set "ROOT=%~dp0"
set "SRC=%ROOT%src\main\c\rdpj"
set "RES=%ROOT%src\main\resources\native"
set SDK=unknown
set OFFLINE=0
set CLEAN=0

:parseloop
if "%1"=="" goto :parsedone
if /i "%~1"=="--sdk" (
  set SDK=%~2
  shift
  shift
) else if /i "%~1"=="--sdk=" (
  set SDK=%~1
  set SDK=!SDK:--sdk=!
  shift
) else if /i "%~1"=="--clean" (
  set CLEAN=1
  shift
) else if /i "%~1"=="--offline" (
  set OFFLINE=1
  shift
) else if /i "%~1"=="-h" or /i "%~1"=="--help" (
  goto :usage
) else (
  echo 未知参数: %1
  goto :usage
)
goto :parseloop

:parsedone

if !CLEAN!==1 (
  echo 清除已生成的 native 产物
  for /F "usebackq delims=" %%d in (`dir /B /AD "%RES%\*"`) do (
    if exist "%%d\%NAME%.so" del "%%d\%NAME%.so"
    if exist "%%d\%NAME%.dylib" del "%%d\%NAME%.dylib"
    if exist "%%d\%NAME%.dll" del "%%d\%NAME%.dll"
    rmdir "%%d" 2>/dev/null
  )
  echo 完成。
  goto :done
)

:usage
echo 发布: build.bat [选项]
echo   --sdk DIR    FreeRDP 3.x SDK 目录 （寻找 include\freerdp\freerdp.h 与 libfreerdp3.lib）
echo   --clean      删除已放入 src\main\resources\native\<os>-<arch>\ 的产物
echo   --offline    不自动拉取 FreeRDP 源（必须 --sdk）
echo   -h, --help   帮助
goto :done

REM ============================================================
REM 1. 平台探测
REM ============================================================
echo 平台: windows x86_64
set PLATFORM=windows
set ARCH=x86_64
set PLATFORM_DIR=windows-x86_64
set TARGET_DIR=%RES%\%PLATFORM_DIR%
if not exist "%TARGET_DIR%" mkdir "%TARGET_DIR%"

REM ============================================================
REM 2. 寻找 FreeRDP SDK
REM ============================================================
if !SDK!==unknown (
  for /d %%d in ("C:\vcpkg\installed\x64-windows-static" "C:\vcpkg\installed\x64-windows" "C:\freerdp" "C:\dev\freerdp" "D:\freerdp" "C:\Program Files\FreeRDP" "%USERPROFILE%\.freerdp-sdk") do (
    if exist "%%d\include\freerdp\freerdp.h" (
      if exist "%%d\lib\libfreerdp3.lib" set SDK=%%d
    )
  )
)

echo SDK: !SDK!

if !SDK!==unknown if !OFFLINE! equ 0 (
  echo 未找到 FreeRDP SDK，尝试从 GitHub 拉取（免登录预编译的 x64 开发包）...
  set URL=https://github.com/FreeRDP/FreeRDP/releases/download/freerdp-release-3-3/freerdp-3.3.0-x64-dev.zip
  curl -sSL --max-time 180 -o %TEMP%\freerdp.zip %URL% 2>/dev/null || wget -q --timeout=180 -O %TEMP%\freerdp.zip %URL% 2>/dev/null
  if exist %TEMP%\freerdp.zip (
    7z x -o%TEMP%\freerdp-sdk %TEMP%\freerdp.zip >nul 2>&1
    if exist %TEMP%\freerdp-sdk\include\freerdp\freerdp.h set SDK=%TEMP%\freerdp-sdk
  )
  if !SDK!==unknown (
    echo 无法拉取 FreeRDP 开发包，请提供 --sdk C:\path
    goto :done
  )
)

if not exist "!SDK!\include\freerdp\freerdp.h" (
  echo 不是 FreeRDP SDK: 找不到 !SDK!\include\freerdp\freerdp.h
  goto :done
)
if not exist "!SDK!\lib\libfreerdp3.lib" (
  echo 不是 FreeRDP SDK: 找不到 !SDK!\lib\libfreerdp3.lib
  goto :done
)

REM ============================================================
REM 3. 编译 rdpj
REM ============================================================
set SHARED_OUT=%TARGET_DIR%\rdpj.dll
echo 开始编译 rdpj （MSVC cl.exe），输出: %SHARED_OUT%

REM 找出 cl.exe
set VCINSTALLDIR=
for /f "tokens=2,* delims= " %%a in ('vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationpath 2^>nul') do (
  set VCINSTALLDIR=%%b
)
if not defined VCINSTALLDIR (
  set VCINSTALLDIR=C:\Program Files\Microsoft Visual Studio\2022\Community
)
for %%f in ("!VCINSTALLDIR!\VC\Tools\MSVC\*\bin\Hostx64\x64\cl.exe") do (
  if exist "%%f" set CL="%%f"
)
if not defined CL (
  set CL=cl.exe
)
echo cl: !CL!
if not exist !CL! (
  echo 无法定位 cl.exe，请在 Developer PowerShell 中手动执行:
  echo   cl.exe /LD /I!SDK!\include /Fe%SHARED_OUT% %SRC%\rdpj.c !SDK!\lib\libfreerdp3.lib
  goto :done
)

REM 设定 vcvarsall 环境（x64 工具链）
set VCVARS=!VCINSTALLDIR!\VC\Auxiliary\Build\vcvarsall.bat
if exist !VCVARS! (
  call "!VCVARS!" x64 2>nul || echo 自动桥接环境模块失败
)

cl.exe /LD /DNDEBUG /W4 /I "!SDK!\include" /I "!SDK!\include\freerdp" \
  /Fe"%SHARED_OUT%" "%SRC%\rdpj.c" "!SDK!\lib\libfreerdp3.lib" "!SDK!\lib\winpr.lib" 2>"%SRC%\compile.log" || (
  echo 编译失败，查看 %SRC%\compile.log
  type "%SRC%\compile.log"
  goto :done
)

if exist "%SHARED_OUT%" (
  echo 产物: %SHARED_OUT%
  dir /B "%SHARED_OUT%"
  goto :done
)

echo 未生成产物 rdpj.dll（查看 %SRC%\compile.log 中的 cl 输出）

:done
echo 完成。已生成: %RES%\%PLATFORM_DIR%\rdpj.dll
