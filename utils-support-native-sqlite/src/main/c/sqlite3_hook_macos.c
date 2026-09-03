/**
 * sqlite3_hook — SQLite update_hook macOS 实现（kqueue/select + pipe）
 *
 * macOS 没有 io_uring，采用 POSIX pipe + select 线程模型：
 *   1. update_callback 将事件写入环形缓冲区并通知管道
 *   2. 独立线程 select 等待管道可读，从环形缓冲区读取并回调 Java FFM
 */

#define BUILDING_DLL
#include "sqlite3_hook.h"

#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <stdint.h>
#include <dlfcn.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/select.h>
#include <pthread.h>
#include <errno.h>

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
    const char *names[] = {"libsqlite3.dylib", "/usr/lib/libsqlite3.dylib"};
    for (int i = 0; i < 2; i++) {
        void *h = dlopen(names[i], RTLD_LAZY | RTLD_LOCAL);
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
    }
    return -1;
}

/* ═══════════════════════════════════════════════════════════════
 *  同步 API
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
    int            pipe_fd[2];
} SyncContext;

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

    if (ctx->pipe_fd[1] >= 0) {
        write(ctx->pipe_fd[1], "x", 1);
    }
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
    ctx->pipe_fd[0] = -1;
    ctx->pipe_fd[1] = -1;

    if (pipe(ctx->pipe_fd) != 0) {
        SQLITE.sqlite3_close(ctx->db); free(ctx); return NULL;
    }
    fcntl(ctx->pipe_fd[0], F_SETFL, O_NONBLOCK);

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
    while (read(ctx->pipe_fd[0], dummy, sizeof(dummy)) > 0) {}

    fd_set fds; FD_ZERO(&fds); FD_SET(ctx->pipe_fd[0], &fds);
    struct timeval tv;
    if (timeout_ms <= 0) { tv.tv_sec = 0; tv.tv_usec = 0; }
    else { tv.tv_sec = timeout_ms / 1000; tv.tv_usec = (timeout_ms % 1000) * 1000; }
    if (select(ctx->pipe_fd[0] + 1, &fds, NULL, NULL, &tv) <= 0) return 0;

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
    if (ctx->pipe_fd[0] >= 0) close(ctx->pipe_fd[0]);
    if (ctx->pipe_fd[1] >= 0) close(ctx->pipe_fd[1]);
    free(ctx);
}

/* ═══════════════════════════════════════════════════════════════
 *  异步 API — POSIX pipe + select + pthread
 * ═══════════════════════════════════════════════════════════════ */

#define RING_SIZE 1024
#define RING_MASK (RING_SIZE - 1)

struct HookAsyncContext {
    sqlite3 *db;
    hook_event_fn on_event;
    void *user_data;
    volatile int active;

    char ring_buf[RING_SIZE][EVENT_BUF_SIZE];
    volatile uint32_t ring_head;
    volatile uint32_t ring_tail;

    int pipe_fd[2];
    char read_buf[EVENT_BUF_SIZE];
    volatile int pending_read;
    pthread_t io_thread;
};

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

    uint32_t head = ctx->ring_head;
    uint32_t next = (head + 1) & RING_MASK;
    if (next == ctx->ring_tail) return;

    snprintf(ctx->ring_buf[head], EVENT_BUF_SIZE,
             "{\"type\":\"%s\",\"database\":\"%s\",\"table\":\"%s\",\"rowId\":%lld}",
             type_str, db_name ? db_name : "main", table_name ? table_name : "", (long long)row_id);
    ctx->ring_head = next;

    if (ctx->pipe_fd[1] >= 0) {
        write(ctx->pipe_fd[1], "x", 1);
    }
}

static void *select_thread(void *arg) {
    HookAsyncContext *ctx = (HookAsyncContext *)arg;

    while (ctx->active) {
        fd_set fds; FD_ZERO(&fds); FD_SET(ctx->pipe_fd[0], &fds);
        struct timeval tv = { .tv_sec = 0, .tv_usec = 100000 };
        int r = select(ctx->pipe_fd[0] + 1, &fds, NULL, NULL, &tv);
        if (r < 0) {
            if (errno == EINTR) continue;
            break;
        }
        if (r > 0) {
            char dummy[EVENT_BUF_SIZE];
            while (read(ctx->pipe_fd[0], dummy, sizeof(dummy)) > 0) {}
        }

        while (ctx->ring_tail != ctx->ring_head) {
            uint32_t tail = ctx->ring_tail;
            if (ctx->on_event) {
                ctx->on_event(ctx->ring_buf[tail], ctx->user_data);
            }
            ctx->ring_tail = (tail + 1) & RING_MASK;
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

    SQLITE.sqlite3_update_hook(ctx->db, async_update_callback, ctx);

    pthread_create(&ctx->io_thread, NULL, select_thread, ctx);
    pthread_detach(ctx->io_thread);

    return ctx;
}

HOOK_API int hook_exec_async(void *handle, const char *sql) {
    return hook_exec(handle, sql);
}

HOOK_API void hook_close_async(void *handle) {
    if (!handle) return;
    HookAsyncContext *ctx = (HookAsyncContext *)handle;
    ctx->active = 0;

    usleep(1000);

    SQLITE.sqlite3_update_hook(ctx->db, NULL, NULL);
    SQLITE.sqlite3_close(ctx->db);

    if (ctx->pipe_fd[0] >= 0) close(ctx->pipe_fd[0]);
    if (ctx->pipe_fd[1] >= 0) close(ctx->pipe_fd[1]);

    free(ctx);
}