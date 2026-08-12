import com.chua.common.support.network.server.ServerSetting;
import com.chua.common.support.network.server.nio.NioHttpServer;

import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.net.HttpURLConnection;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.Map;

/**
 * NioHttpServer 独立测试运行器。
 * 直接实例化（不走 SPI），覆盖核心 HTTP 能力。
 */
public class NioTest {

    static int passed = 0, failed = 0;

    public static void main(String[] args) throws Exception {
        System.out.println("===== NioHttpServer 功能测试 =====\n");

        // 01: GET /echo
        testGetEcho();
        // 02: GET /query
        testGetQueryParams();
        // 03: POST text
        testPostPlainText();
        // 04: POST JSON
        testPostJson();
        // 05: POST form
        testPostForm();
        // 06: PUT
        testPut();
        // 07: DELETE
        testDelete();
        // 08: PATCH
        testPatch();
        // 09: HEAD
        testHead();
        // 10: OPTIONS
        testOptions();
        // 11: Custom headers
        testCustomHeaders();
        // 12: Header case insensitivity
        testHeaderCaseInsensitivity();
        // 13: Status codes
        testStatusCodes();
        // 14: Keep-Alive
        testKeepAlive();
        // 15: Large body
        testLargeBody();
        // 16: Remote address
        testRemoteAddress();
        // 17: byte[] body + OutputStream
        testByteBody();
        // 18: SSE
        testSse();

        System.out.printf("%n===== 结果: %d 通过, %d 失败 =====%n", passed, failed);
        if (failed > 0) System.exit(1);
    }

    static NioHttpServer createServer() {
        ServerSetting setting = ServerSetting.builder()
                .host("127.0.0.1").port(0)
                .readTimeout(30000).build();
        return new NioHttpServer(setting);
    }

    static NioHttpServer startAndWait(NioHttpServer server) throws Exception {
        server.start();
        Thread.sleep(50); // 等 accept 循环启动
        return server;
    }

    // ─── 测试方法 ─────────────────────────────

    static void testGetEcho() throws Exception {
        System.out.println("  [01] GET /echo");
        var server = createServer();
        server.registerMapping("/echo", (req, resp) -> resp.setResult("nio-echo"));
        startAndWait(server);
        try {
            var resp = get(server, "/echo");
            assertEq(200, resp.statusCode(), "状态码");
            assertEq("nio-echo", resp.body(), "响应体");
            pass();
        } finally { server.close(); }
    }

    static void testGetQueryParams() throws Exception {
        System.out.println("  [02] GET /query?name=hello&age=18");
        var server = createServer();
        server.registerMapping("/query", (req, resp) -> {
            resp.setResult("name=" + req.getParam("name") + ",age=" + req.getParam("age"));
        });
        startAndWait(server);
        try {
            var resp = get(server, "/query?name=hello&age=18");
            assertEq(200, resp.statusCode(), "状态码");
            assertEq("name=hello,age=18", resp.body(), "响应体");
            pass();
        } finally { server.close(); }
    }

    static void testPostPlainText() throws Exception {
        System.out.println("  [03] POST /echo text");
        var server = createServer();
        server.registerMapping("/echo", (req, resp) -> resp.setResult(req.getBodyString()));
        startAndWait(server);
        try {
            var resp = post(server, "/echo", "text/plain", "hello-post");
            assertEq(200, resp.statusCode(), "状态码");
            assertEq("hello-post", resp.body(), "响应体");
            pass();
        } finally { server.close(); }
    }

    static void testPostJson() throws Exception {
        System.out.println("  [04] POST /json JSON");
        var server = createServer();
        server.registerMapping("/json", (req, resp) -> {
            String ct = req.getContentType();
            resp.setContentType("application/json");
            resp.setResult("{\"received\":" + req.getBodyString() + ",\"ct\":\"" + ct + "\"}");
        });
        startAndWait(server);
        try {
            String json = "{\"name\":\"test\",\"value\":42}";
            var resp = post(server, "/json", "application/json", json);
            assertEq(200, resp.statusCode(), "状态码");
            assertTrue(resp.body().contains("\"received\":" + json), "包含原始JSON");
            assertTrue(resp.body().contains("application/json"), "包含Content-Type");
            pass();
        } finally { server.close(); }
    }

