#!/usr/bin/env bash
#
# 构建 RDP 桥接库 librdpj（FreeRDP 3.x），输出到
# src/main/resources/native/<os>-<arch>/ （linux: librdpj.so, darwin: librdpj.dylib,
# windows: rdpj.dll，JNA 按 *rdpj* 样例提取并 Native.load）。
#
#   ./build.sh                 # 编译当前平台并自动放入 src/main/resources/native/<os>-<arch>/
#   ./build.sh --sdk /path/sdk # 使用已有的 FreeRDP SDK 目录
#   ./build.sh --clean         # 删除已放入 native/ 的产物
#   ./build.sh --offline       # 不自动拉取 FreeRDP 源（必须 --sdk）
#
# 默认会把 FreeRDP 3.x 从 GitHub 下载源码并 cmake 编译到 /tmp/freerdp-sdk（约 3-8 分钟）。
set -euo pipefail

# ============================================================
# 1. 常量
# ============================================================
NAME="rdpj"
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SRC="$ROOT/src/main/c/rdpj"
RES="$ROOT/src/main/resources/native"

OPTION_SDK=""
OPTION_CLEAN=0
OPTION_OFFLINE=0

usage() {
  echo "发布: ./build.sh [选项]"
  echo "  --sdk     DIR  FreeRDP 3.x SDK 目录（寻找 include/freerdp/freerdp.h 与 libfreerdp3.*）"
  echo "  --clean         删除已放入 src/main/resources/native/<os>-<arch>/ 的产物"
  echo "  --offline       不自动拉取 FreeRDP 源（必须 --sdk）"
  echo "  -h, --help      帮助"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --sdk)
      [ $# -ge 2 ] || { echo "缺少 --sdk 路径参数" >&2; exit 2; }
      OPTION_SDK="$2"; shift 2 ;;
    --sdk=*)
      OPTION_SDK="${1#--sdk=}"; shift ;;
    --clean)
      OPTION_CLEAN=1; shift ;;
    --offline)
      OPTION_OFFLINE=1; shift ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "未知参数: $1" >&2; usage; exit 2 ;;
  esac
done

