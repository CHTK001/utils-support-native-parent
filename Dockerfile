FROM eclipse-temurin:25-jre
WORKDIR /app

# 1. 复制 gateway jar + 全部 runtime 依赖
COPY gateway-server.jar /app/
COPY dependencies/ /app/dependencies/

# 2. 构造 classpath（绝对路径，运行时变量）
ENV GATEWAY_CP="/app/gateway-server.jar:/app/dependencies/*"
ENV JAVA_OPTS="-XX:+UseContainerSupport -XX:MaxRAMPercentage=75 -Dfile.encoding=UTF-8"

# 3. 健康检查（由 Docker 外部 healthcheck 命令执行）
HEALTHCHECK --interval=30s --timeout=3s CMD wget -q -O- http://127.0.0.1:8090/api/connections/list > /dev/null 2>&1 || exit 1

# 4. 启动
#    ${GATEWAY_CP} 含 * 通配符，java 直接接受
#    gateway main class 会自动：
#      a. 检测 guacd 是否已就绪（172.16.0.40 host:4822 via Docker DNS，或容器内自启）
#      b. 启动 HTTP server :8090 + WS bridge :8182
#      c. 启动失败时降级到 SSH-only 模式
ENTRYPOINT ["sh", "-c", "exec java $JAVA_OPTS -cp \"$GATEWAY_CP\" com.chua.gateway.server.GatewayServerApplication"]

# 暴露端口
EXPOSE 8090 8182 4822
