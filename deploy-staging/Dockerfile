FROM eclipse-temurin:25-jre

# Workspace
WORKDIR /app

# 1. 复制 gateway jar（mvn install 产物）
COPY gateway-server.jar /app/

# 2. 复制全部 runtime 依赖
COPY dependencies/ /app/dependencies/

# 3. 构造 CLASSPATH - gateway-server.jar 在前（Main-Class 优先），deps 在后
ENV GATEWAY_CP="/app/gateway-server.jar:/app/dependencies/*"

# 4. Java 调优（容器感知）
ENV JAVA_OPTS="-XX:+UseContainerSupport -XX:MaxRAMPercentage=75 -Dfile.encoding=UTF-8"

# 5. 健康检查（gateway 自带 /api/connections/list）
HEALTHCHECK --interval=30s --timeout=3s --retries=3 \
  CMD java -cp $GATEWAY_CP -DskipHealthCheck=true HealthCheck || exit 1

# 6. 暴露端口
EXPOSE 8090 8182 4822

# 7. 入口
#    gateway main class 会：
#      a. 检测 guacd 是否已就绪（-Dguacd.host / -Dguacd.port）
#      b. 启动 HTTP server :8090 + WS 桥接 :8182
#      c. guacd 找不到时降级到纯 SSH/WS 模式
ENTRYPOINT ["/bin/sh", "-c", "exec /opt/java/openjdk/bin/java $JAVA_OPTS -cp $GATEWAY_CP -Dguacd.host=$GATEWAY_GUACD_HOST -Dguacd.port=$GATEWAY_GUACD_PORT com.chua.gateway.server.GatewayServerApplication"]