if [ "$OPTION_CLEAN" -eq 1 ]; then
  echo "== 清除已生成的 native/ 产物 =="
  shopt -s nullglob
  for d in "$RES"/*; do
    if [ -d "$d" ] && [ -f "$d/$NAME".so -o -f "$d/$NAME".dylib -o -f "$d/$NAME".dll ]; then
      rm -rf "$d"
      echo "已删除: $d"
    fi
  done
  shopt -u nullglob
  echo "完成。"
  exit 0
fi

# ============================================================
# 2. 平台探测
# ============================================================
MACHINE="${MACHINE:-$(uname -m)}"
case "$(uname -s)" in
  Linux*)  PLATFORM="linux" ;;
  Darwin*) PLATFORM="darwin" ;;
  CYGWIN*|MINGW*|MSYS*) PLATFORM="windows" ;;
  SunOS*)  PLATFORM="sunos" ;;
  AIX*)    PLATFORM="aix" ;;
  *)       PLATFORM="unknown" ;;
esac

case "$MACHINE" in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  i386|i686|i86) ARCH="x86" ;;
  *)             ARCH="$MACHINE" ;;
esac

PLATFORM_DIR="${PLATFORM}-${ARCH}"
echo "平台: $PLATFORM / $MACHINE / $ARCH"
TARGET_DIR="$RES/$PLATFORM_DIR"
mkdir -p "$TARGET_DIR"

# ============================================================
# 3. 找 FreeRDP SDK
# ============================================================
find_unix_sdk() {
  local c
  # pkg-config（freerdp3 -> /usr/local/lib/freerdp3 等）
  if command -v pkg-config >/dev/null 2>&1; then
    for spec in freerdp3 freerdp; do
      if pkg-config --atleast-version=3.0 "$spec" 2>/dev/null; then
        c="$(pkg-config --variable=libdir "$spec" 2>/dev/null || echo "")"
        if [ -n "$c" ] && { [ -f "$c/libfreerdp3.so" ] || [ -f "$c/libfreerdp3.dylib" ]; } && [ -f "$(pkg-config --variable=includedir "$spec")/freerdp/freerdp.h" ]; then
          echo "$c"
          return 0
        fi
      fi
    done
  fi
  # 常见目录
  for c in "/usr/local/freerdp" "/opt/freerdp" "/usr/lib/x86_64-linux-gnu/freerdp" "$HOME/.freerdp-sdk"; do
    if [ -f "$c/include/freerdp/freerdp.h" ] && { [ -f "$c/lib/libfreerdp3.so" ] || [ -f "$c/lib/libfreerdp3.dylib" ] || [ -f "$c/lib/libfreerdp3.lib" ]; }; then
      echo "$c"
      return 0
    fi
  done
  return 1
}

sdk="unknown"
if [ -n "$OPTION_SDK" ] && [ -f "$OPTION_SDK/include/freerdp/freerdp.h" ]; then
  sdk="$OPTION_SDK"
else
  sdk="$(find_unix_sdk 2>/dev/null)" || sdk="unknown"
  [ -n "$sdk" ] && sdk="$sdk" || sdk="unknown"
fi

echo "SDK: $sdk"

# ============================================================
# 4. 下载 FreeRDP（可选）
# ============================================================
if [ "$sdk" = "unknown" ] && [ "$OPTION_OFFLINE" -eq 0 ]; then
  echo "未找到 FreeRDP SDK，尝试从 GitHub 拉取源码自行构建（约 3-8 分钟）..."
  if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
    echo "缺少 curl/wget，跳过自动拉取自由端" >&2
    exit 1
  fi
  freerdp_url="https://github.com/FreeRDP/FreeRDP/releases/download/freerdp-release-3-3/freerdp-3.3.0.tar.gz"
  if ! curl -sSL --max-time 120 -o /tmp/freerdp.tar.gz "$freerdp_url" 2>/dev/null &&
     ! wget -q --timeout=120 -O /tmp/freerdp.tar.gz "$freerdp_url" 2>/dev/null; then
    echo "无法拉取 FreeRDP 3.x 源码，请提供 --sdk /path" >&2
    exit 1
  fi
  mkdir -p /tmp/freerdp-src
  tar -xzf /tmp/freerdp.tar.gz -C /tmp/freerdp-src --strip-components=1
  mkdir -p /tmp/freerdp-src/build
  ( cd /tmp/freerdp-src/build && \
    cmake .. -DCMAKE_BUILD_TYPE=Release -DWITH_ZLIB=ON -DWITH_OPENSSL=ON -DWITH_PULSE=OFF \
             -DWITH_ALSA=OFF -DWITH_SSE=ON -DCMAKE_INSTALL_PREFIX=/tmp/freerdp-sdk >/dev/null && \
    make -j"$(nproc 2>/dev/null || echo 4)" >/dev/null && \
    make install >/dev/null )
  sdk="/tmp/freerdp-sdk"
  echo "FreeRDP SDK 已构建: $sdk"
fi

# 确认必需的组件文件
if [ ! -f "$sdk/include/freerdp/freerdp.h" ]; then
  echo "不是 FreeRDP SDK: 找不到 $sdk/include/freerdp/freerdp.h" >&2
  exit 1
fi
if [ ! -f "$sdk/lib/libfreerdp3.so" ] && [ ! -f "$sdk/lib/libfreerdp3.dylib" ]; then
  echo "不是 FreeRDP SDK: 找不到 $sdk/lib/libfreerdp3* 库" >&2
  exit 1
fi
# ============================================================
# 5. 编译 rdpj
# ============================================================
CC="${CC:-cc}"
SHARED_PREFIX=""
SHARED_EXT=""
SHARED_OUT=""

if [ "$PLATFORM" = "linux" ]; then
  CC="${CC:-gcc}"
  SHARED_PREFIX="lib"
  SHARED_EXT="so"
elif [ "$PLATFORM" = "darwin" ]; then
  CC="${CC:-clang}"
  SHARED_PREFIX="lib"
  SHARED_EXT="dylib"
elif [ "$PLATFORM" = "windows" ]; then
  echo "== Windows 平台：使用 MSVC (vcpkg 或 FreeRDP SDK) =="
  SHARED_PREFIX=""
  SHARED_EXT="dll"
fi

SHARED_OUT="$TARGET_DIR/${SHARED_PREFIX}${NAME}.${SHARED_EXT}"
echo "开始编译 $NAME （$CC），输出: $SHARED_OUT"

if [ "$PLATFORM" != "windows" ]; then
  # 同一份 rdpj-win32.c 在 POSIX 上走 WinPR 兼容层：
  # HANDLE / CRITICAL_SECTION / CreateThread / WaitForMultipleObjects 均由 WinPR 提供。
  # rpath 指向 $ORIGIN/@loader_path，便于把 libfreerdp3 / libfreerdp-client3 / libwinpr3
  # 与 librdpj 放在同一 native 目录时直接解析（jar 解包后仍能加载）。
  case "$PLATFORM" in
    linux)  RPATH_FLAG="-Wl,-rpath,\$ORIGIN" ;;
    darwin) RPATH_FLAG="-Wl,-rpath,@loader_path" ;;
    *)      RPATH_FLAG="" ;;
  esac
  if command -v pkg-config >/dev/null 2>&1 && pkg-config --exists freerdp3 freerdp-client3 2>/dev/null; then
    FREERDP_CFLAGS="$(pkg-config --cflags freerdp3 freerdp-client3)"
    FREERDP_LIBS="$(pkg-config --libs freerdp3 freerdp-client3)"
  else
    FREERDP_CFLAGS="-I$sdk/include -I$sdk/include/libfreerdp"
    FREERDP_LIBS="-L$sdk/lib -lfreerdp3 -lfreerdp-client3 -lwinpr3"
  fi
  ( cd "$SRC" && \
    $CC -shared -fPIC -O2 -std=gnu11 \
      $FREERDP_CFLAGS \
      $FREERDP_LIBS \
      $RPATH_FLAG \
      -o "$SHARED_OUT" \
      rdpj-win32.c 2>"$SRC/compile.log" || {
        echo "编译失败，查看 $SRC/compile.log" >&2
        tail -40 "$SRC/compile.log" >&2
        exit 1
      } )
else
  echo "Windows 手动（MSVC / cl.exe）："
  echo "  cl.exe /LD /I"$sdk"/include /Fe$SHARED_OUT $SRC/rdpj-win32.c $sdk/lib/libfreerdp3.lib" >&2
  echo "  在 Developer PowerShell 中执行上述 cl.exe 命令" >&2
  exit 0
fi

echo "== 产物: $SHARED_OUT"
ls -la "$SHARED_OUT"
chmod +x "$SHARED_OUT"

# ============================================================
# 6. 验证
# ============================================================
echo "== 验证 $SHARED_OUT =="
if [ "$PLATFORM" = "linux" ]; then
  command -v ldconfig >/dev/null 2>&1 && ldconfig -n "$(dirname "$SHARED_OUT")" 2>/dev/null || echo "(ldconfig 仅检查环境)"
  command -v ldd >/dev/null 2>&1 && ldd "$SHARED_OUT" || command -v objdump >/dev/null 2>&1 && objdump -p "$SHARED_OUT" || echo "(无法验证链接库)"
  command -v strings >/dev/null 2>&1 && strings "$SHARED_OUT" | grep -Fq "freerdp" || echo "(警告: 没有找到 freerdp 符号)"
elif [ "$PLATFORM" = "darwin" ]; then
  command -v otool >/dev/null 2>&1 && otool -L "$SHARED_OUT" || echo "(otool 不可用)"
fi

echo "== 产物复制 native/$PLATFORM_DIR 中 =="
ls -la "$RES/$PLATFORM_DIR"
echo ""
echo "完成。已生成: $RES/$PLATFORM_DIR/${SHARED_PREFIX}${NAME}.${SHARED_EXT}"
