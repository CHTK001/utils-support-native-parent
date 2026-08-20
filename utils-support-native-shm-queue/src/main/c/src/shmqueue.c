#include "shmqueue.h"
#include "atomic_ops.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>

#ifdef _WIN32
  #include <windows.h>
  typedef HANDLE shm_handle_t;
  typedef HANDLE notify_handle_t;
  #define SHMQ_INVALID_HANDLE ((HANDLE)(intptr_t)-1)
  #define shmq_close_handle(h) CloseHandle(h)
#else
  #include <fcntl.h>
  #include <unistd.h>
  #include <sys/mman.h>
  #include <sys/stat.h>
  #include <time.h>
  #ifdef __linux__
    #include <sys/eventfd.h>
    #include <poll.h>
  #endif
  #if defined(__i386__) || defined(__x86_64__)
    #include <immintrin.h>
  #endif
  typedef int shm_handle_t;
  typedef int notify_handle_t;
  #define SHMQ_INVALID_HANDLE (-1)
  #define shmq_close_handle(h) do { if ((h) >= 0) ::close(h); } while (0)
#endif

#define SHMQ_HEADER_SIZE      64
#define SHMQ_NOTIFY_NAME_OFF  32
#define SHMQ_NOTIFY_NAME_LEN  32

/* 判满瞬态误判时的自旋重试次数（pause 指令级，约几十微秒） */
#define SHMQ_SEND_FULL_RETRIES 10000

/* 每槽发布状态：接收方只读取 READY 槽，CAS 抢占后写入再置 READY（无锁 MPSC） */
#define SHMQ_STATE_EMPTY 0u
#define SHMQ_STATE_READY 1u

struct shmq_header {
    uint32_t     magic;
    uint32_t     version;
    uint32_t     capacity;
    uint32_t     slot_size;
    uint32_t     mode;
    uint32_t     _pad;
    atomic_u32   read_index;
    atomic_u32   write_index;
    char         notify_name[SHMQ_NOTIFY_NAME_LEN];
};

struct shm_queue_ctx {
    char           shm_name[256];
    size_t         shm_size;
    uint32_t       capacity;
    uint32_t       slot_size;
    int            mode;
    uint64_t       spin_ns;
    shm_handle_t   shm_fd;
    notify_handle_t notify_fd;
    void          *map_addr;
    struct shmq_header *header;
    atomic_u32    *state;      /* capacity 个槽位的发布状态 */
    uint8_t       *data_area;  /* 槽数据区起点 */
    int            is_creator;
};

/* 槽位布局：header(64, 按 8 对齐) + state[capacity]*4 + 数据槽 */
static size_t shmq_state_offset(uint32_t capacity) {
    size_t off = SHMQ_HEADER_SIZE;
    return (off + 7u) & ~(size_t)7u;
}

/* ==================== 平台辅助 ==================== */

static int shmq_open_or_create_shm(const char *name, size_t size, int create,
                                   shm_handle_t *out_fd) {
#ifdef _WIN32
    DWORD size_high = (DWORD)((size >> 32) & 0xFFFFFFFFu);
    DWORD size_low  = (DWORD)(size & 0xFFFFFFFFu);
    HANDLE h = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE,
                                  size_high, size_low, name);
    if (h == NULL) {
        return SHMQ_ERR_OPEN_SHM;
    }
    if (!create) {
        DWORD err = GetLastError();
        if (err == ERROR_ALREADY_EXISTS) {
            *out_fd = h;
            return SHMQ_OK;
        }
        CloseHandle(h);
        return SHMQ_ERR_OPEN_SHM;
    }
    *out_fd = h;
    return SHMQ_OK;
#else
    int fd;
    if (create) {
        fd = shm_open(name, O_RDWR | O_CREAT | O_EXCL, 0600);
        if (fd < 0) {
            if (errno == EEXIST) {
                fd = shm_open(name, O_RDWR, 0600);
                if (fd < 0) return SHMQ_ERR_OPEN_SHM;
                *out_fd = fd;
                return SHMQ_OK;
            }
            return SHMQ_ERR_OPEN_SHM;
        }
        if (ftruncate(fd, (off_t)size) != 0) {
            ::close(fd);
            shm_unlink(name);
            return SHMQ_ERR_TRUNCATE;
        }
    } else {
        fd = shm_open(name, O_RDWR, 0600);
        if (fd < 0) return SHMQ_ERR_OPEN_SHM;
    }
    *out_fd = fd;
    return SHMQ_OK;
