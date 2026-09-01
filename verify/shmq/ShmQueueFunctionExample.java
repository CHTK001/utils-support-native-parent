import com.chua.common.support.shmqueue.ShmQueue;
import com.chua.common.support.shmqueue.ShmQueueException;

import java.util.concurrent.atomic.AtomicLong;

/**
 * ShmQueue SPI 全功能测试：
 *  1. 基础收发往返
 *  2. 顺序保持（1000 条）
 *  3. recvTimeout 超时后数据到达
 *  4. 队列满错误码
 *  5. 数据过大错误码
 *  6. attach 第二句柄互通
 *  7. 双线程 SPSC 压力（10 万条校验和）
 *  8. close 幂等
 */
public class ShmQueueFunctionExample {

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
        System.out.println("===== ShmQueue 全功能测试 =====");

        // 1. 基础收发
        try (ShmQueue q = ShmQueue.create("/fn_basic", 32, 256, ShmQueue.Mode.HYBRID)) {
            q.send(1, "hello".getBytes());
            ShmQueue.Message m = q.recv();
            check(m.type() == 1 && "hello".equals(new String(m.bytes())), "基础收发往返",
                    "type=" + m.type() + " text=" + new String(m.bytes()));
        }

        // 2. 顺序保持（容量需 > 1000，SPSC 可用槽 = capacity-1）
        try (ShmQueue q = ShmQueue.create("/fn_order", 2048, 128, ShmQueue.Mode.SPIN)) {
            boolean ok = true;
            for (int i = 0; i < 1000; i++) {
                q.send(i, new byte[0]);
            }
            for (int i = 0; i < 1000; i++) {
                if (q.recv().type() != i) {
                    ok = false;
                    break;
                }
            }
            check(ok, "顺序保持(1000)", ok ? "ok" : "顺序错乱");
        }

        // 3. recvTimeout 超时 + 数据到达
        try (ShmQueue q = ShmQueue.create("/fn_timeout", 16, 128, ShmQueue.Mode.BLOCK)) {
            long t0 = System.nanoTime();
            boolean timeoutOk = false;
            try {
                q.recvTimeout(30_000_000L); // 30ms
            } catch (ShmQueueException e) {
                timeoutOk = e.getCode() == ShmQueue.ERR_TIMEOUT;
            }
            long elapsedMs = (System.nanoTime() - t0) / 1_000_000;
            check(timeoutOk, "recvTimeout 返回 ERR_TIMEOUT", "code=" + (timeoutOk ? "OK" : "??") + " 耗时=" + elapsedMs + "ms");
            q.send(9, "later".getBytes());
            ShmQueue.Message m = q.recvTimeout(2000_000_000L);
            check(m.type() == 9 && "later".equals(new String(m.bytes())), "超时后数据可接收", "type=" + m.type());
        }

        // 4. 队列满
        try (ShmQueue q = ShmQueue.create("/fn_full", 4, 64, ShmQueue.Mode.SPIN)) {
            for (int i = 0; i < 3; i++) q.send(i, new byte[0]);
            boolean fullOk = false;
            try {
                q.send(99, new byte[0]);
            } catch (ShmQueueException e) {
                fullOk = e.getCode() == ShmQueue.ERR_QUEUE_FULL;
            }
            check(fullOk, "队列满返回 ERR_QUEUE_FULL", "");
            q.recv();
            q.send(100, new byte[0]);
            check(true, "消费后恢复可写", "");
        }

        // 5. 数据过大
        try (ShmQueue q = ShmQueue.create("/fn_large", 8, 16, ShmQueue.Mode.HYBRID)) {
            boolean largeOk = false;
            try {
                q.send(1, new byte[64]);
            } catch (ShmQueueException e) {
                largeOk = e.getCode() == ShmQueue.ERR_DATA_TOO_LARGE;
            }
            check(largeOk, "数据过大返回 ERR_DATA_TOO_LARGE", "");
        }

