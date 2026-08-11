"""
Rebuild Docker image with new jar, restart container
"""
import urllib.request, json, time, sys, io, base64
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

# 1. Read local gateway-server.jar
with open(r"D:\ch\project\gateway-server.jar", "rb") as f:
    jar_bytes = f.read()
print(f"JAR size: {len(jar_bytes)}")

# 2. Create new image from this jar
# Easier: use docker buildx or just push to existing image
# Use: docker import a tar that has /app/gateway-server.jar

# Create build context tar
build_tar = io.BytesIO()
import tarfile
with tarfile.open(fileobj=build_tar, mode='w') as tar:
    # Dockerfile
    dockerfile = '''
FROM eclipse-temurin:25-jre
WORKDIR /app
COPY gateway-server.jar /app/gateway-server.jar
EXPOSE 8080 8182
ENV GATEWAY_CP=/app/gateway-server.jar
ENV JAVA_OPTS=-XX:+UseContainerSupport -XX:MaxRAMPercentage=75 -Dfile.encoding=UTF-8
ENV GATEWAY_GUACD_HOST=guacd
ENV GATEWAY_GUACD_PORT=4822
ENTRYPOINT ["/bin/sh","-c","exec /opt/java/openjdk/bin/java $JAVA_OPTS -cp $GATEWAY_CP -Dguacd.host=$GATEWAY_GUACD_HOST -Dguacd.port=$GATEWAY_GUACD_PORT com.chua.gateway.server.GatewayServerApplication"]
'''
    data = dockerfile.encode('utf-8')
    info = tarfile.TarInfo(name='Dockerfile')
    info.size = len(data)
    tar.addfile(info, io.BytesIO(data))
    # JAR
    info = tarfile.TarInfo(name='gateway-server.jar')
    info.size = len(jar_bytes)
    tar.addfile(info, io.BytesIO(jar_bytes))

build_tar_bytes = build_tar.getvalue()

# Build image
print("Building image...")
req = urllib.request.Request(f'{URL}/build?t=gateway-server:v2&dockerfile=Dockerfile&rm=true&forcerm=true',
    data=build_tar_bytes,
    headers={'Content-Type': 'application/x-tar'},
    method='POST')
try:
    r = urllib.request.urlopen(req, timeout=300)
    out = r.read().decode('utf-8', errors='replace')
    print("Build stream (last 500):", out[-500:])
except Exception as e:
    print("Build err:", e)
    if hasattr(e, 'read'):
        print("Body:", e.read().decode('utf-8', errors='replace')[:1000])