#endif
}

static int shmq_map_shm(shm_handle_t fd, size_t size, void **out_addr) {
#ifdef _WIN32
    (void)fd;
    void *addr = MapViewOfFile(fd, FILE_MAP_ALL_ACCESS, 0, 0, size);
    if (addr == NULL) return SHMQ_ERR_MMAP;
    *out_addr = addr;
    return SHMQ_OK;
#else
    void *addr = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (addr == MAP_FAILED) return SHMQ_ERR_MMAP;
    *out_addr = addr;
    return SHMQ_OK;
#endif
}

static void shmq_unmap_shm(void *addr, size_t size) {
#ifdef _WIN32
    (void)size;
    UnmapViewOfFile(addr);
#else
    munmap(addr, size);
#endif
}

static int shmq_create_or_open_notify(const char *notify_name, int create,
                                      notify_handle_t *out) {
#ifdef _WIN32
    (void)create;
    HANDLE h = CreateEventA(NULL, FALSE, FALSE, notify_name);
    if (h == NULL) return SHMQ_ERR_OPEN_SHM;
    *out = h;
    return SHMQ_OK;
#else
    (void)notify_name;
    (void)create;
    int fd = eventfd(0, EFD_NONBLOCK | EFD_CLOEXEC);
    if (fd < 0) return SHMQ_ERR_OPEN_SHM;
    *out = fd;
    return SHMQ_OK;
#endif
}

static int shmq_notify_wake(notify_handle_t fd) {
#ifdef _WIN32
    return SetEvent(fd) ? SHMQ_OK : SHMQ_ERR_WRITE_FD;
#else
    uint64_t one = 1;
    ssize_t n = write(fd, &one, sizeof(one));
    return (n == (ssize_t)sizeof(one)) ? SHMQ_OK : SHMQ_ERR_WRITE_FD;
#endif
}

static int shmq_notify_wait(notify_handle_t fd) {
#ifdef _WIN32
    DWORD r = WaitForSingleObject(fd, INFINITE);
    return (r == WAIT_OBJECT_0) ? SHMQ_OK : SHMQ_ERR_READ_FD;
#else
    uint64_t v = 0;
    for (;;) {
        ssize_t n = read(fd, &v, sizeof(v));
        if (n == (ssize_t)sizeof(v)) return SHMQ_OK;
        if (n < 0 && errno == EINTR) continue;
        return SHMQ_ERR_READ_FD;
    }
#endif
}

static int shmq_notify_wait_timeout(notify_handle_t fd, uint64_t timeout_ns) {
#ifdef _WIN32
    DWORD ms = (DWORD)(timeout_ns / 1000000ull);
    if (ms == 0 && timeout_ns > 0) ms = 1;
    DWORD r = WaitForSingleObject(fd, ms);
    if (r == WAIT_OBJECT_0) return SHMQ_OK;
    if (r == WAIT_TIMEOUT)  return SHMQ_ERR_TIMEOUT;
    return SHMQ_ERR_READ_FD;
#else
    struct pollfd pfd = { .fd = fd, .events = POLLIN };
    int ms = (int)(timeout_ns / 1000000ull);
    if (ms == 0 && timeout_ns > 0) ms = 1;
    int r;
    do {
        r = poll(&pfd, 1, ms);
    } while (r < 0 && errno == EINTR);
    if (r == 0) return SHMQ_ERR_TIMEOUT;
    if (r < 0)  return SHMQ_ERR_READ_FD;
    uint64_t v = 0;
    ssize_t n;
    do {
        n = read(fd, &v, sizeof(v));
    } while (n < 0 && errno == EINTR);
    return SHMQ_OK;
#endif
}

static void shmq_build_notify_name(const char *shm_name, char *out, size_t cap) {
#ifdef _WIN32
    snprintf(out, cap, "Local\\shmq_ntf_%s", shm_name);
    for (size_t i = 7; i < strlen(out); ++i) {
        if (out[i] == '/' || out[i] == '\\') out[i] = '_';
    }
#else
    snprintf(out, cap, "%s", shm_name);
#endif
}

