#!/bin/bash
# ============================================================
#  Gateway Server 一键启动 (Linux)
# ============================================================
#  自动检测 guacd 缺失 / 启动 gateway server
#  用法: ./start-gateway.sh
# ============================================================

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-25-jdk}"
JAVA_EXE="$JAVA_HOME/bin/java"

if [ ! -x "$JAVA_EXE" ]; then
    # 尝试找其他 JDK
    JAVA_EXE=$(command -v java || true)
    if [ -z "$JAVA_EXE" ]; then
        echo "[ERROR] Java not found. Install JDK 25+:"
        echo "  Debian/Ubuntu: apt install openjdk-25-jdk"
        echo "  CentOS/RHEL:    yum install java-25-openjdk-devel"
        exit 1
    fi
fi

ARGFILE="$SCRIPT_DIR/gateway.argfile"
LOGFILE="$SCRIPT_DIR/server.log"

if [ ! -f "$ARGFILE" ]; then
    echo "[ERROR] argfile missing: $ARGFILE"
    echo "[ERROR] Please generate cp.txt + gateway.argfile first"
    exit 1
fi

echo "[START] Gateway Server starting..."
echo "[INFO] Java:  $JAVA_EXE"
echo "[INFO] Argfile: $ARGFILE"
echo "[INFO] Log:   $LOGFILE"
echo

# JDK 25 在某些 Linux 上需要 argfile 指向文件而不是 @
# 用 xargs 把文件内容展开成 argv
"$JAVA_EXE" $(cat "$ARGFILE") > "$LOGFILE" 2>&1 &
GATEWAY_PID=$!

echo "[INFO] Gateway started PID=$GATEWAY_PID"
echo "[INFO] Tail the log: tail -f $LOGFILE"
echo "[INFO] Stop with: kill $GATEWAY_PID"

# 等 1 秒后 attach 终端（前台不退出）
wait $GATEWAY_PID
