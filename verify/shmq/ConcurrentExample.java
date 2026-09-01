import com.chua.common.support.nativehttp.server.ShmHttpServer;
import com.chua.common.support.network.server.ServerSetting;

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicInteger;

public class ConcurrentExample {
    static {
        System.setOut(new java.io.PrintStream(new java.io.FileOutputStream(java.io.FileDescriptor.out), true, java.nio.charset.StandardCharsets.UTF_8));
    }

    public static void main(String[] args) throws Exception {
        int port = 18120;
        ServerSetting setting = ServerSetting.builder()
                .host("0.0.0.0").port(port).protocol("http").auto(false).build();
        ShmHttpServer server = new ShmHttpServer(setting, 1024, 16 * 1024);
        server.registerMapping("/hello", (req, res) -> res.setBody("world").end());
        server.start();
        System.out.println("[diag] started");

        HttpClient client = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(5))
                .version(HttpClient.Version.HTTP_1_1)
                .build();
        URI uri = URI.create("http://127.0.0.1:" + port + "/hello");
        HttpRequest req = HttpRequest.newBuilder(uri).GET().build();

        // 3 轮并发，每轮 100 请求
        for (int round = 1; round <= 3; round++) {
            int N = 100;
            ExecutorService pool = Executors.newFixedThreadPool(16);
            CountDownLatch start = new CountDownLatch(1);
            CountDownLatch done = new CountDownLatch(N);
            ConcurrentHashMap<Integer, Integer> codes = new ConcurrentHashMap<>();
            List<Long> times = new CopyOnWriteArrayList<>();
            long t0 = System.nanoTime();
            for (int i = 0; i < N; i++) {
                pool.submit(() -> {
                    try {
                        start.await();
                        long s = System.nanoTime();
                        HttpResponse<String> r = client.send(req, HttpResponse.BodyHandlers.ofString());
                        times.add((System.nanoTime() - s) / 1_000_000);
                        codes.merge(r.statusCode(), 1, Integer::sum);
                    } catch (Exception e) {
                        codes.merge(-1, 1, Integer::sum);
                    } finally {
                        done.countDown();
                    }
                });
            }
            start.countDown();
            done.await(60, TimeUnit.SECONDS);
            long elapsedMs = (System.nanoTime() - t0) / 1_000_000;
            pool.shutdown();
            List<Long> sorted = new ArrayList<>(times);
            sorted.sort(Long::compareTo);
            long p50 = sorted.get(sorted.size() / 2);
            long p99 = sorted.get((int) (sorted.size() * 0.99));
            System.out.println("[diag] round=" + round + " 耗时=" + elapsedMs + "ms codes=" + codes + " p50=" + p50 + "ms p99=" + p99 + "ms");
        }

        server.stop();
        System.out.println("[diag] done");
    }
}
