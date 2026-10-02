#!/usr/bin/env python3
"""核对「仓库里入库的产物」是否就是「被验收那一轮编出来的产物」。

## 为什么这道检查必须存在

本模块所有验收证据（`PROD_ACCEPT_OK` 33/33、`TASKMGR_COMPARE_OK` 12 项、
`TASKMGR_CPU_WINDOWED_OK`、电池 3 用例）都是针对 **CI 编出来的二进制**
取得的。而调用方实际拿到的是 **仓库里入库的那一份**。

这两个东西相等，不是理所当然的。2026-10-02 审计发现它们曾经**不等**：
`main` 上的四份产物停在 `10063a6`，此后三个提交改了 Rust 源码
（电池排序、Windows CPU 修复）而产物没有重新入库 —— 也就是说
「四平台入库产物已验证」这句话当时对 macOS 与 Windows 都不成立
（早期只核过 Linux 的 `.so`，两个 dylib 一次都没核过）。

这与 AGENTS.md §4.6 记的事故同类：交了全绿报告，交付的却是回退版。
编译、单测、冒烟、生产验收全都发现不了 —— 只能靠逐字节比对。

## 判据

对四个平台逐一比较：

    md5(git cat-file blob HEAD:<入库路径>)  ==  md5(<该 run 的 artifact>)

并直读 PE/ELF/Mach-O 头确认架构，避免「字节数对但架构错」被放过。

全绿时打印 `DELIVERED_VERIFIED`，任一平台不一致打印
`DELIVERED_MISMATCH` 并以退出码 1 结束。

## 用法

    python verify_delivered.py <run_id>
    python verify_delivered.py <run_id> --repo CHTK001/utils-support-native-parent

`run_id` 必须是**结论为 success** 的那一轮 —— 失败的 run 编出来的东西
没有比较价值，脚本会直接拒绝。

## 实现上刻意避免的三个坑

1. **不走 `git fetch` 取产物**。artifacts 分支会被后续 run 覆盖，从分支
   只能拿到「最后一次」的产物，无法取回历史 run 的那一份；而且本机到
   GitHub 的 git 传输不稳（实测连断 10 次），`api.github.com` 却一直正常。
   所以走 artifact API，按 run id 精确取。
2. **不用 shell 重定向解二进制**。`git show ... > file` 会把二进制写坏
   （PowerShell 的 `>` 是文本重定向）。全程 bytes 通道。
3. **不硬编码本机路径**。仓库根由本文件位置推导，仓库名可由参数覆盖。
"""
import argparse
import hashlib
import io
import json
import os
import struct
import subprocess
import sys
import zipfile

DEFAULT_GH_REPO = "CHTK001/utils-support-native-parent"

# 平台 -> (文件名, 容器格式, 期望的架构标识)
TARGETS = [
    ("windows-x86_64", "sysinformer.dll", "PE", 0x8664),
    ("linux-x86_64", "libsysinformer.so", "ELF", 62),
    ("darwin-x86_64", "libsysinformer.dylib", "MACH", 0x01000007),
    ("darwin-aarch64", "libsysinformer.dylib", "MACH", 0x0100000C),
]

NATIVE = "utils-support-native-sysinformer/src/main/resources/native"


def repo_root():
    """从本文件位置推导仓库根，不依赖调用时的当前目录。

    本文件位于 <repo>/tools/sysinformer-accept/verify_delivered.py，
    故上溯两级即仓库根。
    """
    here = os.path.dirname(os.path.abspath(__file__))
    root = os.path.dirname(os.path.dirname(here))
    if not os.path.isdir(os.path.join(root, ".git")):
        sys.exit(f"  推导出的仓库根不像 git 仓库: {root}")
    return root


def gh_token():
    """取 GitHub token。

    只从 `git credential` 读，**不在本文件里落任何凭据**，也不设默认值 ——
    没有凭据时给出明确提示，而不是拿匿名请求去撞 401。
    """
    p = subprocess.run(["git", "credential", "fill"], capture_output=True,
                       input=b"protocol=https\nhost=github.com\n\n")
    for line in p.stdout.decode("utf-8", "replace").splitlines():
        if line.startswith("password="):
            tok = line[len("password="):]
            if tok:
                return tok
    sys.exit("  取不到 GitHub 凭据。\n"
             "       本脚本只从 git credential 读，不内置凭据。\n"
             "       请先配置好 git 凭据助手里的 github 条目。")


def api(url, raw=False, token=None):
    r = subprocess.run(
        ["curl.exe", "-s", "-L", "--max-time", "180", "--ssl-no-revoke",
         "-H", "Authorization: Bearer " + token,
         "-H", "Accept: application/vnd.github+json", url],
        capture_output=True)
    if raw:
        return r.stdout
    out = r.stdout.decode("utf-8", "replace")
    try:
        return json.loads(out)
    except json.JSONDecodeError:
        sys.exit(f"  API 返回的不是 JSON（前 200 字）: {out[:200]}")


