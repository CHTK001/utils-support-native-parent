#!/bin/sh
# 本仓提交前门禁。
#
# 1) 暂存的 *.java 必须是合法 UTF-8（既有行为，原样保留）
# 2) workflow 四坑校验（tools/ci-lint/check_workflows.py）
#
# 安装：sh tools/ci-lint/install-hook.sh
# 手动跑：python3 tools/ci-lint/check_workflows.py
set -u

status=0

# ---- 1) Java 源文件必须是合法 UTF-8 ----
tmp=".git/utf8-check-list"
git diff --cached --name-only --diff-filter=ACM -- '*.java' > "$tmp" 2>/dev/null
while IFS= read -r f; do
  [ -n "$f" ] || continue
  [ -f "$f" ] || continue
  if ! iconv -f UTF-8 -t UTF-8 "$f" >/dev/null 2>&1; then
    echo ""
    echo "[pre-commit] 以下 Java 文件不是合法 UTF-8，请转为 UTF-8 无 BOM 后重新提交："
    echo "             $f"
    status=1
  fi
done < "$tmp"
rm -f "$tmp"

# ---- 2) workflow 四坑（仅在本次提交涉及 workflow 时执行）----
wf_staged=$(git diff --cached --name-only --diff-filter=ACM -- '.github/workflows/' 2>/dev/null)
if [ -n "$wf_staged" ]; then
  # 探测「真的能跑」的解释器：Windows 上 python3 常是 Microsoft Store 存根，
  # command -v 能找到但一运行就报 "Python was not found" 并返回非 0。
  # 只看 command -v 会选中存根，导致 workflow 干净时也被拦（假红）。
  py=""
  for c in python3 python py; do
    if command -v "$c" >/dev/null 2>&1 && "$c" -c "import sys" >/dev/null 2>&1; then
      py="$c"
      break
    fi
  done
  if [ -n "$py" ]; then
    "$py" tools/ci-lint/check_workflows.py || status=1
  else
    echo "[pre-commit][ci-lint] 未找到可用的 python，跳过 workflow 校验（CI 侧仍会跑）" >&2
  fi
fi

exit $status