/* ==================== 公共 API ==================== */

int shmq_create(const char *name, size_t shm_size,
                uint32_t capacity, uint32_t slot_size, int mode,
                shm_queue_ctx **ctx_out) {
    if (!name || !ctx_out) return SHMQ_ERR_INVALID_ARG;
    if (mode < SHMQ_WAIT_SPIN || mode > SHMQ_WAIT_HYBRID) return SHMQ_ERR_INVALID_ARG;
    if (capacity < 2) return SHMQ_ERR_INVALID_ARG;
    if (slot_size < SHMQ_MIN_SLOT_SIZE) return SHMQ_ERR_INVALID_ARG;

    if (shm_size == 0) {
        shm_size = shmq_state_offset(capacity) + (size_t)capacity * 4u + (size_t)capacity * (size_t)slot_size;
    }

    shm_queue_ctx *ctx = (shm_queue_ctx *)calloc(1, sizeof(*ctx));
    if (!ctx) return SHMQ_ERR_NOMEM;
    ctx->shm_fd = SHMQ_INVALID_HANDLE;
    ctx->notify_fd = SHMQ_INVALID_HANDLE;
    ctx->mode = mode;
    ctx->capacity = capacity;
    ctx->slot_size = slot_size;
    ctx->shm_size = shm_size;
    ctx->spin_ns = SHMQ_DEFAULT_SPIN_NS;
    ctx->is_creator = 1;
    snprintf(ctx->shm_name, sizeof(ctx->shm_name), "%s", name);

    int rc = shmq_open_or_create_shm(name, shm_size, 1, &ctx->shm_fd);
    if (rc != SHMQ_OK) {
        rc = shmq_open_or_create_shm(name, shm_size, 0, &ctx->shm_fd);
        if (rc != SHMQ_OK) {
            free(ctx);
            return rc;
        }
        ctx->is_creator = 0;
    }

    rc = shmq_map_shm(ctx->shm_fd, shm_size, &ctx->map_addr);
    if (rc != SHMQ_OK) {
        shmq_close_handle(ctx->shm_fd);
        free(ctx);
        return rc;
    }
    ctx->header = (struct shmq_header *)ctx->map_addr;
    ctx->state = (atomic_u32 *)((uint8_t *)ctx->map_addr + shmq_state_offset(capacity));
    ctx->data_area = (uint8_t *)ctx->state + (size_t)capacity * 4u;

    uint32_t magic = shmq_atomic_load_relaxed((atomic_u32 *)&ctx->header->magic);
    if (magic == SHMQ_MAGIC) {
        if (ctx->header->version != SHMQ_VERSION) {
            shmq_unmap_shm(ctx->map_addr, shm_size);
            shmq_close_handle(ctx->shm_fd);
            free(ctx);
            return SHMQ_ERR_HEADER_VERSION;
        }
        if (ctx->header->capacity != capacity || ctx->header->slot_size != slot_size) {
            shmq_unmap_shm(ctx->map_addr, shm_size);
            shmq_close_handle(ctx->shm_fd);
            free(ctx);
            return SHMQ_ERR_INVALID_ARG;
        }
        ctx->is_creator = 0;
    } else if (magic == 0) {
        ctx->header->magic = SHMQ_MAGIC;
        ctx->header->version = SHMQ_VERSION;
        ctx->header->capacity = capacity;
        ctx->header->slot_size = slot_size;
        ctx->header->mode = (uint32_t)mode;
        shmq_atomic_store_relaxed((atomic_u32 *)&ctx->header->read_index, 0);
        shmq_atomic_store_relaxed((atomic_u32 *)&ctx->header->write_index, 0);
        for (uint32_t i = 0; i < capacity; ++i) {
            shmq_atomic_store_relaxed(&ctx->state[i], SHMQ_STATE_EMPTY);
        }
    } else {
        shmq_unmap_shm(ctx->map_addr, shm_size);
        shmq_close_handle(ctx->shm_fd);
        free(ctx);
        return SHMQ_ERR_HEADER_MAGIC;
    }

    char notify_name[256];
    shmq_build_notify_name(name, notify_name, sizeof(notify_name));
    rc = shmq_create_or_open_notify(notify_name, ctx->is_creator, &ctx->notify_fd);
    if (rc != SHMQ_OK) {
        shmq_unmap_shm(ctx->map_addr, shm_size);
        shmq_close_handle(ctx->shm_fd);
        free(ctx);
        return rc;
    }

    *ctx_out = ctx;
    return SHMQ_OK;
}

