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

# **两个交付物**都要查。
#
# java8 模块用 `<resource><directory>../utils-support-native-sysinformer/
# src/main/resources/native</directory></resource>` **复用**同一批原生库
# （避免二进制重复入库），所以它的 jar 里嵌的是同一批文件 ——
# 它是**第二个把原生库发给调用方的构件**。
# 若哪天 sysinformer 的产物更新而 java8 的打包配置失效，java8 会静默发出
# 旧库，而**编译、单测、冒烟都发现不了**（它们读 target/classes）。
JARS = [
    ("utils-support-native-sysinformer",
     "utils-support-native-sysinformer-4.0.0.42.jar",
     "com/chua/nativesysinformer/support/SysInformerNative.class"),
    ("utils-support-native-sysinformer-java8",
     "utils-support-native-sysinformer-java8-4.0.0.42.jar",
     "com/chua/nativesysinformer/java8/SysInformerJna.class"),
]
# 兼容旧调用：默认 jar 现在是"两个"，DEFAULT_JAR 保留指向第一个
DEFAULT_JAR = f"{JARS[0][0]}/target/{JARS[0][1]}"


def repo_root():
    here = os.path.dirname(os.path.abspath(__file__))
    root = os.path.dirname(os.path.dirname(here))
    if not os.path.isdir(os.path.join(root, ".git")):
        sys.exit(f"  推导出的仓库根不像 git 仓库: {root}")
    return root


