"""
Use jshell to probe network
"""
import urllib.request, json, time, sys
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

# jshell script
js = '''
System.out.println("java.version=" + System.getProperty("java.version"));
try {
    java.net.InetAddress[] addrs = java.net.InetAddress.getAllByName("172.16.0.40");
    for (var a : addrs) System.out.println("addr=" + a.getHostAddress() + " class=" + a.getClass().getSimpleName());
} catch (Exception e) { System.out.println("DNS_FAIL=" + e); }
try (java.net.Socket s = new java.net.Socket()) {
    s.connect(new java.net.InetSocketAddress("172.16.0.40", 22), 5000);
    System.out.println("TCP_OK=" + s.getRemoteSocketAddress());
} catch (Exception e) {
    System.out.println("TCP_FAIL=" + e);
}
try (java.net.Socket s = new java.net.Socket()) {
    s.connect(new java.net.InetSocketAddress("127.0.0.1", 22), 5000);
    System.out.println("LOCAL_TCP_OK=" + s.getRemoteSocketAddress());
} catch (Exception e) {
    System.out.println("LOCAL_TCP_FAIL=" + e);
}
try (java.net.Socket s = new java.net.Socket()) {
    s.connect(new java.net.InetSocketAddress("172.18.0.1", 22), 5000);
    System.out.println("GW_TCP_OK=" + s.getRemoteSocketAddress());
} catch (Exception e) {
    System.out.println("GW_TCP_FAIL=" + e);
}
/exit
'''

# Write jshell script to /tmp
import io, tarfile
buf = io.BytesIO()
with tarfile.open(fileobj=buf, mode='w') as tar:
    data = js.encode('utf-8')
    info = tarfile.TarInfo(name='probe.jsh')
    info.size = len(data)
    tar.addfile(info, io.BytesIO(data))
tar_bytes = buf.getvalue()

req = urllib.request.Request(f'{URL}/containers/gateway-server/archive?path=/tmp',
    data=tar_bytes, method='PUT')
req.add_header('Content-Type', 'application/x-tar')
r = urllib.request.urlopen(req)
print("Upload:", r.status)

# Run jshell
body = json.dumps({"Cmd": ["sh", "-c", "/opt/java/openjdk/bin/jshell /tmp/probe.jsh > /tmp/p.log 2>&1; echo done"]}).encode('utf-8')
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
    data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(
    f'{URL}/exec/{eid}/start', data=b'{}',
    headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(20)

# Read result
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
size_str = data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8')
size = int(size_str, 8) if size_str else 0
print("=== jshell output ===")
print(data[512:512+size].decode('utf-8', errors='replace'))