int shmq_attach(const char *name, size_t shm_size,
                uint32_t capacity, uint32_t slot_size, int mode,
                shm_queue_ctx **ctx_out) {
    if (!name || !ctx_out) return SHMQ_ERR_INVALID_ARG;
    (void)capacity;
    (void)slot_size;
    (void)mode;

    shm_queue_ctx *ctx = (shm_queue_ctx *)calloc(1, sizeof(*ctx));
    if (!ctx) return SHMQ_ERR_NOMEM;
    ctx->shm_fd = SHMQ_INVALID_HANDLE;
    ctx->notify_fd = SHMQ_INVALID_HANDLE;
    ctx->spin_ns = SHMQ_DEFAULT_SPIN_NS;
    ctx->is_creator = 0;
    snprintf(ctx->shm_name, sizeof(ctx->shm_name), "%s", name);

    /* 阶段一：打开并只映射 64 字节头，读取真实容量/槽大小 */
    int rc = shmq_open_or_create_shm(name, SHMQ_HEADER_SIZE, 0, &ctx->shm_fd);
    if (rc != SHMQ_OK) {
        free(ctx);
        return rc;
    }
    void *hdr_map = NULL;
    rc = shmq_map_shm(ctx->shm_fd, SHMQ_HEADER_SIZE, &hdr_map);
    if (rc != SHMQ_OK) {
        shmq_close_handle(ctx->shm_fd);
        free(ctx);
        return rc;
    }
    struct shmq_header *hdr = (struct shmq_header *)hdr_map;
    if (shmq_atomic_load_relaxed((atomic_u32 *)&hdr->magic) != SHMQ_MAGIC) {
        shmq_unmap_shm(hdr_map, SHMQ_HEADER_SIZE);
        shmq_close_handle(ctx->shm_fd);
        free(ctx);
        return SHMQ_ERR_HEADER_MAGIC;
    }
    if (hdr->version != SHMQ_VERSION) {
        shmq_unmap_shm(hdr_map, SHMQ_HEADER_SIZE);
        shmq_close_handle(ctx->shm_fd);
        free(ctx);
        return SHMQ_ERR_HEADER_VERSION;
    }
    ctx->capacity = hdr->capacity;
    ctx->slot_size = hdr->slot_size;
    ctx->mode = (int)hdr->mode;
    size_t real_size = shmq_state_offset(ctx->capacity)
            + (size_t)ctx->capacity * 4u
            + (size_t)ctx->capacity * ctx->slot_size;
    shmq_unmap_shm(hdr_map, SHMQ_HEADER_SIZE);

    /* 阶段二：按真实大小重新映射完整段 */
    ctx->shm_size = real_size;
    rc = shmq_map_shm(ctx->shm_fd, real_size, &ctx->map_addr);
    if (rc != SHMQ_OK) {
        shmq_close_handle(ctx->shm_fd);
        free(ctx);
        return rc;
    }
    ctx->header = (struct shmq_header *)ctx->map_addr;
    ctx->state = (atomic_u32 *)((uint8_t *)ctx->map_addr + shmq_state_offset(ctx->capacity));
    ctx->data_area = (uint8_t *)ctx->state + (size_t)ctx->capacity * 4u;

    char notify_name[256];
    shmq_build_notify_name(name, notify_name, sizeof(notify_name));
    rc = shmq_create_or_open_notify(notify_name, 0, &ctx->notify_fd);
    if (rc != SHMQ_OK) {
        shmq_unmap_shm(ctx->map_addr, real_size);
        shmq_close_handle(ctx->shm_fd);
        free(ctx);
        return rc;
    }

    *ctx_out = ctx;
    return SHMQ_OK;
}

static inline uint8_t *slot_at(struct shm_queue_ctx *ctx, uint32_t idx) {
    return ctx->data_area + (size_t)idx * ctx->slot_size;
}