    static void testPostForm() throws Exception {
        System.out.println("  [05] POST /form urlencoded");
        var server = createServer();
        server.registerMapping("/form", (req, resp) -> {
            Map<String, String> form = req.getFormData();
            resp.setResult("user=" + form.get("user") + ",pass=" + form.get("pass"));
        });
        startAndWait(server);
        try {
            var resp = post(server, "/form", "application/x-www-form-urlencoded", "user=admin&pass=123456");
            assertEq(200, resp.statusCode(), "状态码");
            assertEq("user=admin,pass=123456", resp.body(), "响应体");
            pass();
        } finally { server.close(); }
    }

    static void testPut() throws Exception {
        System.out.println("  [06] PUT /update");
        var server = createServer();
        server.registerMapping("/update", (req, resp) ->
                resp.setResult("method=" + req.getMethod().name() + ",body=" + req.getBodyString()));
        startAndWait(server);
        try {
            var resp = client().send(
                    HttpRequest.newBuilder(uri(server, "/update"))
                            .timeout(Duration.ofSeconds(5))
                            .PUT(HttpRequest.BodyPublishers.ofString("put-data")).build(),
                    HttpResponse.BodyHandlers.ofString());
            assertEq(200, resp.statusCode(), "状态码");
            assertEq("method=PUT,body=put-data", resp.body(), "响应体");
            pass();
        } finally { server.close(); }
    }

    static void testDelete() throws Exception {
        System.out.println("  [07] DELETE /remove");
        var server = createServer();
        server.registerMapping("/remove", (req, resp) ->
                resp.setResult("method=" + req.getMethod().name() + ",path=" + req.getPath()));
        startAndWait(server);
        try {
            var resp = client().send(
                    HttpRequest.newBuilder(uri(server, "/remove"))
                            .timeout(Duration.ofSeconds(5)).DELETE().build(),
                    HttpResponse.BodyHandlers.ofString());
            assertEq(200, resp.statusCode(), "状态码");
            assertEq("method=DELETE,path=/remove", resp.body(), "响应体");
            pass();
        } finally { server.close(); }
    }

    static void testPatch() throws Exception {
        System.out.println("  [08] PATCH /patch");
        var server = createServer();
        server.registerMapping("/patch", (req, resp) ->
                resp.setResult("method=" + req.getMethod().name() + ",body=" + req.getBodyString()));
        startAndWait(server);
        try {
            var resp = client().send(
                    HttpRequest.newBuilder(uri(server, "/patch"))
                            .timeout(Duration.ofSeconds(5))
                            .method("PATCH", HttpRequest.BodyPublishers.ofString("{\"name\":\"updated\"}"))
                            .header("Content-Type", "application/json").build(),
                    HttpResponse.BodyHandlers.ofString());
            assertEq(200, resp.statusCode(), "状态码");
            assertEq("method=PATCH,body={\"name\":\"updated\"}", resp.body(), "响应体");
            pass();
        } finally { server.close(); }
    }

    static void testHead() throws Exception {
        System.out.println("  [09] HEAD /head");
        var server = createServer();
        server.registerMapping("/head", (req, resp) -> {
            resp.setHeader("X-Head-Test", "head-value");
            resp.setResult("ignored");
        });
        startAndWait(server);
        try {
            var resp = client().send(
                    HttpRequest.newBuilder(uri(server, "/head"))
                            .timeout(Duration.ofSeconds(5))
                            .method("HEAD", HttpRequest.BodyPublishers.noBody()).build(),
                    HttpResponse.BodyHandlers.ofByteArray());
            assertEq(200, resp.statusCode(), "状态码");
            assertEq("head-value", resp.headers().firstValue("X-Head-Test").orElse(""), "X-Head-Test");
            pass();
        } finally { server.close(); }
    }

