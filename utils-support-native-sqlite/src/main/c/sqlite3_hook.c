/**
 * sqlite3_hook — SQLite update_hook 动态库
 *
 * 基于 sqlite3_update_hook() 回调机制，实时捕获 INSERT / UPDATE / DELETE 数据变更。
 * 变更事件以 JSON 格式写入内部环形缓冲区，通过跨平台 pipe 通知 Java FFI 侧消费。
 *
 * Java 侧通过 hook_pipe_read() 阻塞等待，在 OS 层面挂起（无 CPU 轮询），
 * C 侧 hook 回调写入 pipe 后立即唤醒，实现真正的响应式事件推送。
 *
 * 运行时动态加载 sqlite3.dll（Windows）或 libsqlite3.so（Linux/macOS），
 * 无需编译时 SQLite 合并包依赖。
 *
 * 编译（Windows，需 MSVC）：
 *   cl /O2 /LD sqlite3_hook.c /Fesqlite3_hook.dll
 *
 * 编译（Linux）：
 *   gcc -O2 -shared -fPIC sqlite3_hook.c -o libsqlite3_hook.so -ldl
 *
 * 运行依赖：系统 PATH / LD_LIBRARY_PATH 中存在 sqlite3 动态库。
 */

#define BUILDING_DLL
#include "sqlite3_hook.h"

#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <stdint.h>

#ifdef _WIN32
#include <windows.h>
#define sleep_ms(ms) Sleep(ms)
#define DLL_HANDLE HMODULE
#define LOAD_LIB(name) LoadLibraryA(name)
#define FIND_SYM(lib, name) GetProcAddress(lib, name)
#define FREE_LIB(lib) FreeLibrary(lib)
typedef HANDLE PipeFd;
#define INVALID_PIPE_FD INVALID_HANDLE_VALUE
#define PIPE_BUF_SIZE 4096
#else
#include <unistd.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <pthread.h>
#define sleep_ms(ms) usleep((ms) * 1000)
#define DLL_HANDLE void*
#define LOAD_LIB(name) dlopen(name, RTLD_LAZY | RTLD_LOCAL)
#define FIND_SYM(lib, name) dlsym(lib, name)
#define FREE_LIB(lib) dlclose(lib)
typedef int PipeFd;
#define INVALID_PIPE_FD (-1)
#define PIPE_BUF_SIZE 4096
#endif

/* ==================== SQLite 类型与常量声明（运行时绑定，无需 sqlite3.h）==================== */

typedef struct sqlite3 sqlite3;
typedef int64_t sqlite3_int64;

#define SQLITE_OK         0
#define SQLITE_ERROR      1
#define SQLITE_INSERT    18
#define SQLITE_UPDATE    23
#define SQLITE_DELETE     9

/* SQLite 回调函数类型 */
typedef void* (*sqlite3_update_hook_fn)(sqlite3*, void(*)(void*,int,char const*,char const*,sqlite3_int64), void*);
typedef int  (*sqlite3_open_fn)(const char*, sqlite3**);
typedef int  (*sqlite3_close_fn)(sqlite3*);
typedef int  (*sqlite3_exec_fn)(sqlite3*, const char*, int(*)(void*,int,char**,char**), void*, char**);
typedef const char* (*sqlite3_errmsg_fn)(sqlite3*);
typedef void (*sqlite3_free_fn)(void*);

/* ==================== 运行时 SQLite 函数表 ==================== */

typedef struct {
    DLL_HANDLE                 handle;
    sqlite3_open_fn            sqlite3_open;
    sqlite3_close_fn           sqlite3_close;
    sqlite3_exec_fn            sqlite3_exec;
    sqlite3_errmsg_fn          sqlite3_errmsg;
    sqlite3_free_fn            sqlite3_free;
    sqlite3_update_hook_fn     sqlite3_update_hook;
} SqliteRuntime;

/* 全局单例 — sqlite3 函数表，所有连接共用 */
static SqliteRuntime SQLITE = {0};

