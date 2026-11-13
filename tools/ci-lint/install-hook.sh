#!/bin/sh
# 把 tools/ci-lint/pre-commit.sh 安装为 .git/hooks/pre-commit（先备份旧钩子）。
#
# 用法：sh tools/ci-lint/install-hook.sh
set -eu

root=$(git rev-parse --show-toplevel)
src="$root/tools/ci-lint/pre-commit.sh"
dst="$root/.git/hooks/pre-commit"

[ -f "$src" ] || { echo "找不到 $src" >&2; exit 1; }

if [ -f "$dst" ] && ! cmp -s "$src" "$dst"; then
    bak="$dst.bak-$(date +%Y%m%d-%H%M%S)"
    cp -p "$dst" "$bak"
    echo "  已备份原钩子 -> $bak"
fi

cp "$src" "$dst"
chmod +x "$dst"
echo "  已安装 $dst"