static inline uint32_t slot_state_load(struct shm_queue_ctx *ctx, uint32_t idx) {
    return shmq_atomic_load_acquire(&ctx->state[idx]);
}

static inline void slot_state_store(struct shm_queue_ctx *ctx, uint32_t idx, uint32_t v) {
    shmq_atomic_store_release(&ctx->state[idx], v);
}

static inline void shmq_cpu_pause(void);

/**
 * CAS 无锁发送（支持多生产者 MPSC）。
 * * 通过 CAS 抢占 write_index 获得唯一槽位，写完数据后以 release 置槽 READY 发布；
 * 接收方只读取 READY 槽，因此并发生产者之间不会互相覆盖，也不会让接收方读到
 * 未写完的数据。
 *
 * 满判定 (w+1)%capacity == r 存在瞬态误判：读取方推进 read_index 与生产者读到
 * 旧 r 之间存在窗口，队列实际已腾出空间却仍判满。因此判满后做有界自旋重试，
 * 仅当持续满才返回 SHMQ_ERR_QUEUE_FULL（调用方可重试）。
 */
int shmq_send(shm_queue_ctx *ctx, uint32_t msg_type, const void *data, uint32_t len) {
    if (!ctx) return SHMQ_ERR_INVALID_ARG;
    if (len + 8u > ctx->slot_size) return SHMQ_ERR_DATA_TOO_LARGE;

    uint32_t slot;
    int attempts = 0;
    for (;;) {
        uint32_t w = shmq_atomic_load_relaxed(&ctx->header->write_index);
        uint32_t r = shmq_atomic_load_acquire(&ctx->header->read_index);
        if (((w + 1u) % ctx->capacity) == r) {
            if (++attempts >= SHMQ_SEND_FULL_RETRIES) return SHMQ_ERR_QUEUE_FULL;
            shmq_cpu_pause();
            continue;
        }
        uint32_t next_w = (w + 1u) % ctx->capacity;
        if (shmq_atomic_cas_acqrel(&ctx->header->write_index, w, next_w)) {
            slot = w;
            break;
        }
    }

    uint8_t *s = slot_at(ctx, slot);
    memcpy(s, &msg_type, 4);
    memcpy(s + 4, &len, 4);
    if (len > 0 && data) {
        memcpy(s + 8, data, len);
    }
    /* release：先数据后状态，保证接收方读到 READY 时数据完整可见 */
    slot_state_store(ctx, slot, SHMQ_STATE_READY);

    if (ctx->mode == SHMQ_WAIT_BLOCK || ctx->mode == SHMQ_WAIT_HYBRID) {
        (void)shmq_notify_wake(ctx->notify_fd);
    }
    return SHMQ_OK;
}

/**
 * 是否有可读数据：队列非空，且队首槽已发布（READY）。
 */
static inline int data_ready(struct shm_queue_ctx *ctx) {
    uint32_t r = shmq_atomic_load_acquire(&ctx->header->read_index);
    uint32_t w = shmq_atomic_load_acquire(&ctx->header->write_index);
    if (r == w) return 0;
    return slot_state_load(ctx, r) == SHMQ_STATE_READY;
}

static inline void shmq_cpu_pause(void) {
#ifdef _WIN32
    YieldProcessor();
#else
#if defined(__i386__) || defined(__x86_64__)
    _mm_pause();
#else
    __asm__ __volatile__("" ::: "memory");
#endif
#endif
}

static inline void spin_wait(uint64_t ns) {
#ifdef _WIN32
    LARGE_INTEGER freq, start, now;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&start);
    for (;;) {
        YieldProcessor();
        QueryPerformanceCounter(&now);
        uint64_t elapsed = (uint64_t)(((now.QuadPart - start.QuadPart) * 1000000000ull) / (uint64_t)freq.QuadPart);
        if (elapsed >= ns) break;
    }
#else
    struct timespec start, now;
    clock_gettime(CLOCK_MONOTONIC, &start);
    for (;;) {
#if defined(__i386__) || defined(__x86_64__)
        _mm_pause();
#endif
        clock_gettime(CLOCK_MONOTONIC, &now);
        uint64_t elapsed = (uint64_t)(now.tv_sec - start.tv_sec) * 1000000000ull
                         + (uint64_t)(now.tv_nsec - start.tv_nsec);
        if (elapsed >= ns) break;
    }
