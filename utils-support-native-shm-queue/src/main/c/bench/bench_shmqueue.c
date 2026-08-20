#include "shmqueue.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
  #include <windows.h>
  typedef HANDLE th_t;
  #define THREAD_SYNC (0)
  #define thread_join(h) WaitForSingleObject(h, INFINITE)
  #define yield_now() Sleep(0)
#else
  #include <pthread.h>
  #include <time.h>
  #include <unistd.h>
  typedef pthread_t th_t;
  #define THREAD_SYNC (NULL)
  #define thread_join(h) pthread_join(h, NULL)
  #define yield_now() sched_yield()
#endif

static uint64_t now_ns(void) {
#ifdef _WIN32
    LARGE_INTEGER f, c;
    QueryPerformanceFrequency(&f);
    QueryPerformanceCounter(&c);
    return (uint64_t)(((c.QuadPart % f.QuadPart) * 1000000000ull) / f.QuadPart
            + (c.QuadPart / f.QuadPart) * 1000000000ull);
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
#endif
}

#define PAYLOAD_LEN 64
#define WARMUP 500000u
#define MEASURE 5000000u

static shm_queue_ctx *g_ctx;
static uint32_t g_count;
static volatile int g_go = 0;

#ifdef _WIN32
static DWORD WINAPI producer(void *p) {
    (void)p;
    uint8_t buf[PAYLOAD_LEN];
    memset(buf, 0xAB, sizeof(buf));
    while (!g_go) Sleep(0);
    for (uint32_t i = 0; i < g_count; ++i) {
        int rc;
        do {
            rc = shmq_send(g_ctx, 1, buf, sizeof(buf));
            if (rc == SHMQ_ERR_QUEUE_FULL) yield_now();
        } while (rc == SHMQ_ERR_QUEUE_FULL);
    }
    return 0;
}
static DWORD WINAPI consumer(void *p) {
    (void)p;
    uint8_t buf[PAYLOAD_LEN];
    while (!g_go) Sleep(0);
    for (uint32_t i = 0; i < g_count; ++i) {
        uint32_t mt, len;
        if (shmq_recv(g_ctx, &mt, buf, sizeof(buf), &len) != SHMQ_OK) return 1;
    }
    return 0;
}
#else
static void *producer(void *p) {
    (void)p;
    uint8_t buf[PAYLOAD_LEN];
    memset(buf, 0xAB, sizeof(buf));
    while (!g_go) yield_now();
    for (uint32_t i = 0; i < g_count; ++i) {
        int rc;
        do {
            rc = shmq_send(g_ctx, 1, buf, sizeof(buf));
            if (rc == SHMQ_ERR_QUEUE_FULL) yield_now();
        } while (rc == SHMQ_ERR_QUEUE_FULL);
    }
    return NULL;
}
static void *consumer(void *p) {
    (void)p;
    uint8_t buf[PAYLOAD_LEN];
    while (!g_go) yield_now();
    for (uint32_t i = 0; i < g_count; ++i) {
        uint32_t mt, len;
        if (shmq_recv(g_ctx, &mt, buf, sizeof(buf), &len) != SHMQ_OK) return (void *)1;
    }
    return NULL;
}
#endif

static void run_bench(const char *name, uint32_t capacity, int mode) {
    shm_queue_ctx *ctx = NULL;
    char shm_name[64];
    snprintf(shm_name, sizeof(shm_name), "/shmq_bench_%s", name);
#ifdef _WIN32
    (void)shm_name;
    /* Windows 复用同一名字即可，进程内线程 SPSC */
    snprintf(shm_name, sizeof(shm_name), "/shmq_bench");
#endif
    int rc = shmq_create(shm_name, 0, capacity, PAYLOAD_LEN + 8, mode, &ctx);
    if (rc != SHMQ_OK) {
        fprintf(stderr, "create %s failed: %d\n", name, rc);
        return;
    }
    g_ctx = ctx;

    /* warmup */
    g_count = WARMUP;
    th_t tp, tc;
#ifdef _WIN32
    tp = CreateThread(NULL, 0, producer, NULL, 0, NULL);
    tc = CreateThread(NULL, 0, consumer, NULL, 0, NULL);
#else
    pthread_create(&tp, NULL, producer, NULL);
    pthread_create(&tc, NULL, consumer, NULL);
#endif
    g_go = 1;
    thread_join(tp);
    thread_join(tc);
    g_go = 0;

    /* measure */
    g_count = MEASURE;
#ifdef _WIN32
    tp = CreateThread(NULL, 0, producer, NULL, 0, NULL);
    tc = CreateThread(NULL, 0, consumer, NULL, 0, NULL);
#else
    pthread_create(&tp, NULL, producer, NULL);
    pthread_create(&tc, NULL, consumer, NULL);
#endif
    g_go = 1;
    uint64_t t0 = now_ns();
    thread_join(tp);
    thread_join(tc);
    uint64_t t1 = now_ns();
    g_go = 0;

    double secs = (double)(t1 - t0) / 1e9;
    double msgs = MEASURE;
    printf("%-8s 容量=%-5u %8.1f 万条/s  %6.1f MB/s  (%.3f s / %u 条, %u B/条)\n",
           name, capacity, msgs / secs / 1e4, msgs * (PAYLOAD_LEN + 8) / secs / 1e6,
           secs, (unsigned)MEASURE, (unsigned)(PAYLOAD_LEN + 8));
    shmq_destroy(ctx, 0);
}

int main(void) {
    printf("=== shmqueue 吞吐基准 (双线程 SPSC, 载荷 %d B) ===\n", PAYLOAD_LEN);
    run_bench("SPIN",   256, SHMQ_WAIT_SPIN);
    run_bench("BLOCK",  256, SHMQ_WAIT_BLOCK);
    run_bench("HYBRID", 256, SHMQ_WAIT_HYBRID);
    run_bench("HYBRID-C4K", 4096, SHMQ_WAIT_HYBRID);
    printf("done\n");
    return 0;
}
