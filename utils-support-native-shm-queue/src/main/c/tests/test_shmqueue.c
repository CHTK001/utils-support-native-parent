#include "shmqueue.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
  #include <windows.h>
  #define sleep_ms(ms) Sleep(ms)
  static uint64_t now_ms(void) {
      LARGE_INTEGER freq, c;
      QueryPerformanceFrequency(&freq);
      QueryPerformanceCounter(&c);
      return (uint64_t)((c.QuadPart * 1000ull) / freq.QuadPart);
  }
#else
  #include <unistd.h>
  #include <pthread.h>
  #include <time.h>
  #define sleep_ms(ms) usleep((ms) * 1000)
  static uint64_t now_ms(void) {
      struct timespec ts;
      clock_gettime(CLOCK_MONOTONIC, &ts);
      return (uint64_t)ts.tv_sec * 1000ull + (uint64_t)ts.tv_nsec / 1000000ull;
  }
#endif

static int g_failures = 0;
#define ASSERT_OK(expr) do { \
    int _r = (expr); \
    if (_r != SHMQ_OK) { \
        fprintf(stderr, "[FAIL] %s -> %d (%s) at line %d\n", #expr, _r, shmq_strerror(_r), __LINE__); \
        g_failures++; \
    } \
} while (0)

#define ASSERT_EQ(a, b) do { \
    if ((a) != (b)) { \
        fprintf(stderr, "[FAIL] %s (%lld) != %s (%lld) at line %d\n", \
                #a, (long long)(a), #b, (long long)(b), __LINE__); \
        g_failures++; \
    } \
} while (0)

static const char *TEST_NAME = "/shmq_ut";
static void cleanup_shm(void) {
#ifndef _WIN32
    shm_unlink(TEST_NAME);
#endif
}

static void test_basic_send_recv(void) {
    cleanup_shm();
    shm_queue_ctx *ctx = NULL;
    ASSERT_OK(shmq_create(TEST_NAME, 0, 16, 256, SHMQ_WAIT_HYBRID, &ctx));
    if (!ctx) return;
    const char *msg = "hello world";
    ASSERT_OK(shmq_send(ctx, 1, msg, (uint32_t)strlen(msg) + 1));
    uint32_t mt = 0, got = 0;
    char buf[256];
    ASSERT_OK(shmq_recv(ctx, &mt, buf, sizeof(buf), &got));
    ASSERT_EQ(mt, 1u);
    ASSERT_EQ(got, (uint32_t)strlen(msg) + 1);
    ASSERT_EQ(strcmp(buf, msg), 0);
    shmq_destroy(ctx, 0);
    cleanup_shm();
}

static void test_order_spin(void) {
    cleanup_shm();
    shm_queue_ctx *ctx = NULL;
    uint32_t cap = 64;
    ASSERT_OK(shmq_create(TEST_NAME, 0, cap, 128, SHMQ_WAIT_SPIN, &ctx));
    if (!ctx) return;
    /* SPSC 满判定为 (w+1)%cap==r，故最多可存 cap-1 条 */
    for (uint32_t i = 0; i < cap - 1; ++i) {
        uint32_t v = i;
        ASSERT_OK(shmq_send(ctx, 0xCAFE, &v, sizeof(v)));
    }
    for (uint32_t i = 0; i < cap - 1; ++i) {
        uint32_t mt = 0, got = 0, v = 0;
        ASSERT_OK(shmq_recv(ctx, &mt, &v, sizeof(v), &got));
        ASSERT_EQ(mt, 0xCAFEu);
        ASSERT_EQ(v, i);
    }
    shmq_destroy(ctx, 0);
    cleanup_shm();
}

static void test_queue_full(void) {
    cleanup_shm();
    shm_queue_ctx *ctx = NULL;
    ASSERT_OK(shmq_create(TEST_NAME, 0, 4, 64, SHMQ_WAIT_SPIN, &ctx));
    if (!ctx) return;
    uint32_t v = 0;
    ASSERT_OK(shmq_send(ctx, 1, &v, sizeof(v)));
    ASSERT_OK(shmq_send(ctx, 1, &v, sizeof(v)));
    ASSERT_OK(shmq_send(ctx, 1, &v, sizeof(v)));
    ASSERT_EQ(shmq_send(ctx, 1, &v, sizeof(v)), SHMQ_ERR_QUEUE_FULL);
    uint32_t mt, got;
    ASSERT_OK(shmq_recv(ctx, &mt, &v, sizeof(v), &got));
    ASSERT_OK(shmq_send(ctx, 1, &v, sizeof(v)));
    shmq_destroy(ctx, 0);
    cleanup_shm();
}