#endif
}

static int shmq_do_recv(shm_queue_ctx *ctx,
                        uint32_t *msg_type, void *buf, uint32_t buf_cap, uint32_t *len_out,
                        int has_timeout, uint64_t timeout_ns) {
    if (!ctx || !msg_type || !len_out) return SHMQ_ERR_INVALID_ARG;
    if (!buf && buf_cap > 0) return SHMQ_ERR_INVALID_ARG;

    uint64_t elapsed_ns = 0;
#ifdef _WIN32
    LARGE_INTEGER freq, start;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&start);
#else
    struct timespec ts_start;
    clock_gettime(CLOCK_MONOTONIC, &ts_start);
#endif

    while (1) {
        if (!data_ready(ctx)) {
            switch (ctx->mode) {
                case SHMQ_WAIT_SPIN:
                    break;
                case SHMQ_WAIT_BLOCK: {
                    int rc;
                    if (has_timeout) {
                        rc = shmq_notify_wait_timeout(ctx->notify_fd, timeout_ns);
                        if (rc == SHMQ_ERR_TIMEOUT) return rc;
                    } else {
                        rc = shmq_notify_wait(ctx->notify_fd);
                    }
                    if (rc != SHMQ_OK) return rc;
                    if (!data_ready(ctx)) continue;
                    goto have_data;
                }
                case SHMQ_WAIT_HYBRID: {
                    spin_wait(ctx->spin_ns);
                    if (data_ready(ctx)) goto have_data;
                    int rc;
                    if (has_timeout) {
                        rc = shmq_notify_wait_timeout(ctx->notify_fd, timeout_ns);
                        if (rc == SHMQ_ERR_TIMEOUT) return rc;
                        if (rc != SHMQ_OK) return rc;
                    } else {
                        rc = shmq_notify_wait(ctx->notify_fd);
                        if (rc != SHMQ_OK) return rc;
                    }
                    if (!data_ready(ctx)) continue;
                    goto have_data;
                }
            }
            if (has_timeout) {
#ifdef _WIN32
                LARGE_INTEGER now;
                QueryPerformanceCounter(&now);
                elapsed_ns = (uint64_t)(((now.QuadPart - start.QuadPart) * 1000000000ull) / (uint64_t)freq.QuadPart);
#else
                struct timespec now;
                clock_gettime(CLOCK_MONOTONIC, &now);
                elapsed_ns = (uint64_t)(now.tv_sec - ts_start.tv_sec) * 1000000000ull
                           + (uint64_t)(now.tv_nsec - ts_start.tv_nsec);
#endif
                if (elapsed_ns >= timeout_ns) return SHMQ_ERR_TIMEOUT;
            }
            continue;
        }
have_data: {
        uint32_t r = shmq_atomic_load_acquire(&ctx->header->read_index);
        uint8_t *slot = slot_at(ctx, r);
        uint32_t mt, ln;
        memcpy(&mt, slot, 4);
        memcpy(&ln, slot + 4, 4);
        if (ln > buf_cap) return SHMQ_ERR_DATA_TOO_LARGE;
        if (ln > 0) memcpy(buf, slot + 8, ln);
        *msg_type = mt;
        *len_out = ln;
        /* 先重置槽为 EMPTY，再推进读索引，允许生产者安全复用该槽 */
        slot_state_store(ctx, r, SHMQ_STATE_EMPTY);
        uint32_t next_r = (r + 1u) % ctx->capacity;
        shmq_atomic_store_release(&ctx->header->read_index, next_r);
        return SHMQ_OK;
        }
    }
}

int shmq_recv(shm_queue_ctx *ctx,
              uint32_t *msg_type, void *buf, uint32_t buf_cap, uint32_t *len_out) {
    return shmq_do_recv(ctx, msg_type, buf, buf_cap, len_out, 0, 0);
}

int shmq_recv_timeout(shm_queue_ctx *ctx,
                      uint32_t *msg_type, void *buf, uint32_t buf_cap, uint32_t *len_out,
                      uint64_t timeout_ns) {
    return shmq_do_recv(ctx, msg_type, buf, buf_cap, len_out, 1, timeout_ns);
}

