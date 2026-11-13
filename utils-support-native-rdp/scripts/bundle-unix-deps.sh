#!/usr/bin/env bash
#
# Bundle the RDP bridge's non-system shared-library dependencies into the
# platform native directory, so the jar can be deployed on hosts without
# FreeRDP / X11 / PulseAudio installed.
#
# Usage:
#   ./bundle-unix-deps.sh <platform-dir> [library-name]
#
# Example:
#   ./bundle-unix-deps.sh src/main/resources/native/linux-x86_64 librdpj.so
#
# Result:
#   <platform-dir>/librdpj.{so,dylib}
#   <platform-dir>/<all transitive non-system deps>
#   <platform-dir>/rdpj.index   （逐行文件名，RdpjBridge 解包时使用）
set -euo pipefail

TARGET_DIR="${1:?usage: bundle-unix-deps.sh <platform-dir> [library-name]}"
LIB_NAME="${2:-}"
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

if [ -z "$TARGET_DIR" ] || [ ! -d "$TARGET_DIR" ]; then
  echo "target dir not found: $TARGET_DIR" >&2
  exit 1
fi
TARGET_DIR="$(cd "$TARGET_DIR" && pwd)"

if [ -z "$LIB_NAME" ]; then
  if [ -f "$TARGET_DIR/librdpj.dylib" ]; then
    LIB_NAME="librdpj.dylib"
  else
    LIB_NAME="librdpj.so"
  fi
fi
MAIN_LIB="$TARGET_DIR/$LIB_NAME"
if [ ! -f "$MAIN_LIB" ]; then
  echo "main library not found: $MAIN_LIB" >&2
  exit 1
fi

# glibc / dynamic loader are provided by the host and must never be bundled.
is_system_lib() {
  case "$(basename "$1")" in
    libc.so.*|libm.so.*|libpthread.so.*|libdl.so.*|librt.so.*|libresolv.so.*| \
    libnsl.so.*|libutil.so.*|ld-linux*|linux-vdso*|libc.musl*|ld-musl*)
      return 0 ;;
    *) return 1 ;;
  esac
}

bundle_linux() {
  command -v patchelf >/dev/null 2>&1 || {
    echo "patchelf is required to set \$ORIGIN rpath" >&2
    exit 1
  }

  local queue=("$MAIN_LIB")
  local copied=0
  while [ "${#queue[@]}" -gt 0 ]; do
    local current="${queue[0]}"
    queue=("${queue[@]:1}")
    while IFS= read -r dep; do
      [ -n "$dep" ] || continue
      [ -f "$dep" ] || continue
      local base
      base="$(basename "$dep")"
      if is_system_lib "$dep"; then
        continue
      fi
      if [ -f "$TARGET_DIR/$base" ]; then
        continue
      fi
      cp -L "$dep" "$TARGET_DIR/$base"
      chmod +x "$TARGET_DIR/$base"
      queue+=("$TARGET_DIR/$base")
      copied=$((copied + 1))
      echo "  bundled $base"
    done < <(ldd "$current" 2>/dev/null \
      | awk '/=> \//{print $3} /^\//{print $1}' \
      | grep -v '^[[:space:]]*$' || true)
  done

  # All bundled libraries resolve their siblings from the extraction directory.
  local f
  for f in "$TARGET_DIR"/*.so*; do
    [ -f "$f" ] || continue
    patchelf --set-rpath '$ORIGIN' "$f"
  done

  (cd "$TARGET_DIR" && ls -1 *.so* | LC_ALL=C sort > rdpj.index)
  echo "linux: bundled $copied dependencies, index:"
  sed 's/^/  /' "$TARGET_DIR/rdpj.index"
}

bundle_darwin() {
  command -v dylibbundler >/dev/null 2>&1 || {
    echo "dylibbundler is required (brew install dylibbundler)" >&2
    exit 1
  }
  # dylibbundler copies non-system dylibs and rewrites install names to
  # @loader_path/<name>, so the extraction directory is self-contained.
  dylibbundler -od -b -x "$MAIN_LIB" -d "$TARGET_DIR" -p "@loader_path/" >/dev/null
  (cd "$TARGET_DIR" && ls -1 *.dylib | LC_ALL=C sort > rdpj.index)
  echo "darwin: index:"
  sed 's/^/  /' "$TARGET_DIR/rdpj.index"
}

case "$LIB_NAME" in
  *.so) bundle_linux ;;
  *.dylib) bundle_darwin ;;
  *) echo "unsupported library: $LIB_NAME" >&2; exit 1 ;;
esac
