#!/usr/bin/env python3
"""对某一轮 CI run 出一份**四平台验收汇总**：产物一致性 + 各平台判定标记。

## 为什么要有这个脚本（而不是人肉翻日志）

「全平台验收通过」这句话必须能被任何人独立复核。而现状是：

* 判定标记散在 4~5 份 job 日志里（`PROD_ACCEPT_OK`、`TASKMGR_COMPARE_OK`、
  `TASKMGR_CPU_WINDOWED_OK`、`BATTERY_VALUE_OK`、`SYSINFORMER_SMOKE_OK`…）
* 产物一致性另有 `verify_delivered.py`
* 汇总表此前只存在于对话里，**重启就没了**

这与本模块反复踩过的「报告/产物/交付脱节」是同一类问题，所以汇总也入库。

## 用法

    python acceptance_report.py <run_id>

输出一张表：每平台 -> 冒烟/生产验收/电池/CPU 判定，以及入库产物 md5 是否
与本轮 artifact 逐字节一致。最后给出 `ACCEPTANCE_REPORT_OK` 或
`ACCEPTANCE_REPORT_FAILED`。

## 判据

* run 结论必须是 `success`，否则直接拒绝（失败的 run 不能作为验收凭据）
* 四个平台**缺任一**即失败（不能只报跑通的那几个）
* 产物必须逐字节一致（复用 verify_delivered 的判定逻辑）
* 每个平台必须出现其应有的判定标记，缺失即失败
"""
import argparse
import hashlib
import io
import json
import os
import re
import struct
import subprocess
import sys
import zipfile

DEFAULT_REPO = "CHTK001/utils-support-native-parent"
NATIVE = "utils-support-native-sysinformer/src/main/resources/native"
PLATFORMS = [
    ("windows-x86_64", "sysinformer.dll", "PE", 0x8664),
    ("linux-x86_64", "libsysinformer.so", "ELF", 62),
    ("darwin-x86_64", "libsysinformer.dylib", "MACH", 0x01000007),
    ("darwin-aarch64", "libsysinformer.dylib", "MACH", 0x0100000C),
]
# 每个平台**必须有**的判定标记
REQUIRED = {
    "windows-x86_64": ["SYSINFORMER_SMOKE_OK", "SYSINFORMER_JNA_SMOKE_OK",
                       "PROD_ACCEPT_OK", "TASKMGR_COMPARE_OK",
                       "TASKMGR_CPU_WINDOWED_OK",
                       # CPU 准确性是**阻断项**（`cpu_accuracy.py`，增量判据）。
                       # 它不在必需标记里的话，该步骤被静默跳过时汇总不会发现 ——
                       # 而「门禁被跳过却不报」正是本模块反复踩的那类问题。
                       "CPU_ACCURACY_OK"],
    "linux-x86_64": ["SYSINFORMER_SMOKE_OK", "SYSINFORMER_JNA_SMOKE_OK",
                     "PROD_ACCEPT_OK", "BATTERY_VALUE_OK"],
    "darwin-x86_64": ["SYSINFORMER_SMOKE_OK", "SYSINFORMER_JNA_SMOKE_OK",
                      "PROD_ACCEPT_OK"],
    "darwin-aarch64": ["SYSINFORMER_SMOKE_OK", "SYSINFORMER_JNA_SMOKE_OK",
                       "PROD_ACCEPT_OK"],
}


def token():
    p = subprocess.run(["git", "credential", "fill"], capture_output=True,
                       input=b"protocol=https\nhost=github.com\n\n")
    for line in p.stdout.decode("utf-8", "replace").splitlines():
        if line.startswith("password="):
            return line[len("password="):]
    sys.exit("取不到 GitHub 凭据")


TOK = token()


def api(url, raw=False):
    r = subprocess.run(
        ["curl.exe", "-s", "-L", "--max-time", "240", "--ssl-no-revoke",
         "-H", "Authorization: Bearer " + TOK,
         "-H", "Accept: application/vnd.github+json", url],
        capture_output=True)
    return r.stdout if raw else json.loads(r.stdout.decode("utf-8", "replace"))


