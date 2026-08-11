"""
Run a Java probe inside the container to test:
1. DNS resolution of 172.16.0.40
2. TCP connect to 172.16.0.40:22
3. JSCH SSH connect
"""
import urllib.request, json, time, sys, base64
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
URL = 'http://172.16.0.40:2375'

# Write probe to /tmp via archive upload
# tar with one file probe.java
import io, tarfile
def make_tar(name, content):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode='w') as tar:
        data = content.encode('utf-8')
        info = tarfile.TarInfo(name=name)
        info.size = len(data)
        tar.addfile(info, io.BytesIO(data))
    return buf.getvalue()

tar = make_tar('Probe.java', '''
import java.net.*;
public class Probe {
    public static void main(String[] a) throws Exception {
        String h = a.length > 0 ? a[0] : "172.16.0.40";
        int p = a.length > 1 ? Integer.parseInt(a[1]) : 22;
        System.out.println("host=" + h);
        System.out.println("java.version=" + System.getProperty("java.version"));
        InetAddress[] addrs;
        try { addrs = InetAddress.getAllByName(h); } catch (Exception e) { System.out.println("DNS_FAIL: " + e); return; }
        for (InetAddress addr : addrs) {
            System.out.println("addr=" + addr.getHostAddress() + " family=" + (addr instanceof Inet6Address ? "v6" : "v4"));
        }
        try (Socket s = new Socket()) {
            s.connect(new InetSocketAddress(h, p), 5000);
            System.out.println("TCP_OK=" + s.getRemoteSocketAddress());
        } catch (Exception e) {
            System.out.println("TCP_FAIL: " + e);
        }
    }
}
''')

# Upload
req = urllib.request.Request(f'{URL}/containers/gateway-server/archive?path=/tmp',
    data=tar, method='PUT')
req.add_header('Content-Type', 'application/x-tar')
try:
    r = urllib.request.urlopen(req)
    print("Upload status:", r.status)
except Exception as e:
    print("Upload err:", e)

# Compile
body = json.dumps({"Cmd": ["sh", "-c", "javac -d /tmp /tmp/Probe.java && echo COMPILED"]}).encode('utf-8')
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
    data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(
    f'{URL}/exec/{eid}/start', data=b'{}',
    headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(4)
status = json.loads(urllib.request.urlopen(f'{URL}/exec/{eid}/json').read())
print("compile exit:", status['ExitCode'])

# Run
body = json.dumps({"Cmd": ["sh", "-c", "java -cp /tmp Probe 172.16.0.40 22 > /tmp/p.log 2>&1; echo done"]}).encode('utf-8')
req = urllib.request.Request(f'{URL}/containers/gateway-server/exec',
    data=body, headers={'Content-Type': 'application/json'}, method='POST')
r = urllib.request.urlopen(req)
eid = json.loads(r.read())['Id']
urllib.request.urlopen(urllib.request.Request(
    f'{URL}/exec/{eid}/start', data=b'{}',
    headers={'Content-Type': 'application/json'}, method='POST')).read()
time.sleep(8)
status = json.loads(urllib.request.urlopen(f'{URL}/exec/{eid}/json').read())
print("run exit:", status['ExitCode'])

# Read result
r = urllib.request.urlopen(f'{URL}/containers/gateway-server/archive?path=/tmp/p.log', timeout=10)
data = r.read()
size_str = data[124:136].rstrip(b'\x00').rstrip(b' ').decode('utf-8')
size = int(size_str, 8) if size_str else 0
print("=== Probe result ===")
print(data[512:512+size].decode('utf-8', errors='replace'))
