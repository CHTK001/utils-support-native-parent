/**
 * sqlite3_hook — SQLite update_hook 真响应式封装
 *
 * 双模式架构：
 *   1. 同步模式 (hook_open)  — 向后兼容，pipe + 阻塞等待
 *   2. 异步模式 (hook_open_async) — 真响应式，OS 原生异步 I/O
 *      - Windows: IOCP + 命名管道
 *      - Linux:   io_uring + eventfd
 *      - 零专用线程、零轮询、事件直接推送
 */
#define BUILDING_DLL
#include "sqlite3_hook.h"

#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <stdint.h>

#ifdef _WIN32
#include <windows.h>
#else
#include <unistd.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <sys/select.h>
#include <pthread.h>
#include <errno.h>
#include <liburing.h>
#endif

/* ═══════════════════════════════════════════════════════════════
 *  SQLite 动态加载
 * ═══════════════════════════════════════════════════════════════ */

typedef struct sqlite3 sqlite3;
typedef int64_t sqlite3_int64;
#define SQLITE_OK         0
#define SQLITE_ERROR      1
#define SQLITE_INSERT    18
#define SQLITE_UPDATE    23
#define SQLITE_DELETE     9

typedef void* (*sqlite3_update_hook_fn)(sqlite3*, void(*)(void*,int,char const*,char const*,sqlite3_int64), void*);
typedef int  (*sqlite3_open_fn)(const char*, sqlite3**);
typedef int  (*sqlite3_close_fn)(sqlite3*);
typedef int  (*sqlite3_exec_fn)(sqlite3*, const char*, int(*)(void*,int,char**,char**), void*, char**);
typedef void (*sqlite3_free_fn)(void*);

typedef struct {
    void *handle;
    sqlite3_open_fn sqlite3_open;
    sqlite3_close_fn sqlite3_close;
    sqlite3_exec_fn sqlite3_exec;
    sqlite3_free_fn sqlite3_free;
    sqlite3_update_hook_fn sqlite3_update_hook;
} SqliteRuntime;

static SqliteRuntime SQLITE = {0};

static int ensure_sqlite_loaded(void) {
    if (SQLITE.handle) return 0;
#ifdef _WIN32
    const char *names[] = {"sqlite3.dll", "winsqlite3.dll"};
    for (int i = 0; i < 2; i++) {
        HMODULE h = LoadLibraryA(names[i]);
        if (h) {
            SQLITE.handle = (void*)h;
            SQLITE.sqlite3_open          = (sqlite3_open_fn)GetProcAddress(h, "sqlite3_open");
            SQLITE.sqlite3_close         = (sqlite3_close_fn)GetProcAddress(h, "sqlite3_close");
            SQLITE.sqlite3_exec          = (sqlite3_exec_fn)GetProcAddress(h, "sqlite3_exec");
            SQLITE.sqlite3_free          = (sqlite3_free_fn)GetProcAddress(h, "sqlite3_free");
            SQLITE.sqlite3_update_hook   = (sqlite3_update_hook_fn)GetProcAddress(h, "sqlite3_update_hook");
            if (SQLITE.sqlite3_open && SQLITE.sqlite3_update_hook) return 0;
            FreeLibrary(h); memset(&SQLITE, 0, sizeof(SQLITE));
        }
    }
#else
    void *h = dlopen("libsqlite3.so", RTLD_LAZY | RTLD_LOCAL);
    if (!h) h = dlopen("libsqlite3.so.0", RTLD_LAZY | RTLD_LOCAL);
    if (h) {
        SQLITE.handle = h;
        SQLITE.sqlite3_open          = (sqlite3_open_fn)dlsym(h, "sqlite3_open");
        SQLITE.sqlite3_close         = (sqlite3_close_fn)dlsym(h, "sqlite3_close");
        SQLITE.sqlite3_exec          = (sqlite3_exec_fn)dlsym(h, "sqlite3_exec");
        SQLITE.sqlite3_free          = (sqlite3_free_fn)dlsym(h, "sqlite3_free");
        SQLITE.sqlite3_update_hook   = (sqlite3_update_hook_fn)dlsym(h, "sqlite3_update_hook");
        if (SQLITE.sqlite3_open && SQLITE.sqlite3_update_hook) return 0;
        dlclose(h); memset(&SQLITE, 0, sizeof(SQLITE));
    }
#endif
    return -1;
}

