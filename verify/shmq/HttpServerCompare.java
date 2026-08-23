import com.chua.common.support.nativehttp.server.ShmHttpServer;
import com.chua.common.support.network.server.Server;
import com.chua.common.support.network.server.ServerSetting;
import com.chua.common.support.network.server.impl.JdkHttpServer;
import com.chua.common.support.network.server.nio.NioHttpServer;
import com.chua.vertx.support.server.VertxHttpServer;

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.concurrent.*;

public class HttpServerCompare {

    public static void main(String[] args) throws Exception {
        int basePort = 19000;
        run("ShmHttpServer", (setting) -> {
            ShmHttpServer s = new ShmHttpServer(setting, 4096, 16 * 1024);
            s.registerMapping("/ping", (req, res) -> res.setBody("pong").end());
            return s;
        }, basePort);

        run("JdkHttpServer", (setting) -> {
            JdkHttpServer s = new JdkHttpServer(setting);
            s.registerMapping("/ping", (req, res) -> res.setBody("pong").end());
            return s;
        }, basePort + 100);

        run("NioHttpServer", (setting) -> {
            NioHttpServer s = new NioHttpServer(setting);
            s.registerMapping("/ping", (req, res) -> res.setBody("pong").end());
            return s;
        }, basePort + 200);

        run("VertxHttpServer", (setting) -> {
            VertxHttpServer s = new VertxHttpServer(setting);
            s.registerMapping("/ping", (req, res) -> res.setBody("pong").end());
            return s;
        }, basePort + 300);
    }

    private static void run(String name, ServerFactory factory, int port) throws Exception {
        ServerSetting setting = ServerSetting.builder()
                .host("0.0.0.0").port(port).protocol("http").auto(false).build();
        Server server = factory.create(setting);
        server.start();
        System.out.println("\n===== " + name + " :" + port + " =====");

        HttpClient client = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(5))
                .version(HttpClient.Version.HTTP_1_1)
                .build();
        URI uri = URI.create("http://127.0.0.1:" + port + "/ping");
        HttpRequest req = HttpRequest.newBuilder(uri).GET().build();

        // 单请求延迟
        int L = 2000;
        long[] lat = new long[L];
        for (int i = 0; i < L; i++) {
            long t0 = System.nanoTime();
            HttpResponse<String> r = client.send(req, HttpResponse.BodyHandlers.ofString());
            if (r.statusCode() != 200) System.err.println("[warn] status=" + r.statusCode());
            lat[i] = (System.nanoTime() - t0) / 1_000;
        }
        Arrays.sort(lat);
        System.out.printf("单请求延迟(顺序)  p50=%dus p95=%dus p99=%dus%n",
                lat[L / 2], lat[(int) (L * 0.95)], lat[(int) (L * 0.99)]);

        // 并发 RPS
        for (int c : new int[]{8, 16, 32}) {
            int per = 2000;
            long total = (long) c * per;
            ExecutorService pool = Executors.newFixedThreadPool(c);
            CountDownLatch start = new CountDownLatch(1);
            CountDownLatch done = new CountDownLatch(c);
            long[] errs = new long[c];
            long t0 = System.nanoTime();
            for (int k = 0; k < c; k++) {
                final int idx = k;
                pool.submit(() -> {
                    try {
                        start.await();
                        for (int i = 0; i < per; i++) {
                            HttpResponse<String> r = client.send(req, HttpResponse.BodyHandlers.ofString());
                            if (r.statusCode() != 200) errs[idx]++;
                        }
                    } catch (Exception e) {
                        errs[idx]++;
                    } finally {
                        done.countDown();
                    }
                });
            }
            start.countDown();
            done.await(120, TimeUnit.SECONDS);
            long t1 = System.nanoTime();
            pool.shutdown();
            double secs = (t1 - t0) / 1e9;
            long err = 0;
            for (long e : errs) err += e;
            System.out.printf("并发=%-2d  %7.0f req/s  (%.3f s, %d 请求, 错误=%d)%n",
                    c, total / secs, secs, total, err);
        }

        server.stop();
        System.out.println("[done] " + name);
    }

    interface ServerFactory {
        Server create(ServerSetting setting);
    }
}