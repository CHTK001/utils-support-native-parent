import com.chua.common.support.nativehttp.server.ShmHttpServer;
import com.chua.common.support.network.server.ServerSetting;

import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.PrintStream;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;

/**
 * ShmHttpServer 吞吐/延迟基准：
 *  1. 单连接顺序请求 → 单请求延迟（p50/p95/p99）
 *  2. 多并发压测 → RPS（8/16/32 并发，每个并发 2000 请求）
 */
public class ShmHttpBench {

    static {
        System.setOut(new PrintStream(new FileOutputStream(FileDescriptor.out), true, StandardCharsets.UTF_8));
    }

    public static void main(String[] args) throws Exception {
        int port = 18090;
        ServerSetting setting = ServerSetting.builder()
                .host("0.0.0.0").port(port).protocol("http").auto(false).build();
        ShmHttpServer server = new ShmHttpServer(setting, 4096, 16 * 1024);
        server.registerMapping("/ping", (req, res) ->
                res.setContentType("text/plain").setBody("pong").end());
        server.start();
        System.out.println("[bench] server started on :" + port);

        HttpClient client = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(5))
                .version(HttpClient.Version.HTTP_1_1)
                .build();
        URI uri = URI.create("http://127.0.0.1:" + port + "/ping");
        HttpRequest req = HttpRequest.newBuilder(uri).GET().build();

        // --- 单请求延迟 ---
        int L = 2000;
        long[] latencies = new long[L];
        for (int i = 0; i < L; i++) {
            long t0 = System.nanoTime();
            HttpResponse<String> r = client.send(req, HttpResponse.BodyHandlers.ofString());
            if (r.statusCode() != 200) {
                System.err.println("unexpected status " + r.statusCode());
                server.stop();
                System.exit(1);
            }
            latencies[i] = (System.nanoTime() - t0) / 1_000;
        }
        Arrays.sort(latencies);
        System.out.printf("[bench] 单请求延迟(顺序)  p50=%dus p95=%dus p99=%dus max=%dus%n",
                latencies[L / 2], latencies[(int) (L * 0.95)], latencies[(int) (L * 0.99)], latencies[L - 1]);

        // --- 并发 RPS ---
        for (int concurrency : new int[]{8, 16, 32}) {
            int perClient = 2000;
            long total = (long) concurrency * perClient;
            ExecutorService pool = Executors.newFixedThreadPool(concurrency);
            CountDownLatch start = new CountDownLatch(1);
            CountDownLatch done = new CountDownLatch(concurrency);
            long[] errs = new long[concurrency];
            long t0 = System.nanoTime();
            for (int c = 0; c < concurrency; c++) {
                final int idx = c;
                pool.submit(() -> {
                    try {
                        start.await();
                        for (int i = 0; i < perClient; i++) {
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
            long errCount = 0;
            for (long e : errs) errCount += e;
            System.out.printf("[bench] 并发=%-2d  %7.0f req/s  (%.3f s / %d 请求, 错误=%d)%n",
                    concurrency, total / secs, secs, total, errCount);
        }

        server.stop();
        System.out.println("[bench] done");
    }
}