    static void testOptions() throws Exception {
        System.out.println("  [10] OPTIONS /options");
        var server = createServer();
        server.registerMapping("/options", (req, resp) -> {
            resp.setHeader("Allow", "GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS");
            resp.setStatus(204);
        });
        startAndWait(server);
        try {
            var resp = client().send(
                    HttpRequest.newBuilder(uri(server, "/options"))
                            .timeout(Duration.ofSeconds(5))
                            .method("OPTIONS", HttpRequest.BodyPublishers.noBody()).build(),
                    HttpResponse.BodyHandlers.ofString());
            assertEq(204, resp.statusCode(), "状态码");
            String allow = resp.headers().firstValue("Allow").orElse("");
            assertTrue(allow.contains("GET"), "Allow含GET");
            assertTrue(allow.contains("PATCH"), "Allow含PATCH");
            pass();
        } finally { server.close(); }
    }

    static void testCustomHeaders() throws Exception {
        System.out.println("  [11] 自定义请求头+响应头");
        var server = createServer();
        server.registerMapping("/headers", (req, resp) -> {
            String custom = req.getHeader("X-Custom-Header");
            resp.setHeader("X-Response-Id", "nio-12345");
            resp.setHeader("X-Echo", custom != null ? custom : "missing");
            resp.setResult("ok");
        });
        startAndWait(server);
        try {
            var resp = client().send(
                    HttpRequest.newBuilder(uri(server, "/headers"))
                            .timeout(Duration.ofSeconds(5))
                            .header("X-Custom-Header", "hello-nio").GET().build(),
                    HttpResponse.BodyHandlers.ofString());
            assertEq(200, resp.statusCode(), "状态码");
            assertEq("hello-nio", resp.headers().firstValue("X-Echo").orElse(""), "X-Echo");
            assertEq("nio-12345", resp.headers().firstValue("X-Response-Id").orElse(""), "X-Response-Id");
            pass();
        } finally { server.close(); }
    }

    static void testHeaderCaseInsensitivity() throws Exception {
        System.out.println("  [12] Header 大小写不敏感");
        var server = createServer();
        server.registerMapping("/h-ci", (req, resp) -> {
            String v1 = req.getHeader("X-My-Header");
            String v2 = req.getHeader("x-my-header");
            String v3 = req.getHeader("X-MY-HEADER");
            String ct = req.getHeader("content-type");
            String ctOrig = req.getContentType();
            resp.setResult("v1=" + v1 + ",v2=" + v2 + ",v3=" + v3 + ",ct=" + ct + ",ctOrig=" + ctOrig);
        });
        startAndWait(server);
        try {
            var resp = client().send(
                    HttpRequest.newBuilder(uri(server, "/h-ci"))
                            .timeout(Duration.ofSeconds(5))
                            .header("X-My-Header", "case-test")
                            .header("Content-Type", "text/plain").GET().build(),
                    HttpResponse.BodyHandlers.ofString());
            assertEq(200, resp.statusCode(), "状态码");
            assertTrue(resp.body().contains("v1=case-test"), "X-My-Header");
            assertTrue(resp.body().contains("v2=case-test"), "x-my-header");
            assertTrue(resp.body().contains("v3=case-test"), "X-MY-HEADER");
            assertTrue(resp.body().contains("ct=text/plain"), "content-type");
            assertTrue(resp.body().contains("ctOrig=text/plain"), "getContentType");
            pass();
        } finally { server.close(); }
    }