        // 6. attach 第二句柄互通
        try (ShmQueue c1 = ShmQueue.create("/fn_attach", 16, 256, ShmQueue.Mode.HYBRID);
             ShmQueue c2 = ShmQueue.attach("/fn_attach")) {
            c1.send(42, "cross".getBytes());
            ShmQueue.Message m = c2.recv();
            check(m.type() == 42 && "cross".equals(new String(m.bytes())), "attach 第二句柄互通", "type=" + m.type());
        }

        // 7. 双线程 SPSC 压力 10 万条
        final int N = 100_000;
        try (ShmQueue q = ShmQueue.create("/fn_stress", 512, 64, ShmQueue.Mode.HYBRID)) {
            AtomicLong sum = new AtomicLong();
            Thread producer = new Thread(() -> {
                for (int i = 0; i < N; i++) {
                    byte[] b = java.nio.ByteBuffer.allocate(4).putInt(i).array();
                    while (true) {
                        try {
                            q.send(1, b);
                            break;
                        } catch (ShmQueueException e) {
                            // 队列满，重试（应有阻塞通知，但此处双保险）
                        }
                    }
                }
            }, "producer");
            Thread consumer = new Thread(() -> {
                long s = 0;
                for (int i = 0; i < N; i++) {
                    ShmQueue.Message m = q.recv();
                    s += java.nio.ByteBuffer.wrap(m.bytes()).getInt();
                }
                sum.set(s);
            }, "consumer");
            long t0 = System.currentTimeMillis();
            producer.start();
            consumer.start();
            producer.join();
            consumer.join();
            long elapsed = System.currentTimeMillis() - t0;
            long expect = (long) N * (N - 1) / 2;
            check(sum.get() == expect, "双线程 SPSC 压力(" + N + " 条, " + elapsed + "ms)", "sum=" + sum.get() + " expect=" + expect);
        }

        // 8. close 幂等
        ShmQueue q = ShmQueue.create("/fn_close", 8, 64, ShmQueue.Mode.SPIN);
        q.close();
        q.close();
        check(true, "close 幂等", "");

        // 9. 多生产者 MPSC（CAS 无锁）4 生产者 → 单消费者，校验和
        final int MPSC_N = 100_000;
        final int producers = 4;
        try (ShmQueue mq = ShmQueue.create("/fn_mpsc", 512, 64, ShmQueue.Mode.HYBRID)) {
            AtomicLong msum = new AtomicLong();
            Thread consumer = new Thread(() -> {
                long s = 0;
                for (int i = 0; i < MPSC_N; i++) {
                    s += java.nio.ByteBuffer.wrap(mq.recv().bytes()).getInt();
                }
                msum.set(s);
            }, "mpsc-consumer");
            consumer.start();
            Thread[] ps = new Thread[producers];
            for (int t = 0; t < producers; t++) {
                final int tid = t;
                final int per = MPSC_N / producers;
                ps[t] = new Thread(() -> {
                    for (int i = 0; i < per; i++) {
                        byte[] b = java.nio.ByteBuffer.allocate(4).putInt(tid * per + i).array();
                        while (true) {
                            try {
                                mq.send(1, b);
                                break;
                            } catch (ShmQueueException e) {
                                // queue full retry
                            }
                        }
                    }
                }, "mpsc-producer-" + t);
                ps[t].start();
            }
            for (Thread p : ps) p.join();
            consumer.join();
            long expect = (long) MPSC_N * (MPSC_N - 1) / 2;
            check(msum.get() == expect, "多生产者 MPSC(" + producers + " 生产者," + MPSC_N + " 条)", "sum=" + msum.get() + " expect=" + expect);
        }

        System.out.println(failures == 0 ? "===== 全部通过 =====" : "===== 失败 " + failures + " 项 =====");
        System.exit(failures == 0 ? 0 : 1);
    }
}