def selftest(root):
    """自带敏感性对照：**证明这个核对能报错**，再相信它的「通过」。

    ## 为什么必须有

    本模块反复踩到「门禁自己坏掉却报通过」：
    * 自制 shell 检查器里 `l.split("#")[0]` 不看引号，导致
      `echo "### $(unclosed"` 被截断 -> **不配对抓不到**，敏感性对照因此失效；
    * `verify_pushed.py` 曾无论成败都返回 0。

    一个只会在一切都对时说「通过」的核对，等于没有核对。所以自检的做法是
    **故意造一个必然失败的输入**，确认它真的报错。

    ## 做法

    取真实 jar，把其中一个平台的库改 1 字节，写到临时目录，
    再对这份篡改样本跑核对 -> 必须 `JAR_DELIVERY_FAILED`。
    """
    import tempfile
    jar = os.path.join(root, DEFAULT_JAR)
    if not os.path.isfile(jar):
        print(f"  自检需要真实 jar 存在: {jar}")
        print("       先跑 `mvn -f utils-support-native-sysinformer/pom.xml "
              "-DskipTests package`")
        return 2

    print("=== verify_jar_native 自检：故意篡改 1 字节，必须被抓到 ===")
    plat, fn = TARGETS[0]
    entry = f"native/{plat}/{fn}"
    tmpdir = tempfile.mkdtemp(prefix="jarselftest-")
    tampered = os.path.join(tmpdir, "tampered.jar")
    zin = zipfile.ZipFile(jar)
    with zipfile.ZipFile(tampered, "w", zipfile.ZIP_DEFLATED) as zout:
        for n in zin.namelist():
            d = zin.read(n)
            if n == entry:
                d = bytearray(d)
                d[len(d) // 2] ^= 0xFF
                d = bytes(d)
            zout.writestr(n, d)

    r = subprocess.run(
        [sys.executable, os.path.abspath(__file__), tampered],
        capture_output=True, text=True, encoding="utf-8", errors="replace")
    caught = ("JAR_DELIVERY_FAILED" in r.stdout) and r.returncode == 1
    print(f"  篡改样本 -> 退出码 {r.returncode}（期望 1）  "
          f"报 FAILED = {'JAR_DELIVERY_FAILED' in r.stdout}")
    for l in r.stdout.splitlines():
        if l.strip().startswith("!"):
            print("    " + l.strip()[:140])

    r2 = subprocess.run(
        [sys.executable, os.path.abspath(__file__), jar],
        capture_output=True, text=True, encoding="utf-8", errors="replace")
    clean = ("JAR_DELIVERY_OK" in r2.stdout) and r2.returncode == 0
    print(f"  真实 jar  -> 退出码 {r2.returncode}（期望 0）  "
          f"报 OK = {'JAR_DELIVERY_OK' in r2.stdout}")

    try:
        for f in os.listdir(tmpdir):
            os.remove(os.path.join(tmpdir, f))
        os.rmdir(tmpdir)
    except OSError:
        pass

    print()
    if caught and clean:
        print("  SELFTEST_OK —— 篡改必被抓到、真品必通过")
        return 0
    print("  **SELFTEST_FAILED** —— 该核对的结论不可采信")
    if not caught:
        print("    （篡改样本没被抓到：核对失效）")
    if not clean:
        print("    （真实 jar 都不通过：可能是环境问题，先修环境）")
    return 1


def check_one(root, jar, binding, label):
    """核对单个 jar；返回 (fails, checked)。"""
    if not os.path.isfile(jar):
        print(f"  找不到 jar: {jar}")
        print("       先跑 `mvn -f <模块>/pom.xml -DskipTests package`")
        return [f"{label}: jar 不存在"], False

    z = zipfile.ZipFile(jar)
    names = z.namelist()
    print(f"  --- {label}")
    print(f"  jar = {os.path.basename(jar)}  "
          f"条目 {len(names)}  大小 {os.path.getsize(jar):,} B\n")

    bad = []
    print(f"  {'平台':<16}{'jar 内':>36}{'git HEAD':>36}  一致")
    for plat, fn in TARGETS:
        entry = f"native/{plat}/{fn}"
        rel = f"{NATIVE}/{plat}/{fn}"
        if entry not in names:
            print(f"  {plat:<16} **jar 内无 {entry}**")
            bad.append(f"{label}/{plat}: jar 内缺 {entry}")
            continue
        injar = z.read(entry)
        head = subprocess.run(["git", "-C", root, "cat-file", "blob",
                               f"HEAD:{rel}"], capture_output=True).stdout
        if not head:
            print(f"  {plat:<16} **git HEAD 无 {rel}**")
            bad.append(f"{label}/{plat}: HEAD 缺 {rel}")
            continue
        a, b = hashlib.md5(injar).hexdigest(), hashlib.md5(head).hexdigest()
        same = injar == head
        if not same:
            bad.append(f"{label}/{plat}: jar {a} != HEAD {b}")
        print(f"  {plat:<16}{a}{b}  {'是' if same else '**否**'}")

    print()
    if binding is None:
        # 显式指定单个 jar 时不知道它该有哪个绑定类 —— **跳过而不是误报**。
        # 首次重构时这里漏了判断，于是 `None in names` 恒 False，
        # 把「没检查」报成了「缺绑定类」；自检立刻抓到了它。
        print("  （未提供绑定类名，跳过绑定类检查）")
    elif binding in names:
        print(f"  绑定类在 jar 内: {binding}")
    else:
        print(f"  **jar 内缺绑定类** {binding}")
        bad.append(f"{label}: 缺绑定类 {binding}")
    return bad, True


def main():
    root = repo_root()
    if "--selftest" in sys.argv:
        return selftest(root)

    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if args:
        # 显式指定单个 jar 时只查它（绑定类未知则不检查）
        jars = [(args[0], None, os.path.basename(args[0]))]
    else:
        jars = [(os.path.join(root, m, "target", j), c, m)
                for m, j, c in JARS]

    allbad = []
    for jar, binding, label in jars:
        bad, _ = check_one(root, jar, binding, label)
        allbad += bad
        print()

    if allbad:
        for b in allbad:
            print("  ! " + b)
        print("  JAR_DELIVERY_FAILED —— 交付物里的库不是被验收过的那一份")
        return 1
    print(f"  {len(jars)} 个交付 jar 内的四个原生库与 git HEAD "
          f"**逐字节一致**，绑定类齐全。")
    print("  JAR_DELIVERY_OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())