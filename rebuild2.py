"""
Simpler: just update existing image. Docker commit from running container
Or: stop, remove, create new with the new image after `docker load`
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

# Approach 1: just exec into running container, copy new jar, restart
# But we changed the JAR which needs restart
# Approach 2: stop current, create new from a tiny "java only" image + new jar

# Easiest: use `docker import` to create a new image from the gateway-server.jar
# That's not great either.

# Best: use existing image, just kill the container and re-create
# But the image is built from Dockerfile. Without Docker build, we need a different approach.

# Plan: 
# 1. Stop the gateway-server container
# 2. Remove it
# 3. Use docker cp (via API) to copy new jar in
# 4. Restart

# Actually simpler: use exec to copy jar via dd /dev/stdin or just wget
# Or: push the new jar to a host dir, then bind-mount

# Let's just exec the wget from a known public file server... no, the new jar is local

# Try: use Docker `cp` API to copy file into running container
# But this requires the file to be on the docker host

# Last resort: just use the existing container, but copy via base64 + exec

import base64
with open(r"D:\ch\project\gateway-server.jar", "rb") as f:
    jar_bytes = f.read()
b64 = base64.b64encode(jar_bytes).decode('ascii')
print(f"JAR b64 size: {len(b64)}")

# Write b64 chunks to /tmp in container, then base64 -d
# But this is going to be very slow (26MB)
# Let's see if there's a faster way

# Try: docker image save/load via tar
# Simpler: just create a minimal Dockerfile+jar tar and use /build
import io, tarfile
build_tar = io.BytesIO()
with tarfile.open(fileobj=build_tar, mode='w') as tar:
    dockerfile = b'FROM eclipse-temurin:25-jre\nWORKDIR /app\nCOPY gateway-server.jar /app/gateway-server.jar\nEXPOSE 8080 8182\n'
    info = tarfile.TarInfo(name='Dockerfile')
    info.size = len(dockerfile)
    tar.addfile(info, io.BytesIO(dockerfile))
    info = tarfile.TarInfo(name='gateway-server.jar')
    info.size = len(jar_bytes)
    tar.addfile(info, io.BytesIO(jar_bytes))
build_tar_bytes = build_tar.getvalue()
print(f"build_tar size: {len(build_tar_bytes)}")

# Try build with longer timeout
import socket
socket.setdefaulttimeout(600)
req = urllib.request.Request(f'{URL}/build?t=gateway-server:v3&dockerfile=Dockerfile&rm=true&forcerm=true&nocache=false',
    data=build_tar_bytes,
    headers={'Content-Type': 'application/x-tar'},
    method='POST')
try:
    r = urllib.request.urlopen(req, timeout=600)
    out = r.read().decode('utf-8', errors='replace')
    print(f"Build OK, last 500: {out[-500:]}")
except Exception as e:
    print(f"Build err: {e}")
    try:
        print("body:", e.read().decode('utf-8', errors='replace')[:2000])
    except: pass
