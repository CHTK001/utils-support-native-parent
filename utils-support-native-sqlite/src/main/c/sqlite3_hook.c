/**
 * sqlite3_hook — SQLite update_hook 动态库
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
#define INVALID_PIPE_FD INVALID_HANDLE_VALUE
#else
#include <unistd.h>
typedef int PipeFd;
#define INVALID_PIPE_FD (-1)
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
            FreeLibrary(h);
            memset(&SQLITE, 0, sizeof(SQLITE));
        }
    }
    return -1;
}

#define MAX_EVENTS 128
#define EVENT_BUF_SIZE 512

typedef struct {
    volatile int has_event;
    char data[EVENT_BUF_SIZE];
} HookEvent;

typedef struct {
    sqlite3 *db;
    HookEvent events[MAX_EVENTS];
    volatile int write_idx;
    volatile int read_idx;
    volatile int active;
} HookContext;

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
    SQLITE.sqlite3_update_hook(ctx->db, update_callback, ctx);
    return ctx;
}

HOOK_API int hook_poll(void *handle, char *buf, int buf_size) {
    if (!handle || !buf || buf_size <= 0) return 0;
    HookContext *ctx = (HookContext *)handle;
    int ri = ctx->read_idx % MAX_EVENTS;
    if (!ctx->events[ri].has_event) return 0;

    int len = (int)strlen(ctx->events[ri].data);
    if (len >= buf_size) len = buf_size - 1;
    memcpy(buf, ctx->events[ri].data, len);
    buf[len] = '\0';

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
    free(ctx);
}
