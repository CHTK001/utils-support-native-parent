#!/usr/bin/env python3
"""GitHub Actions workflow 门禁：拦住「本地校验发现不了」的那几类坑。

背景：2026-10-01 实测，下面几类问题 PyYAML / yamllint 全部通过，只有 GitHub 侧
拒绝或静默绕过（详见仓库 AGENTS.md §5）。本脚本把它们变成提交前可拦的判据。

检查项（对 .github/workflows/ 下每个 *.yml）：
  C1 注释行里出现双花括号 —— GitHub 照样解析注释并报 "An expression was expected"，
     结果是 run 秒失败（0 job）。**只能靠文本扫**：YAML 解析器会把注释丢掉。
  C2 pull_request.paths 未包含 workflow 文件自身 —— 只改 CI 定义的 PR 不触发
     任何 CI，CI 配置变更未经检验就合入。
  C3 concurrency.group 未区分事件类型 —— push 与 workflow_dispatch 同组，
     而 concurrency 对**所有**触发器生效，后到的 push 会取消正在跑的 dispatch。
  C4 cancel-in-progress 不是表达式 —— 手动触发也会被取消/被推送打断。
  C5 shell: pwsh 步骤里用了 $(pwd) —— PowerShell 的 $(...) 是子表达式运算符，
     路径会被解析成目录名，Python 侧收到截断路径报 FileNotFoundError。

只用标准库：C1 必须文本扫，C2~C5 用「顶层块 + 缩进」定位即可，
不引入 PyYAML，保证本机与 CI 行为一致。

用法：
  python3 tools/ci-lint/check_workflows.py            # 检查本仓全部 workflow
  python3 tools/ci-lint/check_workflows.py <file...>  # 只查指定文件
  python3 tools/ci-lint/check_workflows.py --staged   # 只查 git 暂存区里的 workflow

退出码：0 全部通过；1 有失败；2 用法/环境错误。
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

# 顶层键：`key:` 顶格
_TOP_RE = re.compile(r"^(?P<key>[A-Za-z_][\w-]*):\s*(?P<rest>.*)$")
# 列表项：`- 'x'` / `- x`
_ITEM_RE = re.compile(r"^\s*-\s+(?P<val>.*?)\s*$")
# 注释行
_COMMENT_RE = re.compile(r"^\s*#")


class Finding:
    """一条命中。"""

    def __init__(self, path: Path, code: str, line: int, detail: str) -> None:
        self.path = path
        self.code = code
        self.line = line
        self.detail = detail

    def __str__(self) -> str:
        return f"  [{self.code}] {self.path.name}:{self.line}  {self.detail}"


def strip_quotes(s: str) -> str:
    s = s.strip()
    if len(s) >= 2 and s[0] == s[-1] and s[0] in "'\"":
        return s[1:-1]
    return s


def path_covers(pattern: str, target: str) -> bool:
    """判断 paths 里的一条是否覆盖目标路径。

    GitHub 的 paths 支持 glob。这里只处理实际会用到的两种：
      - 完全相等
      - `前缀/**`（含裸 `**`）覆盖其下所有文件
    其余写法（段内 `*`、`?` 等）按「不覆盖」处理 —— 宁可让人改成显式写法，
    也不要在门禁里放过。
    """
    if pattern == target:
        return True
    if pattern == "**":
        return True
    if pattern.endswith("/**"):
        return target.startswith(pattern[: -len("/**")] + "/")
    return False


def top_block(lines: list[str], key: str) -> tuple[int, int] | None:
    """返回顶格键 `key:` 的块范围 [start, end)（不含键行本身）。"""
    for i, ln in enumerate(lines):
        m = _TOP_RE.match(ln)
        if m and m.group("key") == key:
            start = i + 1
            end = start
            while end < len(lines):
                cur = lines[end]
                if cur.strip() == "" or cur.startswith(" ") or cur.startswith("\t"):
                    end += 1
                else:
                    break
            return start, end
    return None


def sub_block(lines: list[str], rng: tuple[int, int], indent: int, key: str) -> tuple[int, int] | None:
    """在块内按缩进找子键 `key:`，返回其块范围。"""
    start, end = rng
    want = " " * indent + key + ":"
    for i in range(start, end):
        if lines[i].rstrip() == want or lines[i] == want:
            s = i + 1
            e = s
            while e < end and (lines[e].strip() == "" or lines[e].startswith(" " * (indent + 1))):
                e += 1
            return s, e
    return None


def list_values(lines: list[str], rng: tuple[int, int]) -> list[str]:
    """收集块内的列表项值。"""
    out = []
    start, end = rng
    for i in range(start, end):
        m = _ITEM_RE.match(lines[i])
        if m and not _COMMENT_RE.match(lines[i]):
            out.append(strip_quotes(m.group("val")))
    return out


def key_value(lines: list[str], rng: tuple[int, int], key: str) -> tuple[str, int] | None:
    """在块内找 `key: value`，返回 (去引号的值, 行号)。"""
    start, end = rng
    pat = re.compile(r"^\s*" + re.escape(key) + r":\s*(?P<v>.*?)\s*$")
    for i in range(start, end):
        m = pat.match(lines[i])
        if m:
            return strip_quotes(m.group("v")), i + 1
    return None


def check_file(path: Path) -> list[Finding]:
    """检查单个 workflow 文件。"""
    findings: list[Finding] = []
    lines = path.read_text(encoding="utf-8").splitlines()

    # C1 注释行含双花括号（唯一能发现的方式：文本扫）
    for i, ln in enumerate(lines):
        if _COMMENT_RE.match(ln) and "{{" in ln:
            findings.append(Finding(
                path, "C1", i + 1,
                "注释行含双花括号：GitHub 会解析注释并报 An expression was expected，run 秒失败"))

    # C2 pull_request.paths 含 workflow 自身
    on_rng = top_block(lines, "on")
    if on_rng is None:
        findings.append(Finding(path, "C2", 1, "找不到顶层 on: 块"))
    else:
        pr_rng = sub_block(lines, on_rng, 2, "pull_request")
        if pr_rng is None:
            findings.append(Finding(
                path, "C2", on_rng[0],
                "没有 pull_request 段：请确认是否刻意（否则 PR 不触发本 workflow）"))
        else:
            paths_rng = sub_block(lines, pr_rng, 4, "paths")
            if paths_rng is None:
                findings.append(Finding(
                    path, "C2", pr_rng[0], "pull_request 下没有 paths：PR 改动任何文件都触发，"
                                        "无法按判据核对（请显式列出）"))
            else:
                vals = list_values(lines, paths_rng)
                self_ref = f".github/workflows/{path.name}"
                if not any(path_covers(v, self_ref) for v in vals):
                    findings.append(Finding(
                        path, "C2", paths_rng[0],
                        f"pull_request.paths 未覆盖 {self_ref}：只改 CI 定义的 PR 不触发"
                        f"任何 CI（可用 `{self_ref}` 或 `.github/workflows/**`）"))

    # C3 / C4 concurrency
    conc_rng = top_block(lines, "concurrency")
    if conc_rng is None:
        # 无 concurrency 不是缺陷
        pass
    else:
        grp = key_value(lines, conc_rng, "group")
        if grp is None:
            findings.append(Finding(path, "C3", conc_rng[0], "concurrency 下没有 group"))
        elif "github.event_name" not in grp[0]:
            findings.append(Finding(
                path, "C3", grp[1],
                "concurrency.group 未含 github.event_name：push 与 workflow_dispatch 同组，"
                "后到的 push 会取消正在跑的 dispatch"))
        cip = key_value(lines, conc_rng, "cancel-in-progress")
        if cip is None:
            findings.append(Finding(
                path, "C4", conc_rng[0],
                "concurrency 下没有 cancel-in-progress（默认 false，推送不会只保留最新；"
                "若刻意如此请写明）"))
        elif "github.event_name" not in cip[0]:
            findings.append(Finding(
                path, "C4", cip[1],
                "cancel-in-progress 不是表达式：手动触发也会被取消，无法区分两种语义"))

    # C5 shell: pwsh 步骤里的 $(pwd)
    for i, ln in enumerate(lines):
        if not re.match(r"^\s*shell:\s*pwsh\s*$", ln):
            continue
        # 向后扫该步骤的块：空行或缩进 >= 8 的行仍属本步骤
        j = i + 1
        while j < len(lines):
            cur = lines[j]
            if cur.strip() == "" or cur.startswith(" " * 8):
                if not _COMMENT_RE.match(cur) and "$(pwd)" in cur:
                    findings.append(Finding(
                        path, "C5", j + 1,
                        "pwsh 步骤里用了 $(pwd)：PowerShell 的 $(...) 是子表达式，"
                        "路径会被解析成目录名（应用 Join-Path $PWD $env:X）"))
                j += 1
            else:
                break

    return findings


def repo_root() -> Path:
    """取仓库根目录。"""
    try:
        out = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True, text=True, check=True)
        return Path(out.stdout.strip())
    except Exception:
        # 回退：从本文件位置向上找 tools/
        return Path(__file__).resolve().parents[2]


def staged_workflows(root: Path) -> list[Path]:
    """取暂存区里新增/修改的 workflow 文件。"""
    try:
        out = subprocess.run(
            ["git", "diff", "--cached", "--name-only", "--diff-filter=ACM",
             "--", ".github/workflows/"],
            capture_output=True, text=True, check=True, cwd=str(root))
    except Exception as e:  # noqa: BLE001
        print(f"取暂存区失败: {e}", file=sys.stderr)
        sys.exit(2)
    return [root / p for p in out.stdout.splitlines() if p.strip().endswith((".yml", ".yaml"))]


def main() -> int:
    ap = argparse.ArgumentParser(description="GitHub Actions workflow 门禁")
    ap.add_argument("files", nargs="*", help="要检查的 workflow 文件（缺省=全部）")
    ap.add_argument("--staged", action="store_true", help="只检查 git 暂存区里的 workflow")
    args = ap.parse_args()

    root = repo_root()
    wf_dir = root / ".github" / "workflows"

    if args.files:
        targets = [Path(f) for f in args.files]
    elif args.staged:
        targets = staged_workflows(root)
    else:
        targets = sorted(wf_dir.glob("*.yml")) + sorted(wf_dir.glob("*.yaml"))

    if not targets:
        print("  没有需要检查的 workflow（跳过）")
        return 0

    all_findings: list[Finding] = []
    for p in targets:
        if not p.exists():
            print(f"  [跳过] 文件不存在：{p}")
            continue
        all_findings.extend(check_file(p))

    if all_findings:
        print(f"  CI_LINT_FAIL  发现 {len(all_findings)} 处问题：")
        for f in all_findings:
            print(str(f))
        print("  说明见 AGENTS.md §5「GitHub Actions workflow 的四处坑」。")
        return 1

    print(f"  CI_LINT_OK  已检查 {len(targets)} 个 workflow，未发现四类坑")
    return 0


if __name__ == "__main__":
    sys.exit(main())