    static void testStatusCodes() throws Exception {
        System.out.println("  [13] 状态码: 201,204,302,400,404,500");
        var server = createServer();
        server.registerMapping("/s201", (req, resp) -> { resp.setStatus(201); resp.setResult("created"); });
        server.registerMapping("/s204", (req, resp) -> resp.setStatus(204));
        server.registerMapping("/s302", (req, resp) -> resp.sendRedirect("/target"));
        server.registerMapping("/s400", (req, resp) -> resp.sendError(400, "Bad Request"));
        server.registerMapping("/s404", (req, resp) -> resp.sendError(404, "Not Found"));
        server.registerMapping("/s500", (req, resp) -> resp.sendError(500, "Server Error"));
        startAndWait(server);
        try {
            assertEq(201, get(server, "/s201").statusCode(), "201");
            assertEq(204, get(server, "/s204").statusCode(), "204");
            HttpClient noRedirect = HttpClient.newBuilder().followRedirects(HttpClient.Redirect.NEVER).build();
            var r302 = noRedirect.send(
                    HttpRequest.newBuilder(uri(server, "/s302")).timeout(Duration.ofSeconds(5)).GET().build(),
                    HttpResponse.BodyHandlers.ofString());
            assertEq(302, r302.statusCode(), "302");
            assertEq("/target", r302.headers().firstValue("Location").orElse(""), "Location");
            assertEq(400, get(server, "/s400").statusCode(), "400");
            assertEq(404, get(server, "/s404").statusCode(), "404");
            assertEq(500, get(server, "/s500").statusCode(), "500");
            pass();
        } finally { server.close(); }
    }

    static void testKeepAlive() throws Exception {
        System.out.println("  [14] Keep-Alive 5次请求");
        var server = createServer();
        server.registerMapping("/ka", (req, resp) -> resp.setResult("ka-" + System.nanoTime()));
        startAndWait(server);
        try {
            HttpClient c = client();
            String prev = null;
            for (int i = 0; i < 5; i++) {
                var resp = c.send(
                        HttpRequest.newBuilder(uri(server, "/ka"))
                                .timeout(Duration.ofSeconds(5)).GET().build(),
                        HttpResponse.BodyHandlers.ofString());
                assertEq(200, resp.statusCode(), "第" + (i + 1) + "次状态码");
                assertTrue(resp.body().startsWith("ka-"), "第" + (i + 1) + "次响应");
                assertTrue(!resp.body().equals(prev), "响应独立");
                prev = resp.body();
            }
            pass();
        } finally { server.close(); }
    }

    static void testLargeBody() throws Exception {
        System.out.println("  [15] 大报文 1MB");
        int size = 1024 * 1024;
        byte[] payload = new byte[size];
        for (int i = 0; i < size; i++) payload[i] = (byte) ('A' + (i % 26));
        String payloadStr = new String(payload, StandardCharsets.UTF_8);
        var server = createServer();
        server.registerMapping("/big", (req, resp) -> resp.setResult(req.getBodyString()));
        startAndWait(server);
        try {
            var resp = post(server, "/big", "text/plain", payloadStr);
            assertEq(200, resp.statusCode(), "状态码");
            assertEq(size, resp.body().length(), "长度");
            pass();
        } finally { server.close(); }
    }

    static void testRemoteAddress() throws Exception {
        System.out.println("  [16] RemoteAddress");
        var server = createServer();
        server.registerMapping("/remote", (req, resp) ->
                resp.setResult("addr=" + req.getRemoteAddress() + ",port=" + req.getRemotePort()));
        startAndWait(server);
        try {
            var resp = get(server, "/remote");
            assertEq(200, resp.statusCode(), "状态码");
            assertTrue(resp.body().contains("addr=127.0.0.1"), "地址: " + resp.body());
            String portStr = resp.body().substring(resp.body().indexOf("port=") + 5);
            assertTrue(Integer.parseInt(portStr) > 0, "端口>0");
            pass();
        } finally { server.close(); }
    }

