/**
 * sqlite3_hook ? SQLite update_hook ???????????
 *
 * ?? pipe/fifo ?? OS ??????
 *   - hook_wait() ?????????? CPU ???
 *   - update_callback ?? pipe ????????
 */
#define BUILDING_DLL
#include "sqlite3_hook.h"

#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <stdint.h>

#ifdef _WIN32
#include <windows.h>
typedef HANDLE PipeFd;
#define PIPE_INVALID INVALID_HANDLE_VALUE
#define PIPE_BUF_SIZE 4096
#else
#include <unistd.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <sys/select.h>
typedef int PipeFd;
#define PIPE_INVALID (-1)
#define PIPE_BUF_SIZE 4096
#endif

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
    PipeFd         pipe_fd[2];
} HookContext;

#ifdef _WIN32
static int pipe_open(PipeFd fd[2]) {
    return CreatePipe(&fd[0], &fd[1], NULL, 0) ? 0 : -1;
}
static void pipe_close(PipeFd fd[2]) {
    if (fd[0] != PIPE_INVALID) CloseHandle(fd[0]);
    if (fd[1] != PIPE_INVALID) CloseHandle(fd[1]);
    fd[0] = fd[1] = PIPE_INVALID;
}
static int pipe_write_signal(PipeFd w) {
    DWORD n;
    return WriteFile(w, "x", 1, &n, NULL) ? 0 : -1;
}
static int pipe_wait_readable(PipeFd r, int timeout_ms) {
    return WaitForSingleObject(r, timeout_ms <= 0 ? INFINITE : (DWORD)timeout_ms) == WAIT_OBJECT_0;
}
static int pipe_read_byte(PipeFd r, char *buf, int sz) {
    DWORD n;
    return ReadFile(r, buf, (DWORD)sz, &n, NULL) ? (int)n : -1;
}
#else
static int pipe_open(PipeFd fd[2]) {
    return pipe(fd) == 0 ? 0 : -1;
}
static void pipe_close(PipeFd fd[2]) {
    if (fd[0] >= 0) close(fd[0]);
    if (fd[1] >= 0) close(fd[1]);
    fd[0] = fd[1] = PIPE_INVALID;
}
static int pipe_write_signal(PipeFd w) {
    return write(w, "x", 1) == 1 ? 0 : -1;
}
static int pipe_wait_readable(PipeFd r, int timeout_ms) {
    fd_set fds;
    FD_ZERO(&fds);
    FD_SET(r, &fds);
    struct timeval tv;
    if (timeout_ms <= 0) {
        tv.tv_sec = tv.tv_usec = 0;
    } else {
        tv.tv_sec  = timeout_ms / 1000;
        tv.tv_usec = (timeout_ms % 1000) * 1000;
    }
    return select(r + 1, &fds, NULL, NULL, &tv) > 0;
}
static int pipe_read_byte(PipeFd r, char *buf, int sz) {
    return (int)read(r, buf, (size_t)sz);
}
#endif

static void update_callback(void *ctx_ptr, int action, const char *db_name,
                            const char *table_name, sqlite3_int64 row_id)
{
    HookContext *ctx = (HookContext *)ctx_ptr;
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

    /* ? 1 ??? pipe ?? hook_wait */
    if (ctx->pipe_fd[1] != PIPE_INVALID) {
        pipe_write_signal(ctx->pipe_fd[1]);
    }
}

HOOK_API void* hook_open(const char *db_path) {
    if (!db_path) return NULL;
    if (ensure_sqlite_loaded() != 0) return NULL;

    HookContext *ctx = (HookContext *)calloc(1, sizeof(HookContext));
    if (!ctx) return NULL;

    int rc = SQLITE.sqlite3_open(db_path, &ctx->db);
    if (rc != SQLITE_OK) {
        SQLITE.sqlite3_close(ctx->db);
        free(ctx);
        return NULL;
    }

    ctx->active = 1;
    ctx->pipe_fd[0] = PIPE_INVALID;
    ctx->pipe_fd[1] = PIPE_INVALID;
    if (pipe_open(ctx->pipe_fd) != 0) {
        SQLITE.sqlite3_close(ctx->db);
        free(ctx);
        return NULL;
    }

    SQLITE.sqlite3_update_hook(ctx->db, update_callback, ctx);
    return ctx;
}

/**
 * ????????????????????????
 * @param handle     hook_open ?????
 * @param buf        ?????
 * @param buf_size   ?????
 * @param timeout_ms ?????<=0 ???????
 * @return           ?????????? 0????? -1
 */
HOOK_API int hook_wait(void *handle, char *buf, int buf_size, int timeout_ms) {
    if (!handle || !buf || buf_size <= 0) return -1;
    HookContext *ctx = (HookContext *)handle;

    /* ???????????? */
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

    /* ?? pipe ???????????????? */
    char dummy[PIPE_BUF_SIZE];
    while (1) {
        int n = pipe_read_byte(ctx->pipe_fd[0], dummy, PIPE_BUF_SIZE);
        if (n <= 0) break;
    }

    /* OS ?????? */
    if (!pipe_wait_readable(ctx->pipe_fd[0], timeout_ms)) {
        return 0;  /* ?? */
    }

    /* ??? */
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
    HookContext *ctx = (HookContext *)handle;
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
    HookContext *ctx = (HookContext *)handle;
    char *errmsg = NULL;
    int rc = SQLITE.sqlite3_exec(ctx->db, sql, NULL, NULL, &errmsg);
    if (rc != SQLITE_OK && errmsg) {
        SQLITE.sqlite3_free(errmsg);
    }
    return rc;
}

HOOK_API void hook_close(void *handle) {
    if (!handle) return;
    HookContext *ctx = (HookContext *)handle;
    ctx->active = 0;
    SQLITE.sqlite3_update_hook(ctx->db, NULL, NULL);
    SQLITE.sqlite3_close(ctx->db);
    pipe_close(ctx->pipe_fd);
    free(ctx);
}