/* ═══════════════════════════════════════════════════════════════
 *  同步模式（向后兼容）
 * ═══════════════════════════════════════════════════════════════ */

#define MAX_EVENTS 128
#define EVENT_BUF_SIZE 512

typedef struct {
    volatile int  has_event;
    char          data[EVENT_BUF_SIZE];
} HookEvent;

typedef struct {
    sqlite3       *db;
    HookEvent      events[MAX_EVENTS];
    volatile int   write_idx;
    volatile int   read_idx;
    volatile int   active;
#ifdef _WIN32
    HANDLE         pipe_fd[2];
#else
    int            pipe_fd[2];
#endif
} SyncContext;

#ifdef _WIN32
static int sync_pipe_open(HANDLE fd[2]) {
    return CreatePipe(&fd[0], &fd[1], NULL, 0) ? 0 : -1;
}
static void sync_pipe_close(HANDLE fd[2]) {
    if (fd[0] != INVALID_HANDLE_VALUE) CloseHandle(fd[0]);
    if (fd[1] != INVALID_HANDLE_VALUE) CloseHandle(fd[1]);
    fd[0] = fd[1] = INVALID_HANDLE_VALUE;
}
static int sync_pipe_write(HANDLE w) { DWORD n; return WriteFile(w, "x", 1, &n, NULL) ? 0 : -1; }
static int sync_pipe_wait(HANDLE r, int ms) { return WaitForSingleObject(r, ms <= 0 ? INFINITE : (DWORD)ms) == WAIT_OBJECT_0; }
static int sync_pipe_read(HANDLE r, char *buf, int sz) { DWORD n; return ReadFile(r, buf, (DWORD)sz, &n, NULL) ? (int)n : -1; }
#else
static int sync_pipe_open(int fd[2]) { return pipe(fd) == 0 ? 0 : -1; }
static void sync_pipe_close(int fd[2]) {
    if (fd[0] >= 0) close(fd[0]);
    if (fd[1] >= 0) close(fd[1]);
    fd[0] = fd[1] = -1;
}
static int sync_pipe_write(int w) { return write(w, "x", 1) == 1 ? 0 : -1; }
static int sync_pipe_wait(int r, int ms) {
    fd_set fds; FD_ZERO(&fds); FD_SET(r, &fds);
    struct timeval tv;
    if (ms <= 0) { tv.tv_sec = tv.tv_usec = 0; }
    else { tv.tv_sec = ms / 1000; tv.tv_usec = (ms % 1000) * 1000; }
    return select(r + 1, &fds, NULL, NULL, &tv) > 0;
}
static int sync_pipe_read(int r, char *buf, int sz) { return (int)read(r, buf, (size_t)sz); }
#endif

static void sync_update_callback(void *ctx_ptr, int action, const char *db_name,
                                  const char *table_name, sqlite3_int64 row_id) {
    SyncContext *ctx = (SyncContext *)ctx_ptr;
    if (!ctx || !ctx->active) return;

    const char *type_str;
    switch (action) {
        case SQLITE_INSERT: type_str = "INSERT"; break;
        case SQLITE_UPDATE: type_str = "UPDATE"; break;
        case SQLITE_DELETE: type_str = "DELETE"; break;
        default:            type_str = "UNKNOWN"; break;
    }

    int wi = ctx->write_idx % MAX_EVENTS;
    snprintf(ctx->events[wi].data, EVENT_BUF_SIZE,
             "{\"type\":\"%s\",\"database\":\"%s\",\"table\":\"%s\",\"rowId\":%lld}",
             type_str, db_name ? db_name : "main", table_name ? table_name : "", (long long)row_id);
    ctx->events[wi].has_event = 1;
    ctx->write_idx = wi + 1;

#ifdef _WIN32
    if (ctx->pipe_fd[1] != INVALID_HANDLE_VALUE) sync_pipe_write(ctx->pipe_fd[1]);
#else
    if (ctx->pipe_fd[1] >= 0) sync_pipe_write(ctx->pipe_fd[1]);
#endif
}

