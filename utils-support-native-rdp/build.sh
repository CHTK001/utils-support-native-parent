#!/usr/bin/env bash
#
# Build the RDP bridge library (FreeRDP 3.x) for Linux / macOS.
#
#   ./build.sh                 # detect FreeRDP via pkg-config, build current platform
#   ./build.sh --sdk /path/sdk # use a local FreeRDP SDK (include/ + lib/)
#   ./build.sh --clean         # remove generated native/<platform>/ libraries
#   ./build.sh --offline       # never download FreeRDP (requires --sdk or pkg-config)
#
# Output:
#   src/main/resources/native/linux-x86_64/librdpj.so
#   src/main/resources/native/darwin-<arch>/librdpj.dylib
#
# The same C source (rdpj-win32.c) is used on Windows and POSIX. On POSIX,
# WinPR provides the Windows compatibility layer (HANDLE / CRITICAL_SECTION /
# CreateThread / WaitForMultipleObjects / Interlocked*).
set -euo pipefail

NAME="rdpj"
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SRC="$ROOT/src/main/c/rdpj"
RES="$ROOT/src/main/resources/native"

OPTION_SDK=""
OPTION_CLEAN=0
OPTION_OFFLINE=0

usage() {
  echo "Usage: ./build.sh [options]"
  echo "  --sdk DIR     FreeRDP 3.x SDK directory (include/ + lib/)"
  echo "  --clean       remove generated native/<platform>/ libraries"
  echo "  --offline     do not download FreeRDP"
  echo "  -h, --help    show this help"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --sdk)
      [ $# -ge 2 ] || { echo "missing --sdk path" >&2; exit 2; }
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
      echo "unknown argument: $1" >&2; usage; exit 2 ;;
  esac
done

if [ "$OPTION_CLEAN" -eq 1 ]; then
  echo "== remove generated native libraries =="
  shopt -s nullglob
  for d in "$RES"/*; do
    if [ -d "$d" ] && { [ -f "$d/librdpj.so" ] || [ -f "$d/librdpj.dylib" ] || [ -f "$d/rdpj.dll" ]; }; then
      rm -rf "$d"
      echo "removed: $d"
    fi
  done
  shopt -u nullglob
  exit 0
fi

# ------------------------------------------------------------
# platform
# ------------------------------------------------------------
MACHINE="${MACHINE:-$(uname -m)}"
case "$(uname -s)" in
  Linux*)  PLATFORM="linux" ;;
  Darwin*) PLATFORM="darwin" ;;
  CYGWIN*|MINGW*|MSYS*) PLATFORM="windows" ;;
  *)       PLATFORM="unknown" ;;
esac

case "$MACHINE" in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) ARCH="$MACHINE" ;;
esac

PLATFORM_DIR="${PLATFORM}-${ARCH}"
TARGET_DIR="$RES/$PLATFORM_DIR"
mkdir -p "$TARGET_DIR"
echo "platform: $PLATFORM / $MACHINE / $ARCH"

# ------------------------------------------------------------
# FreeRDP SDK
# ------------------------------------------------------------
SDK_SOURCE="unknown"

if [ -n "$OPTION_SDK" ]; then
  if [ -f "$OPTION_SDK/include/freerdp/freerdp.h" ]; then
    SDK_SOURCE="$OPTION_SDK"
  else
    echo "not a FreeRDP SDK: missing $OPTION_SDK/include/freerdp/freerdp.h" >&2
    exit 1
  fi
fi

if [ "$SDK_SOURCE" = "unknown" ] && command -v pkg-config >/dev/null 2>&1 \
   && pkg-config --atleast-version=3.0 freerdp3 2>/dev/null; then
  SDK_SOURCE="pkg-config"
fi

if [ "$SDK_SOURCE" = "unknown" ]; then
  for c in "/usr/local/freerdp" "/opt/freerdp" "$HOME/.freerdp-sdk"; do
    if [ -f "$c/include/freerdp/freerdp.h" ] \
       && { [ -f "$c/lib/libfreerdp3.so" ] || [ -f "$c/lib/libfreerdp3.dylib" ]; }; then
      SDK_SOURCE="$c"
      break
    fi
  done
fi

if [ "$SDK_SOURCE" = "unknown" ]; then
  if [ "$OPTION_OFFLINE" -eq 1 ]; then
    echo "FreeRDP 3.x not found. Install freerdp3-dev / freerdp, or pass --sdk." >&2
    exit 1
  fi
  echo "FreeRDP 3.x not found. Install freerdp3-dev (Linux) or 'brew install freerdp' (macOS)." >&2
  echo "After installation, re-run ./build.sh (pkg-config will locate it)." >&2
  exit 1
fi

echo "FreeRDP SDK: $SDK_SOURCE"

# ------------------------------------------------------------
# compile
# ------------------------------------------------------------
CC="${CC:-cc}"
if [ "$PLATFORM" = "linux" ]; then
  CC="${CC:-gcc}"
  SHARED_PREFIX="lib"
  SHARED_EXT="so"
  RPATH_FLAG="-Wl,-rpath,\$ORIGIN"
  UNDEFINED_FLAG="-Wl,--no-undefined"
elif [ "$PLATFORM" = "darwin" ]; then
  CC="${CC:-clang}"
  SHARED_PREFIX="lib"
  SHARED_EXT="dylib"
  RPATH_FLAG="-Wl,-rpath,@loader_path"
  UNDEFINED_FLAG=""
else
  echo "Windows: use build.bat / MSVC; this script only builds Linux/macOS." >&2
  exit 0
fi

SHARED_OUT="$TARGET_DIR/${SHARED_PREFIX}${NAME}.${SHARED_EXT}"

if [ "$SDK_SOURCE" = "pkg-config" ]; then
  FREERDP_CFLAGS="$(pkg-config --cflags freerdp3 freerdp-client3 2>/dev/null || true)"
  FREERDP_LIBS="$(pkg-config --libs freerdp3 freerdp-client3 2>/dev/null || true)"
  # 显式补齐 client/winpr：某些发行版的 .pc 只声明核心库，
  # 会让 librdpj 留下 freerdp_disconnect 之类的未解析符号。
  case " $FREERDP_LIBS " in
    *" -lfreerdp-client3 "*) ;;
    *) FREERDP_LIBS="$FREERDP_LIBS -lfreerdp-client3" ;;
  esac
  case " $FREERDP_LIBS " in
    *" -lwinpr3 "*) ;;
    *) FREERDP_LIBS="$FREERDP_LIBS -lwinpr3" ;;
  esac
else
  FREERDP_CFLAGS="-I$SDK_SOURCE/include -I$SDK_SOURCE/include/libfreerdp"
  FREERDP_LIBS="-L$SDK_SOURCE/lib -lfreerdp3 -lfreerdp-client3 -lwinpr3"
fi

echo "compiler: $CC"
echo "cflags:   $FREERDP_CFLAGS"
echo "libs:     $FREERDP_LIBS"

(
  cd "$SRC"
  $CC -shared -fPIC -O2 -std=gnu11 \
    $FREERDP_CFLAGS \
    -o "$SHARED_OUT" \
    rdpj-win32.c \
    $FREERDP_LIBS \
    $RPATH_FLAG \
    $UNDEFINED_FLAG
) 2>"$SRC/compile.log" || {
  echo "compile failed; see $SRC/compile.log" >&2
  tail -60 "$SRC/compile.log" >&2
  exit 1
}

chmod +x "$SHARED_OUT"
echo "artifact: $SHARED_OUT"
ls -la "$SHARED_OUT"

if [ "$PLATFORM" = "linux" ]; then
  command -v ldd >/dev/null 2>&1 && ldd "$SHARED_OUT" || true
elif [ "$PLATFORM" = "darwin" ]; then
  command -v otool >/dev/null 2>&1 && otool -L "$SHARED_OUT" || true
fi

echo "done"