def repo_root():
    here = os.path.dirname(os.path.abspath(__file__))
    root = os.path.dirname(os.path.dirname(here))
    if not os.path.isdir(os.path.join(root, ".git")):
        sys.exit(f"  推导出的仓库根不像 git 仓库: {root}")
    return root


def arch_of(b, kind):
    try:
        if kind == "PE" and b[:2] == b"MZ":
            o = struct.unpack_from("<I", b, 0x3C)[0]
            return struct.unpack_from("<H", b, o + 4)[0]
        if kind == "ELF" and b[:4] == b"\x7fELF":
            return struct.unpack_from("<H", b, 18)[0]
        if kind == "MACH" and b[:4] in (b"\xcf\xfa\xed\xfe", b"\xce\xfa\xed\xfe"):
            return struct.unpack_from("<I", b, 4)[0]
    except struct.error:
        return None
    return None


def job_log(job_id):
    zb = api(f"https://api.github.com/repos/{DEFAULT_REPO}/actions/jobs/"
             f"{job_id}/logs", raw=True)
    if zb[:2] != b"PK":
        return zb.decode("utf-8", "replace")
    return "\n".join(zipfile.ZipFile(io.BytesIO(zb)).read(n).decode(
        "utf-8", "replace") for n in
        zipfile.ZipFile(io.BytesIO(zb)).namelist())


def strip(s):
    s = re.sub(r"^\d{4}-\d\d-\d\dT[\d:.]+Z\s+", "", s)
    return re.sub(r"\x1b\[[0-9;]*m", "", s)


def missing_markers(plat, text):
    """返回该平台**缺失**的判定标记（空列表 = 齐全）。

    抽成独立函数是为了能自检：若这段逻辑坏掉（比如 REQUIRED 被写空、
    或判定写成恒真），`ACCEPTANCE_REPORT_OK` 就变成假绿灯。
    """
    return [k for k in REQUIRED[plat] if k not in text]


