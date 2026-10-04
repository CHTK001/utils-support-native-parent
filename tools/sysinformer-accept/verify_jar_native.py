#!/usr/bin/env python3
"""核对**交付物（jar）里装的原生库**是否就是入库且已验收的那四份。

## 为什么需要这一步

此前所有核对都停在 `src/main/resources/native/` 这一层：
`verify_delivered.py` 比的是「仓库里的文件 == CI 编出来的」。
但**调用方拿到的是 Maven 构件（jar）**，不是仓库里的文件。

`mvn compile` 通过 **不等于** `mvn package` 打出来的 jar 里装对了东西 ——
资源过滤、打包排除、`<resources>` 配置写错，都会让 jar 里少一个平台的库
或装成旧的那份，而**编译、单测、冒烟全都发现不了**（它们读的是
`target/classes` 或源码目录）。

这与本模块反复踩过的「报告/产物/交付脱节」是同一类，只是最后一段：
**交付物本身没被核对过**。

## 判据

对四个平台逐一：

    md5(jar 内 native/<平台>/<库>)  ==  md5(git HEAD:<仓库路径>)

并确认绑定类在 jar 里。任一不符即失败 —— 因为那意味着**交付出去的 jar
里的库不是被验收过的那一份**。

## 用法

    python verify_jar_native.py [jar 路径]

不带参数时用模块 target 下的默认名字。
"""
import hashlib
import os
import subprocess
import sys
import zipfile

NATIVE = "utils-support-native-sysinformer/src/main/resources/native"
TARGETS = [
    ("windows-x86_64", "sysinformer.dll"),
    ("linux-x86_64", "libsysinformer.so"),
    ("darwin-x86_64", "libsysinformer.dylib"),
    ("darwin-aarch64", "libsysinformer.dylib"),
]
BINDING_CLASS = "com/chua/nativesysinformer/support/SysInformerNative.class"
DEFAULT_JAR = ("utils-support-native-sysinformer/target/"
               "utils-support-native-sysinformer-4.0.0.42.jar")


def repo_root():
    here = os.path.dirname(os.path.abspath(__file__))
    root = os.path.dirname(os.path.dirname(here))
    if not os.path.isdir(os.path.join(root, ".git")):
        sys.exit(f"  推导出的仓库根不像 git 仓库: {root}")
    return root


def main():
    root = repo_root()
    jar = sys.argv[1] if len(sys.argv) > 1 else os.path.join(root, DEFAULT_JAR)
    if not os.path.isfile(jar):
        print(f"  找不到 jar: {jar}")
        print("       先跑 `mvn -f utils-support-native-sysinformer/pom.xml "
              "-DskipTests package`")
        return 2

    z = zipfile.ZipFile(jar)
    names = z.namelist()
    print(f"  jar = {os.path.basename(jar)}  "
          f"条目 {len(names)}  大小 {os.path.getsize(jar):,} B\n")

    bad = []
    print(f"  {'平台':<16}{'jar 内':>36}{'git HEAD':>36}  一致")
    for plat, fn in TARGETS:
        entry = f"native/{plat}/{fn}"
        rel = f"{NATIVE}/{plat}/{fn}"
        if entry not in names:
            print(f"  {plat:<16} **jar 内无 {entry}**")
            bad.append(f"{plat}: jar 内缺 {entry}")
            continue
        injar = z.read(entry)
        head = subprocess.run(["git", "-C", root, "cat-file", "blob",
                               f"HEAD:{rel}"], capture_output=True).stdout
        if not head:
            print(f"  {plat:<16} **git HEAD 无 {rel}**")
            bad.append(f"{plat}: HEAD 缺 {rel}")
            continue
        a, b = hashlib.md5(injar).hexdigest(), hashlib.md5(head).hexdigest()
        same = injar == head
        if not same:
            bad.append(f"{plat}: jar {a} != HEAD {b}")
        print(f"  {plat:<16}{a}{b}  {'是' if same else '**否**'}")

    print()
    if BINDING_CLASS in names:
        print(f"  绑定类在 jar 内: {BINDING_CLASS}")
    else:
        print(f"  **jar 内缺绑定类** {BINDING_CLASS}")
        bad.append("缺绑定类")

    print()
    if bad:
        for b in bad:
            print("  ! " + b)
        print("  JAR_DELIVERY_FAILED —— 交付物里的库不是被验收过的那一份")
        return 1
    print("  交付 jar 内的四个原生库与 git HEAD **逐字节一致**，绑定类齐全。")
    print("  JAR_DELIVERY_OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())