    static void testByteBody() throws Exception {
        System.out.println("  [17] setBody(byte[]) + getOutputStream()");
        var server = createServer();
        server.registerMapping("/bytes", (req, resp) -> {
            resp.setBody(new byte[]{0x48, 0x65, 0x6C, 0x6C, 0x6F}); // "Hello"
        });
        server.registerMapping("/stream", (req, resp) -> {
            resp.setContentType("application/octet-stream");
            try { resp.getOutputStream().write(new byte[]{0x01, 0x02, 0x03, 0x04}); }
            catch (Exception e) { resp.sendError(500, e.getMessage()); }
        });
        startAndWait(server);
        try {
            var r1 = client().send(
                    HttpRequest.newBuilder(uri(server, "/bytes")).timeout(Duration.ofSeconds(5)).GET().build(),
                    HttpResponse.BodyHandlers.ofByteArray());
            assertEq(200, r1.statusCode(), "/bytes状态码");
            assertEq(5, r1.body().length, "/bytes长度");
            assertEq('H', (char) r1.body()[0], "/bytes首字节");
            assertEq('o', (char) r1.body()[4], "/bytes尾字节");

            var r2 = client().send(
                    HttpRequest.newBuilder(uri(server, "/stream")).timeout(Duration.ofSeconds(5)).GET().build(),
                    HttpResponse.BodyHandlers.ofByteArray());
            assertEq(200, r2.statusCode(), "/stream状态码");
            assertEq(4, r2.body().length, "/stream长度");
            assertEq(1, r2.body()[0], "/stream首字节");
            assertEq(4, r2.body()[3], "/stream尾字节");
            pass();
        } finally { server.close(); }
    }

    static void testSse() throws Exception {
        System.out.println("  [18] SSE 流式推送");
        var server = createServer();
        server.registerMapping("/sse", (req, resp) -> {
            resp.sse();
            resp.sseEvent("msg", "event-1");
            resp.sseEvent("msg", "event-2");
            resp.sseEvent("msg", "event-3");
            resp.sseClose();
        });
        startAndWait(server);
        try {
            var url = new java.net.URL("http://127.0.0.1:" + server.getPort() + "/sse");
            var conn = (HttpURLConnection) url.openConnection();
            conn.setRequestMethod("GET");
            conn.setConnectTimeout(5000);
            conn.setReadTimeout(5000);
            int status = conn.getResponseCode();
            assertEq(200, status, "状态码");
            String ct = conn.getHeaderField("Content-Type");
            assertTrue(ct != null && ct.contains("text/event-stream"), "Content-Type: " + ct);
            StringBuilder sb = new StringBuilder();
            try (var reader = new BufferedReader(
                    new InputStreamReader(conn.getInputStream(), StandardCharsets.UTF_8))) {
                String line;
                while ((line = reader.readLine()) != null) sb.append(line).append('\n');
            }
            String body = sb.toString();
            assertTrue(body.contains("data: event-1"), "event-1");
            assertTrue(body.contains("data: event-2"), "event-2");
            assertTrue(body.contains("data: event-3"), "event-3");
            conn.disconnect();
            pass();
        } finally { server.close(); }
    }

    // ─── 辅助方法 ─────────────────────────────

    static HttpClient client() { return HttpClient.newHttpClient(); }

    static URI uri(NioHttpServer server, String path) {
        return URI.create("http://127.0.0.1:" + server.getPort() + path);
    }

    static HttpResponse<String> get(NioHttpServer server, String path) throws Exception {
        return client().send(
                HttpRequest.newBuilder(uri(server, path)).timeout(Duration.ofSeconds(5)).GET().build(),
                HttpResponse.BodyHandlers.ofString());
    }

    static HttpResponse<String> post(NioHttpServer server, String path, String ct, String body) throws Exception {
        return client().send(
                HttpRequest.newBuilder(uri(server, path)).timeout(Duration.ofSeconds(10))
                        .header("Content-Type", ct)
                        .POST(HttpRequest.BodyPublishers.ofString(body)).build(),
                HttpResponse.BodyHandlers.ofString());
    }

    static void assertEq(Object expected, Object actual, String msg) {
        if (expected == null ? actual != null : !expected.equals(actual))
            throw new AssertionError(msg + " — 期望 " + expected + "，实际 " + actual);
    }
    static void assertEq(int expected, int actual, String msg) {
        if (expected != actual)
            throw new AssertionError(msg + " — 期望 " + expected + "，实际 " + actual);
    }
    static void assertTrue(boolean cond, String msg) {
        if (!cond) throw new AssertionError(msg);
    }
    static void pass() { passed++; System.out.println("    \u2713 通过"); }
}