def selftest():
    """自检：证明标记判定**能发现缺失**，不是恒真。

    「四平台判定标记齐全」这句话完全依赖 `missing_markers`。若它恒返回空
    （例如 REQUIRED 被写空、或比较写成 `k in text or True` 之类），
    这个核对就永远报 OK —— 而本模块反复踩过「门禁自己坏掉却报通过」，
    所以自检的做法是**故意去掉一个标记，确认它真被发现**。
    """
    print("=== acceptance_report 自检：标记判定必须能发现缺失 ===")
    ok = True

    want = {"windows-x86_64", "linux-x86_64",
            "darwin-x86_64", "darwin-aarch64"}
    if set(REQUIRED) != want:
        print(f"  **REQUIRED 的平台集合异常: {sorted(REQUIRED)}**")
        ok = False
    else:
        print(f"  REQUIRED 覆盖四平台 OK")
    for p, keys in REQUIRED.items():
        if not keys:
            print(f"  **{p} 的必需标记为空 —— 该平台等于不判**")
            ok = False

    for plat in sorted(REQUIRED):
        keys = REQUIRED[plat]
        # ① 全标记文本 -> 不应有缺失
        full = "\n".join(f"line {k} line" for k in keys)
        miss = missing_markers(plat, full)
        if miss:
            print(f"  {plat:<16} 全标记文本却报缺失 {miss} **FAIL**")
            ok = False
        # ② 逐个去掉一个标记 -> 必须被发现
        for drop in keys:
            partial = "\n".join(f"line {k} line" for k in keys if k != drop)
            miss = missing_markers(plat, partial)
            if drop not in miss:
                print(f"  {plat:<16} 去掉 {drop} 却没被发现 **FAIL**")
                ok = False
        print(f"  {plat:<16} 全标记通过、逐个缺失均被发现  "
              f"（{len(keys)} 个标记）")

    print()
    if ok:
        print("  SELFTEST_OK —— 标记判定能发现缺失，不是恒真")
        return 0
    print("  **SELFTEST_FAILED** —— 本核对的「标记齐全」不可采信")
    return 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run_id", nargs="?")
    ap.add_argument("--repo", default=DEFAULT_REPO)
    ap.add_argument("--selftest", action="store_true",
                    help="自检标记判定能发现缺失")
    args = ap.parse_args()
    if args.selftest:
        return selftest()
    if not args.run_id:
        ap.error("需要 run_id（或 --selftest）")
    run = args.run_id
    root = repo_root()

    info = api(f"https://api.github.com/repos/{args.repo}/actions/runs/{run}")
    if info.get("conclusion") != "success":
        print(f"  run {run} 结论 = {info.get('conclusion')}，"
              f"不能作为验收凭据")
        raise SystemExit(2)
    print(f"=== 四平台验收汇总  run {run} ===")
    print(f"  sha={info.get('head_sha')}  结论=success  "
          f"完成于 {info.get('updated_at')}\n")

    jobs = api(f"https://api.github.com/repos/{args.repo}/actions/runs/{run}"
               f"/jobs?per_page=100").get("jobs", [])
    logs = {}
    for j in jobs:
        nm = j.get("name", "")
        for plat, _, _, _ in PLATFORMS:
            if nm.startswith(plat):
                logs[plat] = job_log(j["id"])

    arts = {a["name"]: a for a in api(
        f"https://api.github.com/repos/{args.repo}/actions/runs/{run}"
        f"/artifacts").get("artifacts", [])}

    fails = []
    print(f"  {'平台':<16}{'产物一致':<10}{'架构':<16}判定标记")
    for plat, fn, kind, want in PLATFORMS:
        rel = f"{NATIVE}/{plat}/{fn}"
        committed = subprocess.run(["git", "-C", root, "cat-file", "blob",
                                    f"HEAD:{rel}"],
                                   capture_output=True).stdout
        a = arts.get(f"{plat}-sysinformer")
        if not committed or not a:
            fails.append(f"{plat}: 缺产物（入库={bool(committed)} artifact={bool(a)}）")
            print(f"  {plat:<16}{'缺':<10}{'-':<16}缺产物")
            continue
        zb = api(a["archive_download_url"], raw=True)
        if len(zb) < 1000:
            fails.append(f"{plat}: artifact 下载失败（{len(zb)} 字节）")
            print(f"  {plat:<16}{'下载失败':<10}{'-':<16}")
            continue
        zf = zipfile.ZipFile(io.BytesIO(zb))
        names = [n for n in zf.namelist() if n.endswith(fn)]
        if not names:
            fails.append(f"{plat}: artifact 包内没有 {fn}")
            print(f"  {plat:<16}{'内容不符':<10}{'-':<16}")
            continue
        built = zf.read(names[0])
        same = committed == built
        if not same:
            fails.append(f"{plat}: 入库产物与本轮 artifact 不一致")
        arch = arch_of(built, kind)
        if arch != want:
            fails.append(f"{plat}: 架构 {arch} != {want}")

        log = logs.get(plat)
        marks = []
        if log is None:
            fails.append(f"{plat}: 拿不到 job 日志")
            marks = ["(无日志)"]
        else:
            text = "\n".join(strip(x) for x in log.splitlines())
            for key in REQUIRED[plat]:
                n = text.count(key)
                marks.append(f"{key}×{n}" if n else f"**缺 {key}**")
            for key in missing_markers(plat, text):
                fails.append(f"{plat}: 日志里没有 {key}")
        arch_txt = {0x8664: "x86_64(PE)", 62: "x86_64(ELF)",
                    0x01000007: "x86_64(Mach-O)",
                    0x0100000C: "arm64(Mach-O)"}.get(arch, str(arch))
        print(f"  {plat:<16}{'是' if same else '**否**':<10}{arch_txt:<16}"
              f"{' '.join(marks)}")

    print()
    if fails:
        for f in fails:
            print("  ! " + f)
        print("  ACCEPTANCE_REPORT_FAILED")
        raise SystemExit(1)
    print("  四平台：产物逐字节一致、架构正确、判定标记齐全。")
    print("  ACCEPTANCE_REPORT_OK")


if __name__ == "__main__":
    main()