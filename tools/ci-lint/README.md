# ci-lint — workflow 门禁

拦住「本地校验发现不了」的那几类 GitHub Actions 坑。这些坑 PyYAML / yamllint /
编辑器**全部报绿**，只有 GitHub 侧拒绝或静默绕过（详见仓库 `AGENTS.md` §5）。

## 检查项

| 码 | 判据 | 不拦会怎样 |
|---|---|---|
| C1 | 注释行里出现双花括号 | GitHub 照样解析注释 → `An expression was expected` → run 秒失败（0 job）。**只能靠文本扫**，YAML 解析器会把注释丢掉 |
| C2 | `pull_request.paths` 未包含（或未以通配覆盖）workflow 文件自身 | 只改 CI 定义的 PR 不触发任何 CI，配置未经检验就合入。判据接受 `该文件全路径` 或覆盖它的 `前缀/**` |
| C3 | `concurrency.group` 未含 `github.event_name` | push 与 `workflow_dispatch` 同组；concurrency 对**所有**触发器生效，后到的 push 会取消正在跑的 dispatch |
| C4 | `cancel-in-progress` 不是表达式 | 手动触发也会被取消，两种语义分不开 |
| C5 | `shell: pwsh` 步骤里用了 `$(pwd)` | PowerShell 的 `$(...)` 是子表达式运算符，路径被解析成目录名，Python 侧收到截断路径报 `FileNotFoundError` |

## 用法

```sh
# 手工跑（检查本仓全部 workflow）
python3 tools/ci-lint/check_workflows.py

# 只查暂存区里的 workflow（提交前门禁用）
python3 tools/ci-lint/check_workflows.py --staged

# 只查指定文件
python3 tools/ci-lint/check_workflows.py .github/workflows/native-nmap.yml
```

退出码：`0` 通过（打印 `CI_LINT_OK`）；`1` 有命中（打印 `CI_LINT_FAIL`）；`2` 用法/环境错误。

## 安装为提交前门禁

```sh
sh tools/ci-lint/install-hook.sh
```

会把 `pre-commit.sh` 装到 `.git/hooks/pre-commit`（旧钩子先备份为
`pre-commit.bak-<时间戳>`）。该钩子做两件事：暂存 `*.java` 的 UTF-8 检查（既有行为）
+ 本次提交涉及 workflow 时的四坑校验。

> `.git/hooks/` 不入版本库，所以换机器要重跑一次 `install-hook.sh`。

## 接入 CI

`.github/workflows/ci-selfcheck.yml` 在每次 workflow 变更或本目录变更时跑一次门禁。
它的 `paths` 必须覆盖 `.github/workflows/**`（含它自己）与 `tools/ci-lint/**` ——
少列任何一项就会出现「改了却不跑」的 0-job 空 run（即 C2 要防的事）。
本文件自身也受 C1~C5 约束，门禁会一并检查它。

## 设计取舍

- **只用标准库**。C1 本来就只能文本扫（解析器看不到注释），C2~C5 用「顶层块 + 缩进」
  定位即可；不引入 PyYAML，保证本机与 CI 行为一致、无需额外安装。
- 因此解析器假设这些文件保持**顶格顶层键 + 空格缩进**的写法（本仓 8 个 workflow 一致）。
  若将来出现流式写法（`{...}`）或 tab 缩进，脚本会报「找不到块」而不是静默放过。
- **误报即失败**（`C2` 在无 `pull_request` 段时会提醒），宁可让人显式确认，也不要静默绕过。

## 敏感性对照

自检时放过一个「同时含四类坑」的探针，必须报出 **5 条**（C1~C5）才算脚本有效——
只看「干净时输出 OK」无法排除脚本根本没在检查。删除探针后应回到 `CI_LINT_OK`。