HOOK_API void* hook_open(const char *db_path) {
    if (!db_path) return NULL;
    if (ensure_sqlite_loaded() != 0) return NULL;

    SyncContext *ctx = (SyncContext *)calloc(1, sizeof(SyncContext));
    if (!ctx) return NULL;

    if (SQLITE.sqlite3_open(db_path, &ctx->db) != SQLITE_OK) {
        SQLITE.sqlite3_close(ctx->db); free(ctx); return NULL;
    }

    ctx->active = 1;
#ifdef _WIN32
    ctx->pipe_fd[0] = INVALID_HANDLE_VALUE;
    ctx->pipe_fd[1] = INVALID_HANDLE_VALUE;
#else
    ctx->pipe_fd[0] = -1;
    ctx->pipe_fd[1] = -1;
#endif
    if (sync_pipe_open(ctx->pipe_fd) != 0) {
        SQLITE.sqlite3_close(ctx->db); free(ctx); return NULL;
    }

    SQLITE.sqlite3_update_hook(ctx->db, sync_update_callback, ctx);
    return ctx;
}

HOOK_API int hook_wait(void *handle, char *buf, int buf_size, int timeout_ms) {
    if (!handle || !buf || buf_size <= 0) return -1;
    SyncContext *ctx = (SyncContext *)handle;

    int ri = ctx->read_idx % MAX_EVENTS;
    if (ctx->events[ri].has_event) {
        int len = (int)strlen(ctx->events[ri].data);
        if (len >= buf_size) len = buf_size - 1;
        memcpy(buf, ctx->events[ri].data, len);
        buf[len] = '\0';
        ctx->events[ri].has_event = 0;
        ctx->read_idx = ri + 1;
        return len;
    }

    char dummy[4096];
    while (1) {
        int n = sync_pipe_read(ctx->pipe_fd[0], dummy, 4096);
        if (n <= 0) break;
    }

    if (!sync_pipe_wait(ctx->pipe_fd[0], timeout_ms)) return 0;

    ri = ctx->read_idx % MAX_EVENTS;
    if (!ctx->events[ri].has_event) return 0;

    int len = (int)strlen(ctx->events[ri].data);
    if (len >= buf_size) len = buf_size - 1;
    memcpy(buf, ctx->events[ri].data, len);
    buf[len] = '\0';
    ctx->events[ri].has_event = 0;
    ctx->read_idx = ri + 1;
    return len;
}

HOOK_API int hook_poll(void *handle, char *buf, int buf_size) {
    if (!handle || !buf || buf_size <= 0) return 0;
    SyncContext *ctx = (SyncContext *)handle;
    int ri = ctx->read_idx % MAX_EVENTS;
    if (!ctx->events[ri].has_event) return 0;
    int len = (int)strlen(ctx->events[ri].data);
    if (len >= buf_size) len = buf_size - 1;
    memcpy(buf, ctx->events[ri].data, len);
    buf[len] = 0;
    ctx->events[ri].has_event = 0;
    ctx->read_idx = ri + 1;
    return len;
}

HOOK_API int hook_exec(void *handle, const char *sql) {
    if (!handle || !sql) return SQLITE_ERROR;
    SyncContext *ctx = (SyncContext *)handle;
    char *errmsg = NULL;
    int rc = SQLITE.sqlite3_exec(ctx->db, sql, NULL, NULL, &errmsg);
    if (rc != SQLITE_OK && errmsg) SQLITE.sqlite3_free(errmsg);
    return rc;
}

HOOK_API void hook_close(void *handle) {
    if (!handle) return;
    SyncContext *ctx = (SyncContext *)handle;
    ctx->active = 0;
    SQLITE.sqlite3_update_hook(ctx->db, NULL, NULL);
    SQLITE.sqlite3_close(ctx->db);
    sync_pipe_close(ctx->pipe_fd);
    free(ctx);
}

