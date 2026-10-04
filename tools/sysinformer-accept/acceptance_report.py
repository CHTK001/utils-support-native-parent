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
                       "TASKMGR_CPU_WINDOWED_OK"],
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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run_id")
    ap.add_argument("--repo", default=DEFAULT_REPO)
    args = ap.parse_args()
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
                if n:
                    marks.append(f"{key}×{n}")
                else:
                    marks.append(f"**缺 {key}**")
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