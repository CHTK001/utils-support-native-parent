import com.chua.common.support.shmqueue.ShmQueue;

import java.nio.ByteBuffer;
import java.util.concurrent.atomic.AtomicLong;

/**
 * ShmQueue 吞吐基准：
 *  SPSC 双线程（生产者+消费者），按等待模式对比 msgs/s 与 MB/s。
 *  载荷 64 B，warmup 后测 200 万条。
 */
public class ShmQueueBench {

    private static final int N = 2_000_000;
    private static final byte[] PAYLOAD = new byte[64];

    public static void main(String[] args) throws Exception {
        System.out.println("=== ShmQueue 吞吐基准 (SPSC 双线程, 载荷 64B, 200万条) ===");
        bench(ShmQueue.Mode.SPIN, 512);
        bench(ShmQueue.Mode.BLOCK, 512);
        bench(ShmQueue.Mode.HYBRID, 512);
        bench(ShmQueue.Mode.HYBRID, 4096);
        System.out.println("done");
    }

    private static void bench(ShmQueue.Mode mode, int capacity) throws Exception {
        try (ShmQueue q = ShmQueue.create("/bench_" + mode + "_" + capacity, capacity, 128, mode)) {
            AtomicLong done = new AtomicLong();
            Thread producer = new Thread(() -> {
                for (int i = 0; i < N; i++) {
                    while (true) {
                        try {
                            q.send(i, PAYLOAD);
                            break;
                        } catch (Exception e) {
                            // queue full -> retry
                        }
                    }
                }
            }, "bench-producer");
            Thread consumer = new Thread(() -> {
                for (int i = 0; i < N; i++) {
                    q.recv();
                }
                done.set(1);
            }, "bench-consumer");

            producer.start();
            consumer.start();
            long t0 = System.nanoTime();
            producer.join();
            consumer.join();
            long t1 = System.nanoTime();
            double secs = (t1 - t0) / 1e9;
            System.out.printf("%-6s 容量=%-4d %8.1f 万条/s  %6.1f MB/s  (%.3f s / %d 条, %d B/条)%n",
                    mode, capacity, N / secs / 1e4, N * (64 + 8) / secs / 1e6, secs, N, 64 + 8);
        }
    }
}