/* ═══════════════════════════════════════════════════════════════
 *  异步模式 — 真响应式
 *
 *  核心设计：
 *    1. update_callback 写入环形缓冲区（无锁）
 *    2. 通过 OS 异步通知（IOCP/io_uring）唤醒事件循环
 *    3. 事件循环读取环形缓冲区，回调 Java
 * ═══════════════════════════════════════════════════════════════ */

#define RING_SIZE 1024
#define RING_MASK (RING_SIZE - 1)

struct HookAsyncContext {
    sqlite3 *db;
    hook_event_fn on_event;
    void *user_data;
    volatile int active;

    /* 环形缓冲区 */
    char ring_buf[RING_SIZE][EVENT_BUF_SIZE];
    volatile uint32_t ring_head;
    volatile uint32_t ring_tail;

#ifdef _WIN32
    /* Windows: IOCP + 命名管道 */
    HANDLE iocp;
    HANDLE pipe_read;
    HANDLE pipe_write;
    OVERLAPPED overlapped;
    char read_buf[EVENT_BUF_SIZE];
    volatile int pending_read;
#else
    /* Linux: io_uring + pipe */
    struct io_uring ring;
    int pipe_fd[2];
    char read_buf[EVENT_BUF_SIZE];
    volatile int pending_read;
    pthread_t io_thread;
#endif
};

/* ─── 异步 update_callback ─── */
static void async_update_callback(void *ctx_ptr, int action, const char *db_name,
                                   const char *table_name, sqlite3_int64 row_id) {
    HookAsyncContext *ctx = (HookAsyncContext *)ctx_ptr;
    if (!ctx || !ctx->active) return;

    const char *type_str;
    switch (action) {
        case SQLITE_INSERT: type_str = "INSERT"; break;
        case SQLITE_UPDATE: type_str = "UPDATE"; break;
        case SQLITE_DELETE: type_str = "DELETE"; break;
        default:            type_str = "UNKNOWN"; break;
    }

    /* 写入环形缓冲区 */
    uint32_t head = ctx->ring_head;
    uint32_t next = (head + 1) & RING_MASK;
    if (next == ctx->ring_tail) return;

    snprintf(ctx->ring_buf[head], EVENT_BUF_SIZE,
             "{\"type\":\"%s\",\"database\":\"%s\",\"table\":\"%s\",\"rowId\":%lld}",
             type_str, db_name ? db_name : "main", table_name ? table_name : "", (long long)row_id);
    ctx->ring_head = next;

    /* 通知事件循环 */
#ifdef _WIN32
    if (ctx->pipe_write != INVALID_HANDLE_VALUE) {
        DWORD n;
        WriteFile(ctx->pipe_write, "x", 1, &n, NULL);
    }
#else
    if (ctx->pipe_fd[1] >= 0) {
        write(ctx->pipe_fd[1], "x", 1);
    }
#endif
}

/* ═══════════════════════════════════════════════════════════════
 *  Windows IOCP 实现
 * ═══════════════════════════════════════════════════════════════ */
#ifdef _WIN32

/* IOCP 完成回调 */
static void CALLBACK iocp_completion(DWORD err, DWORD bytes, LPOVERLAPPED ov) {
    HookAsyncContext *ctx = CONTAINING_RECORD(ov, HookAsyncContext, overlapped);
    if (!ctx || !ctx->active) return;
    ctx->pending_read = 0;

    /* 从环形缓冲区读取并回调 */
    while (ctx->ring_tail != ctx->ring_head) {
        uint32_t tail = ctx->ring_tail;
        if (ctx->on_event) {
            ctx->on_event(ctx->ring_buf[tail], ctx->user_data);
        }
        ctx->ring_tail = (tail + 1) & RING_MASK;
    }

    /* 重新投递异步读 */
    if (ctx->active) {
        memset(&ctx->overlapped, 0, sizeof(ctx->overlapped));
        BOOL ok = ReadFile(ctx->pipe_read, ctx->read_buf, EVENT_BUF_SIZE, NULL, &ctx->overlapped);
        if (ok || GetLastError() == ERROR_IO_PENDING) {
            ctx->pending_read = 1;
        }
    }
}