static void test_data_too_large(void) {
    cleanup_shm();
    shm_queue_ctx *ctx = NULL;
    ASSERT_OK(shmq_create(TEST_NAME, 0, 8, 16, SHMQ_WAIT_HYBRID, &ctx));
    if (!ctx) return;
    char buf[64] = {0};
    ASSERT_EQ(shmq_send(ctx, 1, buf, sizeof(buf)), SHMQ_ERR_DATA_TOO_LARGE);
    shmq_destroy(ctx, 0);
    cleanup_shm();
}

static void test_recv_timeout(void) {
    cleanup_shm();
    shm_queue_ctx *ctx = NULL;
    ASSERT_OK(shmq_create(TEST_NAME, 0, 8, 64, SHMQ_WAIT_BLOCK, &ctx));
    if (!ctx) return;
    uint32_t mt, got;
    char buf[16];
    uint64_t t0 = now_ms();
    ASSERT_EQ(shmq_recv_timeout(ctx, &mt, buf, sizeof(buf), &got, 50 * 1000 * 1000ull),
              SHMQ_ERR_TIMEOUT);
    uint64_t elapsed = now_ms() - t0;
    if (elapsed < 40 || elapsed > 1000) {
        fprintf(stderr, "[WARN] timeout elapsed=%llu ms\n", (unsigned long long)elapsed);
    }
    shmq_destroy(ctx, 0);
    cleanup_shm();
}

static void test_attach(void) {
    cleanup_shm();
    shm_queue_ctx *c1 = NULL, *c2 = NULL;
    ASSERT_OK(shmq_create(TEST_NAME, 0, 16, 128, SHMQ_WAIT_HYBRID, &c1));
    if (!c1) return;
    ASSERT_OK(shmq_attach(TEST_NAME, 0, 0, 0, 0, &c2));
    if (c2) {
        uint32_t cap1, cap2;
        shmq_capacity(c1, &cap1);
        shmq_capacity(c2, &cap2);
        ASSERT_EQ(cap1, cap2);
        shmq_destroy(c2, 0);
    }
    shmq_destroy(c1, 0);
    cleanup_shm();
}

/* ===== 多线程 SPSC 压力测试（线程替代跨进程，Windows 也能跑） ===== */

#define MT_COUNT 200000u

#ifdef _WIN32
static DWORD WINAPI mt_producer(LPVOID p) {
    shm_queue_ctx *ctx = (shm_queue_ctx *)p;
    for (uint32_t i = 0; i < MT_COUNT; ++i) {
        int rc;
        do {
            rc = shmq_send(ctx, 1, &i, sizeof(i));
            if (rc == SHMQ_ERR_QUEUE_FULL) {
                Sleep(0); /* 让出时间片，等待消费者腾出空间 */
            }
        } while (rc == SHMQ_ERR_QUEUE_FULL);
        if (rc != SHMQ_OK) return 1;
    }
    return 0;
}
static DWORD WINAPI mt_consumer(LPVOID p) {
    shm_queue_ctx *ctx = (shm_queue_ctx *)p;
    uint64_t sum = 0;
    for (uint32_t i = 0; i < MT_COUNT; ++i) {
        uint32_t mt, got, v = 0;
        int rc = shmq_recv(ctx, &mt, &v, sizeof(v), &got);
        if (rc != SHMQ_OK || mt != 1 || got != sizeof(v)) return 2;
        sum += v;
    }
    return (sum == (uint64_t)MT_COUNT * (MT_COUNT - 1) / 2) ? 0 : 3;
}
#else
static void *mt_producer(void *p) {
    shm_queue_ctx *ctx = (shm_queue_ctx *)p;
    for (uint32_t i = 0; i < MT_COUNT; ++i) {
        int rc;
        do {
            rc = shmq_send(ctx, 1, &i, sizeof(i));
            if (rc == SHMQ_ERR_QUEUE_FULL) {
                sched_yield();
            }
        } while (rc == SHMQ_ERR_QUEUE_FULL);
        if (rc != SHMQ_OK) return (void *)1;
    }
    return NULL;
}
static void *mt_consumer(void *p) {
    shm_queue_ctx *ctx = (shm_queue_ctx *)p;
    uint64_t sum = 0;
    for (uint32_t i = 0; i < MT_COUNT; ++i) {
        uint32_t mt, got, v = 0;
        int rc = shmq_recv(ctx, &mt, &v, sizeof(v), &got);
        if (rc != SHMQ_OK || mt != 1 || got != sizeof(v)) return (void *)2;
        sum += v;
    }
    return (sum == (uint64_t)MT_COUNT * (MT_COUNT - 1) / 2) ? NULL : (void *)3;
}
#endif