static int ensure_sqlite_loaded(void) {
    if (SQLITE.handle) return 0;

    const char *lib_names[] = {
#ifdef _WIN32
        "sqlite3.dll",
        "winsqlite3.dll"
#elif __APPLE__
        "libsqlite3.dylib",
        "/usr/lib/libsqlite3.dylib"
#else
        "libsqlite3.so",
        "libsqlite3.so.0"
#endif
    };

    for (int i = 0; i < (int)(sizeof(lib_names)/sizeof(lib_names[0])); i++) {
        DLL_HANDLE h = LOAD_LIB(lib_names[i]);
        if (h) {
            SQLITE.handle = h;
            SQLITE.sqlite3_open          = (sqlite3_open_fn)FIND_SYM(h, "sqlite3_open");
            SQLITE.sqlite3_close         = (sqlite3_close_fn)FIND_SYM(h, "sqlite3_close");
            SQLITE.sqlite3_exec          = (sqlite3_exec_fn)FIND_SYM(h, "sqlite3_exec");
            SQLITE.sqlite3_errmsg        = (sqlite3_errmsg_fn)FIND_SYM(h, "sqlite3_errmsg");
            SQLITE.sqlite3_free          = (sqlite3_free_fn)FIND_SYM(h, "sqlite3_free");
            SQLITE.sqlite3_update_hook   = (sqlite3_update_hook_fn)FIND_SYM(h, "sqlite3_update_hook");
            if (SQLITE.sqlite3_open && SQLITE.sqlite3_update_hook) return 0;
            FREE_LIB(h);
            memset(&SQLITE, 0, sizeof(SQLITE));
        }
    }
    fprintf(stderr, "[sqlite3_hook] 错误: 未找到 sqlite3 动态库\n");
    return -1;
}

/* ==================== 跨平台 pipe 辅助 ==================== */

static int pipe_create(PipeFd pipes[2]) {
#ifdef _WIN32
    if (!CreatePipe(&pipes[0], &pipes[1], NULL, PIPE_BUF_SIZE)) return -1;
    /* 设为无继承，避免句柄泄露 */
    SetHandleInformation(pipes[0], HANDLE_FLAG_INHERIT, 0);
    SetHandleInformation(pipes[1], HANDLE_FLAG_INHERIT, 0);
    return 0;
#else
    if (pipe(pipes) != 0) return -1;
    /* 设非阻塞写端，避免回调中写满阻塞 */
    int flags = fcntl(pipes[1], F_GETFL, 0);
    fcntl(pipes[1], F_SETFL, flags | O_NONBLOCK);
    /* 设非阻塞读端，Java 侧用 select 控制阻塞时长 */
    flags = fcntl(pipes[0], F_GETFL, 0);
    fcntl(pipes[0], F_SETFL, flags | O_NONBLOCK);
    return 0;
#endif
}

static void pipe_close_pipe(PipeFd fd) {
#ifdef _WIN32
    if (fd != INVALID_PIPE_FD) CloseHandle(fd);
#else
    if (fd >= 0) close(fd);
#endif
}

static int pipe_write_signal(PipeFd write_fd) {
#ifdef _WIN32
    DWORD written;
    return WriteFile(write_fd, "X", 1, &written, NULL) ? 0 : -1;
#else
    /* 非阻塞写，buffer 满则丢弃信号（事件已在环缓冲中） */
    return (write(write_fd, "X", 1) >= 0) ? 0 : 0;
#endif
}

/* ==================== 环形缓冲区 ==================== */

#define MAX_EVENTS 128
#define EVENT_BUF_SIZE 512

typedef struct {
    volatile int  has_event;
    char          data[EVENT_BUF_SIZE];
} HookEvent;

typedef struct {
    sqlite3   *db;
    HookEvent  events[MAX_EVENTS];
    volatile int write_idx;
    volatile int read_idx;
    volatile int active;
    /* pipe: [0]=read, [1]=write */
    PipeFd     pipe_fd[2];
} HookContext;

/* ==================== update_hook 回调 ==================== */

static void update_callback(void *ctx_ptr, int action, const char *db_name,
                            const char *table_name, sqlite3_int64 row_id)
{
    HookContext *ctx = (HookContext *)ctx_ptr;
    if (!ctx || !ctx->active) return;

    const char *type_str;
    switch (action) {
        case SQLITE_INSERT: type_str = "INSERT";  break;
        case SQLITE_UPDATE: type_str = "UPDATE";  break;
        case SQLITE_DELETE: type_str = "DELETE";  break;
        default:            type_str = "UNKNOWN"; break;
    }

    int wi = ctx->write_idx % MAX_EVENTS;
    snprintf(ctx->events[wi].data, EVENT_BUF_SIZE,
             "{\"type\":\"%s\",\"database\":\"%s\",\"table\":\"%s\",\"rowId\":%lld}",
             type_str, db_name ? db_name : "main", table_name ? table_name : "", (long long)row_id);
    ctx->events[wi].has_event = 1;
    ctx->write_idx = wi + 1;

    /* 通过 pipe 通知 Java 侧有事件可用（非阻塞，buffer 满则丢弃信号） */
    pipe_write_signal(ctx->pipe_fd[1]);
}

/* ==================== 导出 API ==================== */

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
    ctx->pipe_fd[0] = INVALID_PIPE_FD;
    ctx->pipe_fd[1] = INVALID_PIPE_FD;
    if (pipe_create(ctx->pipe_fd) != 0) {
        fprintf(stderr, "[sqlite3_hook] 创建 pipe 失败\n");
    }

    SQLITE.sqlite3_update_hook(ctx->db, update_callback, ctx);
    return ctx;
}

