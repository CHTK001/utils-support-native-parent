#!/bin/sh
# ============================================================
#  Gateway Server 一键升级启动 (Linux + Docker)
# ============================================================
#  升级模式：传入新 gateway-server.jar 自动替换 + 重新 build
#  用法：
#    ./deploy-gateway.sh                       # 用当前 gateway-server.jar build
#    ./deploy-gateway.sh /tmp/gateway-server.jar # 指定新 jar build
# ============================================================
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# 1. 处理可选 jar 参数
NEW_JAR="$1"
if [ -n "$NEW_JAR" ] && [ -f "$NEW_JAR" ]; then
    echo "[deploy] Step 0/5: Copy new jar → gateway-server.jar"
    cp -f "$NEW_JAR" gateway-server.jar
    echo "[deploy] 新 jar 大小: $(ls -la gateway-server.jar | awk '{print $5}') bytes"
fi

# 2. 检查 dependencies/ 完整性
if [ ! -d dependencies ] || [ -z "$(ls dependencies/*.jar 2>/dev/null)" ]; then
    echo "[ERROR] dependencies/ 目录为空或缺失"
    echo "[ERROR] 请确认 gateway-deploy.tar 已完整解压"
    exit 1
fi

# 3. 清理旧 container + image
echo "[deploy] Step 1/5: Stop existing container (if any)"
docker stop gateway-server 2>/dev/null || true
sleep 1

echo "[deploy] Step 2/5: Remove existing container + image"
docker rm -f gateway-server 2>/dev/null || true
docker rmi -f gateway-server:latest 2>/dev/null || true

# 4. Build 新镜像
echo "[deploy] Step 3/5: Build gateway-server:latest image"
docker build -t gateway-server:latest . 2>&1 | tail -20

# 5. Run 新容器
echo "[deploy] Step 4/5: Run new container"
docker run -d \
  --name gateway-server \
  --restart unless-stopped \
  -p 18090:8090 \
  -p 18182:8182 \
  -p 14822:4822 \
  -e JAVA_OPTS="-XX:+UseContainerSupport -XX:MaxRAMPercentage=75 -Dfile.encoding=UTF-8" \
  -e GATEWAY_HTTP_PORT=8090 \
  -e GATEWAY_WS_PORT=8182 \
  -e GATEWAY_GUACD_HOST=guacd \
  -e GATEWAY_GUACD_PORT=4822 \
  gateway-server:latest

# 6. 健康检查
echo "[deploy] Step 5/5: Wait for gateway healthy"
for i in 1 2 3 4 5 6 7 8 9 10; do
    sleep 3
    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:18090/api/connections/list || true)
    if [ "$HTTP_CODE" = "200" ]; then
        echo "[deploy] ✓ Gateway healthy (HTTP $HTTP_CODE)"
        break
    fi
    echo "[deploy] 等待启动... ($((i*3))s / 30s, HTTP=$HTTP_CODE)"
done

# 7. 报告
echo
echo "[deploy] =============================="
echo "[deploy] gateway-server 启动完成"
echo "[deploy]   HTTP API:  http://172.16.0.40:18090/api/connections/list"
echo "[deploy]   WS 桥接:  ws://172.16.0.40:18182"
echo "[deploy]   guacd 端口: 14822 (经 guacd 容器转发)"
echo "[deploy]   容器名:   gateway-server"
echo "[deploy]   镜像:     gateway-server:latest"
echo "[deploy] =============================="
echo "[deploy] 跟踪日志: docker logs -f gateway-server"
echo "[deploy] 重启: docker restart gateway-server"
echo "[deploy] 升级: $0 /path/to/new/gateway-server.jar"
