import com.chua.common.support.nativehttp.server.ShmHttpServer;
import com.chua.common.support.network.http.HttpMethod;
import com.chua.common.support.network.server.ServerSetting;

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.Callable;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;

/**
 * ShmHttpServer 全功能测试：
 *  1. GET 简单响应
 *  2. POST body echo
 *  3. 查询参数
 *  4. 未注册路由 404
 *  5. 并发 100 请求
 *  6. 服务重启（start→stop→start）
 */
public class ShmHttpFunctionTest {

    private static int failures = 0;

    private static void check(boolean ok, String name, String detail) {
        if (ok) {
            System.out.println("[PASS] " + name);
        } else {
            System.out.println("[FAIL] " + name + "  <- " + detail);
            failures++;
        }
    }

    public static void main(String[] args) throws Exception {
        System.out.println("===== ShmHttpServer 全功能测试 =====");
        int port = 18081;
        ServerSetting setting = ServerSetting.builder()
                .host("0.0.0.0").port(port).protocol("http").auto(false).build();
        ShmHttpServer server = new ShmHttpServer(setting, 1024, 16 * 1024);

        server.registerMapping("/hello", (req, res) ->
                res.setContentType("text/plain; charset=utf-8").setBody("world").end());
        server.registerMapping("/echo", HttpMethod.POST, (req, res) ->
                res.setContentType("application/json")
                        .setBody("{\"echo\":\"" + req.getBodyString() + "\"}").end());
        server.registerMapping("/greet", (req, res) ->
                res.setContentType("application/json")
                        .setBody("{\"name\":\"" + req.getParam("name") + "\"}").end());

        HttpClient client = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(5)).build();
        String base = "http://127.0.0.1:" + port;

        // --- 生命周期 1 ---
        server.start();
        System.out.println("[test] server #1 started");

        // 1. GET
        HttpResponse<String> hello = client.send(
                HttpRequest.newBuilder(URI.create(base + "/hello")).GET().build(),
                HttpResponse.BodyHandlers.ofString());
        check(hello.statusCode() == 200 && "world".equals(hello.body()),
                "GET /hello", hello.statusCode() + " " + hello.body());

        // 2. POST body
        HttpResponse<String> echo = client.send(
                HttpRequest.newBuilder(URI.create(base + "/echo"))
                        .header("Content-Type", "text/plain")
                        .POST(HttpRequest.BodyPublishers.ofString("ping-pong")).build(),
                HttpResponse.BodyHandlers.ofString());
        check(echo.statusCode() == 200 && "{\"echo\":\"ping-pong\"}".equals(echo.body()),
                "POST /echo body", echo.statusCode() + " " + echo.body());

        // 3. 查询参数
        HttpResponse<String> greet = client.send(
                HttpRequest.newBuilder(URI.create(base + "/greet?name=chua")).GET().build(),
                HttpResponse.BodyHandlers.ofString());
        check(greet.statusCode() == 200 && "{\"name\":\"chua\"}".equals(greet.body()),
                "GET /greet?name=chua", greet.statusCode() + " " + greet.body());

        // 4. 404
        HttpResponse<String> nf = client.send(
                HttpRequest.newBuilder(URI.create(base + "/no-such-route")).GET().build(),
                HttpResponse.BodyHandlers.ofString());
        check(nf.statusCode() == 404, "未注册路由 404", "status=" + nf.statusCode());

        // 5. 并发 100 请求
        ExecutorService pool = Executors.newFixedThreadPool(16);
        List<Callable<Boolean>> tasks = new ArrayList<>();
        for (int i = 0; i < 100; i++) {
            final int id = i;
            tasks.add(() -> {
                HttpResponse<String> r = client.send(
                        HttpRequest.newBuilder(URI.create(base + "/hello")).GET().build(),
                        HttpResponse.BodyHandlers.ofString());
                return r.statusCode() == 200 && "world".equals(r.body());
            });
        }
        long t0 = System.currentTimeMillis();
        List<Future<Boolean>> results = pool.invokeAll(tasks);
        int okCount = 0;
        for (Future<Boolean> f : results) {
            if (f.get()) okCount++;
        }
        long elapsed = System.currentTimeMillis() - t0;
        pool.shutdown();
        check(okCount == 100, "并发 100 请求全部成功 (" + elapsed + "ms)", "ok=" + okCount);

        // 6. 服务重启
        server.stop();
        System.out.println("[test] server #1 stopped");

        server.start();
        System.out.println("[test] server #2 started");
        HttpResponse<String> hello2 = client.send(
                HttpRequest.newBuilder(URI.create(base + "/hello")).GET().build(),
                HttpResponse.BodyHandlers.ofString());
        check(hello2.statusCode() == 200 && "world".equals(hello2.body()),
                "重启后服务可用", hello2.statusCode() + " " + hello2.body());
        server.stop();
        System.out.println("[test] server #2 stopped");

        // 重启后端口应已释放（能再次绑定即通过）
        ShmHttpServer server3 = new ShmHttpServer(setting, 1024, 16 * 1024);
        server3.registerMapping("/hello", (req, res) -> res.setBody("world").end());
        server3.start();
        HttpResponse<String> hello3 = client.send(
                HttpRequest.newBuilder(URI.create(base + "/hello")).GET().build(),
                HttpResponse.BodyHandlers.ofString());
        check(hello3.statusCode() == 200, "第三次启动可绑定端口", String.valueOf(hello3.statusCode()));
        server3.stop();

        System.out.println(failures == 0 ? "===== 全部通过 =====" : "===== 失败 " + failures + " 项 =====");
        System.exit(failures == 0 ? 0 : 1);
    }
}