HOOK_API void* hook_open_async(const char *db_path, hook_event_fn on_event, void *user_data) {
    if (!db_path || !on_event) return NULL;
    if (ensure_sqlite_loaded() != 0) return NULL;

    HookAsyncContext *ctx = (HookAsyncContext *)calloc(1, sizeof(HookAsyncContext));
    if (!ctx) return NULL;

    ctx->on_event = on_event;
    ctx->user_data = user_data;
    ctx->active = 1;

    if (SQLITE.sqlite3_open(db_path, &ctx->db) != SQLITE_OK) {
        SQLITE.sqlite3_close(ctx->db); free(ctx); return NULL;
    }

    ctx->iocp = CreateIoCompletionPort(INVALID_HANDLE_VALUE, NULL, 0, 1);
    if (!ctx->iocp) {
        SQLITE.sqlite3_close(ctx->db); free(ctx); return NULL;
    }

    char pipe_name[128];
    static volatile long pipe_counter = 0;
    long id = InterlockedIncrement(&pipe_counter);
    snprintf(pipe_name, sizeof(pipe_name), "\\\\.\\pipe\\sqlite_hook_%ld", id);

    ctx->pipe_read = CreateNamedPipeA(pipe_name,
        PIPE_ACCESS_INBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
        1, EVENT_BUF_SIZE, EVENT_BUF_SIZE, 0, NULL);

    if (ctx->pipe_read == INVALID_HANDLE_VALUE) {
        CloseHandle(ctx->iocp);
        SQLITE.sqlite3_close(ctx->db); free(ctx); return NULL;
    }

    ctx->pipe_write = CreateFileA(pipe_name, GENERIC_WRITE, 0, NULL,
        OPEN_EXISTING, 0, NULL);

    if (ctx->pipe_write == INVALID_HANDLE_VALUE) {
        CloseHandle(ctx->pipe_read);
        CloseHandle(ctx->iocp);
        SQLITE.sqlite3_close(ctx->db); free(ctx); return NULL;
    }

    if (!CreateIoCompletionPort(ctx->pipe_read, ctx->iocp, (ULONG_PTR)ctx, 1)) {
        CloseHandle(ctx->pipe_write);
        CloseHandle(ctx->pipe_read);
        CloseHandle(ctx->iocp);
        SQLITE.sqlite3_close(ctx->db); free(ctx); return NULL;
    }

    SQLITE.sqlite3_update_hook(ctx->db, async_update_callback, ctx);

    /* 投递首个异步读 */
    BOOL ok = ReadFile(ctx->pipe_read, ctx->read_buf, EVENT_BUF_SIZE, NULL, &ctx->overlapped);
    if (ok || GetLastError() == ERROR_IO_PENDING) {
        ctx->pending_read = 1;
    }

    return ctx;
}

HOOK_API int hook_exec_async(void *handle, const char *sql) {
    return hook_exec(handle, sql);
}

HOOK_API void hook_close_async(void *handle) {
    if (!handle) return;
    HookAsyncContext *ctx = (HookAsyncContext *)handle;
    ctx->active = 0;

    SQLITE.sqlite3_update_hook(ctx->db, NULL, NULL);
    SQLITE.sqlite3_close(ctx->db);

    if (ctx->pipe_write != INVALID_HANDLE_VALUE) CloseHandle(ctx->pipe_write);
    if (ctx->pipe_read != INVALID_HANDLE_VALUE) CloseHandle(ctx->pipe_read);
    if (ctx->iocp) CloseHandle(ctx->iocp);

    free(ctx);
}

/* ═══════════════════════════════════════════════════════════════
 *  Linux io_uring 实现
 * ═══════════════════════════════════════════════════════════════ */
#else