int shmq_get_notify_fd(shm_queue_ctx *ctx, int *fd) {
    if (!ctx || !fd) return SHMQ_ERR_INVALID_ARG;
#ifdef _WIN32
    (void)ctx; (void)fd;
    return SHMQ_ERR_NOT_SUPPORTED;
#else
    if (ctx->mode == SHMQ_WAIT_SPIN) return SHMQ_ERR_INVALID_ARG;
    *fd = ctx->notify_fd;
    return SHMQ_OK;
#endif
}

#ifdef _WIN32
int shmq_get_notify_handle(shm_queue_ctx *ctx, void **handle_out) {
    if (!ctx || !handle_out) return SHMQ_ERR_INVALID_ARG;
    if (ctx->mode == SHMQ_WAIT_SPIN) return SHMQ_ERR_INVALID_ARG;
    *handle_out = (void *)ctx->notify_fd;
    return SHMQ_OK;
}
#endif

int shmq_set_spin_ns(shm_queue_ctx *ctx, uint64_t spin_ns) {
    if (!ctx) return SHMQ_ERR_INVALID_ARG;
    ctx->spin_ns = spin_ns;
    return SHMQ_OK;
}

int shmq_capacity(shm_queue_ctx *ctx, uint32_t *out) {
    if (!ctx || !out) return SHMQ_ERR_INVALID_ARG;
    *out = ctx->capacity;
    return SHMQ_OK;
}

int shmq_slot_size(shm_queue_ctx *ctx, uint32_t *out) {
    if (!ctx || !out) return SHMQ_ERR_INVALID_ARG;
    *out = ctx->slot_size;
    return SHMQ_OK;
}

int shmq_mode(shm_queue_ctx *ctx, int *out) {
    if (!ctx || !out) return SHMQ_ERR_INVALID_ARG;
    *out = ctx->mode;
    return SHMQ_OK;
}

void shmq_destroy(shm_queue_ctx *ctx, int unlink) {
    if (!ctx) return;
    if (unlink && ctx->is_creator) {
#ifdef _WIN32
        /* Windows 无直接删除 API，由系统在最后 handle 关闭后回收 */
#else
        shm_unlink(ctx->shm_name);
#endif
    }
    if (ctx->map_addr) {
        shmq_unmap_shm(ctx->map_addr, ctx->shm_size);
    }
    if (ctx->shm_fd != SHMQ_INVALID_HANDLE) {
        shmq_close_handle(ctx->shm_fd);
    }
    if (ctx->notify_fd != SHMQ_INVALID_HANDLE) {
        shmq_close_handle(ctx->notify_fd);
    }
    free(ctx);
}

int shmq_unlink(const char *name) {
    if (!name) return SHMQ_ERR_INVALID_ARG;
#ifdef _WIN32
    (void)name;
    return SHMQ_ERR_NOT_SUPPORTED;
#else
    if (shm_unlink(name) != 0) return SHMQ_ERR_INVALID_ARG;
    return SHMQ_OK;
#endif
}

const char *shmq_strerror(int err) {
    switch (err) {
        case SHMQ_OK: return "OK";
        case SHMQ_ERR_INVALID_ARG:    return "invalid argument";
        case SHMQ_ERR_NOMEM:          return "out of memory";
        case SHMQ_ERR_OPEN_SHM:       return "open shm failed";
        case SHMQ_ERR_TRUNCATE:       return "ftruncate failed";
        case SHMQ_ERR_MMAP:           return "mmap failed";
        case SHMQ_ERR_HEADER_MAGIC:   return "header magic mismatch";
        case SHMQ_ERR_HEADER_VERSION: return "header version mismatch";
        case SHMQ_ERR_QUEUE_FULL:     return "queue full";
        case SHMQ_ERR_DATA_TOO_LARGE: return "data too large for slot";
        case SHMQ_ERR_WRITE_FD:       return "write notify fd failed";
        case SHMQ_ERR_READ_FD:        return "read notify fd failed";
        case SHMQ_ERR_TIMEOUT:        return "recv timeout";
        case SHMQ_ERR_NOT_SUPPORTED:  return "not supported on this platform";
        case SHMQ_ERR_DESTROYED:      return "queue destroyed";
        default:                      return "unknown error";
    }
}
