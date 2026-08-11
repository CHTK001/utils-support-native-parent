# gateway-server-starter 启动说明（本地验证用）

## 前置

- JDK 25+（Amazon Corretto 25.0.3 验证 OK）
- Maven 3.9+
- 前端 Node.js 18+ + pnpm

确保以下文件在 `D:\ch\project\` 根目录：

- `cp.txt` — runtime classpath（classpath 全 jar 路径，`;` 分隔）
- `gateway.argfile` — 启动 argfile，含 `-cp <classpath> com.chua.gateway.server.GatewayServerApplication`
- `run-gateway.bat` — 一键启动脚本（Win）

## 端口说明

| 端口 | 用途 |
|---|---|
| **8090** | HTTP REST API（`/api/connections/...`） |
| **8182** | WebSocket 桥接（`/ws/{protocol}/{tunnelId}`） |
| 4822 | guacd 子进程（RDP/VNC 转发用，按需启动） |

## 方式一：一键启动脚本（推荐）

```cmd
D:\ch\project\run-gateway.bat
```

stdout/stderr 输出到 `D:\ch\project\server.log`。

期望输出（看到这些说明启动成功）：

```
[gateway-server] WebSocket 桥接服务启动: port=8182
[gateway-server] Gateway 服务端启动完成: port=8090 type=http
[gateway-server] 路由已注册: count=3
```

## 方式二：手动启动

```powershell
cd D:\ch\project
java @D:\ch\project\gateway.argfile
```

## 验证 REST 端点

```powershell
# 获取预配置 keys
curl http://127.0.0.1:8090/api/connections/keys

# 列出可用协议
curl http://127.0.0.1:8090/api/connections/list

# custom 模式鉴权（创建 VNC 隧道）
curl -X POST http://127.0.0.1:8090/api/connections/authenticate `
  -H "Content-Type: application/json" `
  -d '{"mode":"custom","protocol":"vnc","host":"127.0.0.1","port":5900}'

# 期望响应（wrapped {status:0, data:{...}}）
# {"status":0,"data":{"tunnelId":"xxx","wsUrl":"/ws/vnc/xxx","protocol":"vnc","host":"127.0.0.1","port":5900}}
```

## 验证 WebSocket

`wsUrl` 已自动切到独立 8182 端口：

```powershell
# 直连测试（不经过 vite proxy）
wscat -c ws://127.0.0.1:8182/ws/vnc/<tunnelId>

# 通过 vite proxy 转发（开发用）
wscat -c ws://localhost:7788/ws/vnc/<tunnelId>
```

首条 text 帧必须为 JSON bind 消息：
```json
{"action":"bind","tunnelId":"<tunnelId>"}
```
后续 binary/text 帧会被转发到对应协议桥接器。

## 前端启动

```bash
cd D:\ch\project\vue-support-parent-starter
pnpm install
pnpm dev --filter vue-support-gateway-starter
# 访问 http://localhost:7788/#/remote
```

前端 vite proxy：
- `/api/*` → `http://127.0.0.1:8090`
- `/ws/*` → `ws://127.0.0.1:8182`

## 排错

| 现象 | 原因 | 解决 |
|---|---|---|
| `ClassNotFoundException: org.slf4j.LoggerFactory` | cp.txt 没 slf4j jar | 重新跑 `mvn dependency:build-classpath` |
| `BindException: Address already in use: 8080/8182` | 端口被占 | `taskkill /F /PID <pid>` 或修改 `gateway.http.port`/`gateway.ws.port` |
| sqlite 表创建失败 | `~/.utils-support-gateway/` 目录无写权限 | `mkdir -p ~/.utils-support-gateway && chmod 755` |
| 前端 WS 连不上 | vite proxy 转发 8182 失败 | 确认 gateway server 已启动，curl `ws://127.0.0.1:8182` 通 |