static void test_multithread_spsc(void) {
    cleanup_shm();
    shm_queue_ctx *ctx = NULL;
    ASSERT_OK(shmq_create(TEST_NAME, 0, 256, 64, SHMQ_WAIT_HYBRID, &ctx));
    if (!ctx) return;
    uint64_t t0 = now_ms();
#ifdef _WIN32
    HANDLE probes = CreateThread(NULL, 0, mt_producer, ctx, 0, NULL);
    HANDLE conss = CreateThread(NULL, 0, mt_consumer, ctx, 0, NULL);
    DWORD r1 = 0, r2 = 0;
    WaitForSingleObject(probes, INFINITE);
    WaitForSingleObject(conss, INFINITE);
    GetExitCodeThread(probes, &r1);
    GetExitCodeThread(conss, &r2);
    CloseHandle(probes);
    CloseHandle(conss);
    const char *fail1 = (r1 == 0) ? NULL : "producer failed";
    const char *fail2 = (r2 == 0) ? NULL : "consumer failed";
#else
    pthread_t tp, tc;
    pthread_create(&tp, NULL, mt_producer, ctx);
    pthread_create(&tc, NULL, mt_consumer, ctx);
    void *r1 = NULL, *r2 = NULL;
    pthread_join(tp, &r1);
    pthread_join(tc, &r2);
    const char *fail1 = (r1 == NULL) ? NULL : "producer failed";
    const char *fail2 = (r2 == NULL) ? NULL : "consumer failed";
#endif
    uint64_t elapsed = now_ms() - t0;
    if (fail1) {
        fprintf(stderr, "[FAIL] %s\n", fail1);
        g_failures++;
    }
    if (fail2) {
        fprintf(stderr, "[FAIL] %s\n", fail2);
        g_failures++;
    }
    if (!fail1 && !fail2) {
        printf("[OK] multithread SPSC: %u msgs %llu ms -> PASSED\n",
               (unsigned)MT_COUNT, (unsigned long long)elapsed);
    }
    shmq_destroy(ctx, 0);
    cleanup_shm();
}

static void test_block_wakeup(void) {
    cleanup_shm();
    shm_queue_ctx *ctx = NULL;
    ASSERT_OK(shmq_create(TEST_NAME, 0, 8, 64, SHMQ_WAIT_BLOCK, &ctx));
    if (!ctx) return;
#ifndef _WIN32
    pid_t pid = fork();
    if (pid == 0) {
        shm_queue_ctx *c2 = NULL;
        if (shmq_attach(TEST_NAME, 0, 0, 0, 0, &c2) != SHMQ_OK) _exit(1);
        uint32_t mt, got;
        char buf[64];
        int rc = shmq_recv(c2, &mt, buf, sizeof(buf), &got);
        if (rc != SHMQ_OK || mt != 42 || got != 5 || memcmp(buf, "wake", 5) != 0) _exit(2);
        shmq_destroy(c2, 0);
        _exit(0);
    }
    sleep_ms(50);
    ASSERT_OK(shmq_send(ctx, 42, "wake", 5));
    int status = 0;
    waitpid(pid, &status, 0);
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        fprintf(stderr, "[FAIL] child exited %d\n", status);
        g_failures++;
    }
#endif
    shmq_destroy(ctx, 0);
    cleanup_shm();
}

/* ===== 多生产者 MPSC 压力测试（CAS 无锁抢占槽位） ===== */

#define MPSC_THREADS 4
#define MPSC_PER_THREAD 50000u
#define MPSC_TOTAL ((uint32_t)MPSC_THREADS * MPSC_PER_THREAD)

static shm_queue_ctx *g_mpsc_ctx;