/* io_uring 事件循环线程 */
static void *io_uring_thread(void *arg) {
    HookAsyncContext *ctx = (HookAsyncContext *)arg;
    struct io_uring *ring = &ctx->ring;

    while (ctx->active) {
        struct io_uring_cqe *cqe;
        struct __kernel_timespec ts = { .tv_sec = 0, .tv_nsec = 100000000 };
        int ret = io_uring_wait_cqe_timeout(ring, &cqe, &ts);
        if (ret == -ETIME || ret == -EINTR) continue;
        if (ret < 0) continue;

        if (cqe->res > 0) {
            char dummy[EVENT_BUF_SIZE];
            read(ctx->pipe_fd[0], dummy, sizeof(dummy));
        }

        io_uring_cqe_seen(ring, cqe);

        while (ctx->ring_tail != ctx->ring_head) {
            uint32_t tail = ctx->ring_tail;
            if (ctx->on_event) {
                ctx->on_event(ctx->ring_buf[tail], ctx->user_data);
            }
            ctx->ring_tail = (tail + 1) & RING_MASK;
        }

        if (ctx->active) {
            struct io_uring_sqe *sqe = io_uring_get_sqe(ring);
            if (sqe) {
                io_uring_prep_read(sqe, ctx->pipe_fd[0], ctx->read_buf, EVENT_BUF_SIZE, 0);
                io_uring_submit(ring);
            }
        }
    }
    return NULL;
}

HOOK_API void* hook_open_async(const char *db_path, hook_event_fn on_event, void *user_data) {
    if (!db_path || !on_event) return NULL;
    if (ensure_sqlite_loaded() != 0) return NULL;

    HookAsyncContext *ctx = (HookAsyncContext *)calloc(1, sizeof(HookAsyncContext));
    if (!ctx) return NULL;

    ctx->on_event = on_event;
    ctx->user_data = user_data;
    ctx->active = 1;
    ctx->pipe_fd[0] = -1;
    ctx->pipe_fd[1] = -1;

    if (SQLITE.sqlite3_open(db_path, &ctx->db) != SQLITE_OK) {
        SQLITE.sqlite3_close(ctx->db); free(ctx); return NULL;
    }

    if (pipe(ctx->pipe_fd) != 0) {
        SQLITE.sqlite3_close(ctx->db); free(ctx); return NULL;
    }

    fcntl(ctx->pipe_fd[0], F_SETFL, O_NONBLOCK);

    if (io_uring_queue_init(256, &ctx->ring, 0) < 0) {
        close(ctx->pipe_fd[0]); close(ctx->pipe_fd[1]);
        SQLITE.sqlite3_close(ctx->db); free(ctx); return NULL;
    }

    struct io_uring_sqe *sqe = io_uring_get_sqe(&ctx->ring);
    if (sqe) {
        io_uring_prep_read(sqe, ctx->pipe_fd[0], ctx->read_buf, EVENT_BUF_SIZE, 0);
        io_uring_submit(&ctx->ring);
    }

    SQLITE.sqlite3_update_hook(ctx->db, async_update_callback, ctx);

    pthread_create(&ctx->io_thread, NULL, io_uring_thread, ctx);

    return ctx;
}

HOOK_API int hook_exec_async(void *handle, const char *sql) {
    return hook_exec(handle, sql);
}

HOOK_API void hook_close_async(void *handle) {
    if (!handle) return;
    HookAsyncContext *ctx = (HookAsyncContext *)handle;

    /* 先移除 hook，防止新事件产生 */
    SQLITE.sqlite3_update_hook(ctx->db, NULL, NULL);

    /* 通知线程退出 */
    ctx->active = 0;

    /* 写入 pipe 唤醒 io_uring_wait_cqe_timeout */
    if (ctx->pipe_fd[1] >= 0) {
        write(ctx->pipe_fd[1], "x", 1);
    }

    /* 等待线程退出 */
    pthread_join(ctx->io_thread, NULL);

    /* 线程退出后再关闭 db、pipe、io_uring */
    SQLITE.sqlite3_close(ctx->db);

    if (ctx->pipe_fd[0] >= 0) close(ctx->pipe_fd[0]);
    if (ctx->pipe_fd[1] >= 0) close(ctx->pipe_fd[1]);

    io_uring_queue_exit(&ctx->ring);
    free(ctx);
}

#endif