HOOK_API char* hook_poll(void *handle) {
    if (!handle) return NULL;
    HookContext *ctx = (HookContext *)handle;
    int ri = ctx->read_idx % MAX_EVENTS;
    if (!ctx->events[ri].has_event) return NULL;
    char *result = strdup(ctx->events[ri].data);
    ctx->events[ri].has_event = 0;
    ctx->read_idx = ri + 1;
    return result;
}

/**
 * 阻塞等待下一条变更事件。
 *
 * <p>在 OS 层面挂起：Windows 调用 WaitForSingleObject(pipe_read)，
 * Linux/macOS 调用 select()，仅在 C 侧 hook 写入 pipe 时才唤醒，无 CPU 轮询。</p>
 *
 * @param handle     hook_open 返回的句柄
 * @param timeout_ms 超时毫秒数（<=0 表示无限等待）
 * @return           事件 JSON 字符串（调用者需 free），超时或失败返回 NULL
 */
HOOK_API char* hook_wait(void *handle, int timeout_ms) {
    if (!handle) return NULL;
    HookContext *ctx = (HookContext *)handle;

    for (;;) {
        /* 先尝试非阻塞读取，避免不必要的阻塞 */
        char *event = hook_poll(handle);
        if (event) return event;

        if (ctx->pipe_fd[0] == INVALID_PIPE_FD) {
            /* pipe 不可用，退化为轮询 */
            if (timeout_ms > 0) {
                sleep_ms(10);
                static volatile int elapsed = 0;
                elapsed += 10;
                if (elapsed >= timeout_ms) return NULL;
            } else {
                sleep_ms(10);
            }
            continue;
        }

#ifdef _WIN32
        /* Windows: WaitForSingleObject 阻塞直到 pipe 有数据 */
        DWORD dw = WaitForSingleObject(ctx->pipe_fd[0],
                                       (timeout_ms > 0 && timeout_ms < 500) ? (DWORD)timeout_ms : 500);
        if (dw == WAIT_TIMEOUT) return NULL;
#else
        /* POSIX: select 阻塞直到 pipe 可读 */
        if (timeout_ms > 0) {
            struct timeval tv;
            tv.tv_sec  = timeout_ms / 1000;
            tv.tv_usec = (timeout_ms % 1000) * 1000;
            fd_set fds;
            FD_ZERO(&fds);
            FD_SET(ctx->pipe_fd[0], &fds);
            int ret = select(ctx->pipe_fd[0] + 1, &fds, NULL, NULL, &tv);
            if (ret <= 0) return NULL; /* timeout or error */
        } else {
            fd_set fds;
            FD_ZERO(&fds);
            FD_SET(ctx->pipe_fd[0], &fds);
            if (select(ctx->pipe_fd[0] + 1, &fds, NULL, NULL, NULL) <= 0) continue;
        }
        /* drain the signal byte */
        char buf[16];
        recv(ctx->pipe_fd[0], buf, sizeof(buf), 0);
#endif
    }
}

HOOK_API int hook_exec(void *handle, const char *sql) {
    if (!handle || !sql) return SQLITE_ERROR;
    HookContext *ctx = (HookContext *)handle;
    char *errmsg = NULL;
    int rc = SQLITE.sqlite3_exec(ctx->db, sql, NULL, NULL, &errmsg);
    if (rc != SQLITE_OK && errmsg) {
        fprintf(stderr, "[sqlite3_hook] SQL 错误: %s\n", errmsg);
        SQLITE.sqlite3_free(errmsg);
    }
    return rc;
}

HOOK_API void hook_free(void *ptr) {
    if (ptr) free(ptr);
}

/**
 * 关闭 pipe 读写端（不关闭数据库连接）。
 * 在 hook_close 中调用。
 */
HOOK_API void hook_pipe_close(void *handle) {
    if (!handle) return;
    HookContext *ctx = (HookContext *)handle;
    pipe_close_pipe(ctx->pipe_fd[0]);
    pipe_close_pipe(ctx->pipe_fd[1]);
    ctx->pipe_fd[0] = INVALID_PIPE_FD;
    ctx->pipe_fd[1] = INVALID_PIPE_FD;
}

HOOK_API void hook_close(void *handle) {
    if (!handle) return;
    HookContext *ctx = (HookContext *)handle;
    ctx->active = 0;
    hook_pipe_close(handle);
    SQLITE.sqlite3_update_hook(ctx->db, NULL, NULL);
    SQLITE.sqlite3_close(ctx->db);
    free(ctx);
}