#ifdef _WIN32
static DWORD WINAPI mpsc_producer(LPVOID p) {
    uintptr_t t = (uintptr_t)p;
    shm_queue_ctx *ctx = g_mpsc_ctx;
    for (uint32_t i = 0; i < MPSC_PER_THREAD; ++i) {
        uint32_t v = (uint32_t)((uint64_t)t * MPSC_PER_THREAD + i);
        int rc;
        do {
            rc = shmq_send(ctx, 1, &v, sizeof(v));
            if (rc == SHMQ_ERR_QUEUE_FULL) Sleep(0);
        } while (rc == SHMQ_ERR_QUEUE_FULL);
        if (rc != SHMQ_OK) return 1;
    }
    return 0;
}
#else
static void *mpsc_producer(void *p) {
    uintptr_t t = (uintptr_t)p;
    shm_queue_ctx *ctx = g_mpsc_ctx;
    for (uint32_t i = 0; i < MPSC_PER_THREAD; ++i) {
        uint32_t v = (uint32_t)((uint64_t)t * MPSC_PER_THREAD + i);
        int rc;
        do {
            rc = shmq_send(ctx, 1, &v, sizeof(v));
            if (rc == SHMQ_ERR_QUEUE_FULL) sched_yield();
        } while (rc == SHMQ_ERR_QUEUE_FULL);
        if (rc != SHMQ_OK) return (void *)1;
    }
    return NULL;
}
#endif

static void test_multiproducer_mpsc(void) {
    cleanup_shm();
    shm_queue_ctx *ctx = NULL;
    ASSERT_OK(shmq_create(TEST_NAME, 0, 512, 64, SHMQ_WAIT_HYBRID, &ctx));
    if (!ctx) return;
    g_mpsc_ctx = ctx;

#ifdef _WIN32
    HANDLE threads[MPSC_THREADS];
    for (uintptr_t t = 0; t < MPSC_THREADS; ++t) {
        threads[t] = CreateThread(NULL, 0, mpsc_producer, (LPVOID)t, 0, NULL);
    }
#else
    pthread_t threads[MPSC_THREADS];
    for (uintptr_t t = 0; t < MPSC_THREADS; ++t) {
        pthread_create(&threads[t], NULL, mpsc_producer, (void *)t);
    }
#endif

    uint64_t sum = 0;
    for (uint32_t i = 0; i < MPSC_TOTAL; ++i) {
        uint32_t mt, got, v = 0;
        int rc = shmq_recv(ctx, &mt, &v, sizeof(v), &got);
        if (rc != SHMQ_OK || mt != 1 || got != sizeof(v)) {
            fprintf(stderr, "[FAIL] MPSC recv failed: %d\n", rc);
            g_failures++;
            break;
        }
        sum += v;
    }
#ifdef _WIN32
    for (uintptr_t t = 0; t < MPSC_THREADS; ++t) {
        WaitForSingleObject(threads[t], INFINITE);
        CloseHandle(threads[t]);
    }
#else
    for (uintptr_t t = 0; t < MPSC_THREADS; ++t) {
        pthread_join(threads[t], NULL);
    }
#endif
    uint64_t expect = (uint64_t)MPSC_TOTAL * (MPSC_TOTAL - 1) / 2;
    if (sum == expect) {
        printf("[OK] multiproducer MPSC: %u msgs, 4 生产者 -> PASSED\n", (unsigned)MPSC_TOTAL);
    } else {
        fprintf(stderr, "[FAIL] MPSC sum=%llu expect=%llu\n",
                (unsigned long long)sum, (unsigned long long)expect);
        g_failures++;
    }
    shmq_destroy(ctx, 0);
    cleanup_shm();
}

int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    fprintf(stderr, "=== shmqueue unit tests ===\n");
    fprintf(stderr, "T1 basic_send_recv\n"); test_basic_send_recv();
    fprintf(stderr, "T2 order_spin\n"); test_order_spin();
    fprintf(stderr, "T3 queue_full\n"); test_queue_full();
    fprintf(stderr, "T4 data_too_large\n"); test_data_too_large();
    fprintf(stderr, "T5 recv_timeout\n"); test_recv_timeout();
    fprintf(stderr, "T6 block_wakeup\n"); test_block_wakeup();
    fprintf(stderr, "T7 attach\n"); test_attach();
    fprintf(stderr, "T8 multithread_spsc\n"); test_multithread_spsc();
    fprintf(stderr, "T9 multiproducer_mpsc\n"); test_multiproducer_mpsc();
    if (g_failures == 0) {
        printf("ALL TESTS PASSED\n");
        return 0;
    }
    fprintf(stderr, "FAILED: %d\n", g_failures);
    return 1;
}
