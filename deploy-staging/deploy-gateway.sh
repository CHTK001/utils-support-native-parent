#!/bin/sh
# ============================================================
#  Gateway Server 一键升级启动 (Linux + Docker)
# ============================================================
#  流程：
#    1. docker build -t gateway-server:latest .
#    2. docker stop gateway-server (if running)
#    3. docker rm gateway-server (if exists)
#    4. docker run -d --name gateway-server ...
#    5. docker logs gateway-server -f
# ============================================================
set -e

cd "$(dirname "$0")"

echo "[deploy] Step 1/4: Build gateway-server image"
docker build -t gateway-server:latest . 2>&1 | tail -20

echo "[deploy] Step 2/4: Stop existing container (if any)"
docker stop gateway-server 2>/dev/null || true
sleep 2

echo "[deploy] Step 3/4: Remove existing container (if any)"
docker rm -f gateway-server 2>/dev/null || true

echo "[deploy] Step 4/4: Run new container"
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

echo "[deploy] Waiting 10s for gateway to be ready..."
sleep 10

echo "[deploy] Container status:"
docker ps --filter "name=gateway-server" --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"

echo "[deploy] Recent logs:"
docker logs gateway-server --tail 30

echo "[deploy] Health check:"
curl -s -o /dev/null -w "HTTP %{http_code}\n" http://127.0.0.1:18090/api/connections/list || true
echo
echo "[deploy] Done. gateway-server 已运行于 http://172.16.0.40:18090 (host)"
echo "[deploy] 跟踪日志: docker logs -f gateway-server"