ARCH_NAME = {
    0x8664: "x86_64(PE)", 0x014C: "x86(PE)",
    62: "x86_64(ELF)", 3: "x86(ELF)", 183: "aarch64(ELF)",
    0x01000007: "x86_64(Mach-O)", 0x0100000C: "arm64(Mach-O)",
}


def arch_text(v):
    if v is None:
        return "读不出"
    return f"{v:#x} {ARCH_NAME.get(v, '?')}"


def arch_of(buf, kind):
    """从容器头直读架构标识，读不出返回 None。"""
    try:
        if kind == "PE" and buf[:2] == b"MZ":
            off = struct.unpack_from("<I", buf, 0x3C)[0]
            return struct.unpack_from("<H", buf, off + 4)[0]
        if kind == "ELF" and buf[:4] == b"\x7fELF":
            return struct.unpack_from("<H", buf, 18)[0]
        if kind == "MACH" and buf[:4] in (b"\xcf\xfa\xed\xfe",
                                           b"\xce\xfa\xed\xfe"):
            return struct.unpack_from("<I", buf, 4)[0]
    except struct.error:
        return None
    return None


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[1])
    ap.add_argument("run_id", help="CI run id，结论必须是 success")
    ap.add_argument("--repo", default=DEFAULT_GH_REPO, help="owner/name")
    args = ap.parse_args()

    root = repo_root()
    token = gh_token()
    print(f"=== 交付核对：run {args.run_id}  ({args.repo})\n")

    info = api(f"https://api.github.com/repos/{args.repo}"
               f"/actions/runs/{args.run_id}", token=token)
    if info.get("conclusion") != "success":
        # 这一条是硬前置：失败的 run 编出来的产物没有比较价值
        print(f"  该 run 结论为 {info.get('conclusion')}，"
              f"不能作为验收凭据")
        raise SystemExit(2)
    print(f"  status={info.get('status')}  conclusion={info.get('conclusion')}")
    print(f"  sha={info.get('head_sha')}\n")

    arts = api(f"https://api.github.com/repos/{args.repo}/actions/runs/"
               f"{args.run_id}/artifacts", token=token)
    byname = {a["name"]: a for a in arts.get("artifacts", [])}
    print(f"  该 run 产物 {len(byname)} 个")
    missing = [p for p, _, _, _ in TARGETS if f"{p}-sysinformer" not in byname]
    if missing:
        print("  缺以下平台的产物，四个平台缺任一都不能宣称已核对：")
        for m in missing:
            print("    " + m)
        print(f"  实际有: {sorted(byname)}")
        raise SystemExit(2)
    print()

    fails = []
    for plat, fn, kind, want_arch in TARGETS:
        rel = f"{NATIVE}/{plat}/{fn}"
        committed = subprocess.run(["git", "-C", root, "cat-file", "blob",
                                    f"HEAD:{rel}"],
                                   capture_output=True).stdout
        if not committed:
            print(f"  {plat:<16} 仓库里没有 {rel}")
            fails.append(f"{plat}: 仓库缺产物")
            continue

        zb = api(byname[f"{plat}-sysinformer"]["archive_download_url"],
                 raw=True, token=token)
        if len(zb) < 1000:
            print(f"  {plat:<16} 产物下载失败（收到 {len(zb)} 字节）")
            fails.append(f"{plat}: 产物下载失败")
            continue
        zf = zipfile.ZipFile(io.BytesIO(zb))
        names = [n for n in zf.namelist() if n.endswith(fn)]
        if not names:
            print(f"  {plat:<16} artifact 包内没有 {fn}")
            fails.append(f"{plat}: artifact 内容不符")
            continue
        built = zf.read(names[0])

        m1 = hashlib.md5(committed).hexdigest()
        m2 = hashlib.md5(built).hexdigest()
        same = committed == built
        a1, a2 = arch_of(committed, kind), arch_of(built, kind)
        arch_ok = a1 == want_arch and a2 == want_arch

        flag = "是" if same else "**否**"
        print(f"  {plat}")
        print(f"    入库(HEAD)  {m1}  {len(committed):>10,} 字节  "
              f"架构 {arch_text(a1)}")
        print(f"    本轮编出    {m2}  {len(built):>10,} 字节  "
              f"架构 {arch_text(a2)}")
        print(f"    逐字节一致 = {flag}    架构符合预期 = "
              f"{'是' if arch_ok else '**否**'}")
        if not same:
            fails.append(f"{plat}: 入库 {m1} != 本轮 {m2}")
        if not arch_ok:
            fails.append(f"{plat}: 架构异常 入库={a1} 本轮={a2} 期望={want_arch}")

    print()
    if fails:
        for f in fails:
            print("  ! " + f)
        print("  DELIVERED_MISMATCH —— 交付的产物与被验收的不是同一份")
        raise SystemExit(1)
    print("  四平台入库产物与该 run 编出的**逐字节相同**。")
    print("  验收证据与交付物是同一个二进制 —— 闭环成立。")
    print("  DELIVERED_VERIFIED")


if __name__ == "__main__":
    main()