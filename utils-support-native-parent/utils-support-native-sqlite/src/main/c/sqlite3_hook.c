/**
 * sqlite3_hook — SQLite update_hook 动态库
 *
 * 基于 sqlite3_update_hook() 回调机制，实时捕获 INSERT / UPDATE / DELETE 数据变更。
 * 变更事件以 JSON 格式写入环形缓冲区，供 Java FFI 侧轮询消费。
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
#else
#include <unistd.h>
#include <dlfcn.h>
#define sleep_ms(ms) usleep((ms) * 1000)
#define DLL_HANDLE void*
#define LOAD_LIB(name) dlopen(name, RTLD_LAZY | RTLD_LOCAL)
#define FIND_SYM(lib, name) dlsym(lib, name)
#define FREE_LIB(lib) dlclose(lib)
#endif

/* ==================== SQLite 类型与常量声明（运行时绑定，无需 sqlite3.h）==================== */

typedef struct sqlite3 sqlite3;
typedef int64_t sqlite3_int64;

#define SQLITE_OK         0
#define SQLITE_ERROR      1
#define SQLITE_INSERT    18
#define SQLITE_UPDATE    23
#define SQLITE_DELETE     9

/* SQLite 回调函数类型 — 返回类型必须匹配 sqlite3.h */
typedef void* (*sqlite3_update_hook_fn)(sqlite3*, void(*)(void*,int,char const*,char const*,sqlite3_int64), void*);
typedef int  (*sqlite3_open_fn)(const char*, sqlite3**);
typedef int  (*sqlite3_close_fn)(sqlite3*);
typedef int  (*sqlite3_exec_fn)(sqlite3*, const char*, int(*)(void*,int,char**,char**), void*, char**);
typedef const char* (*sqlite3_errmsg_fn)(sqlite3*);
typedef void (*sqlite3_free_fn)(void*);

/* ==================== 运行时 SQLite 函数表 ==================== */

typedef struct {
    DLL_HANDLE           handle;
    sqlite3_open_fn      sqlite3_open;
    sqlite3_close_fn     sqlite3_close;
    sqlite3_exec_fn      sqlite3_exec;
    sqlite3_errmsg_fn    sqlite3_errmsg;
    sqlite3_free_fn      sqlite3_free;
    sqlite3_update_hook_fn sqlite3_update_hook;
} SqliteRuntime;

/* 全局单例 — sqlite3 函数表，所有连接共用 */
static SqliteRuntime SQLITE = {0};

/**
 * 加载 sqlite3 动态库并解析全部所需符号。
 * 线程安全（首次调用后不再修改）。
 *
 * @return 0 成功，-1 失败
 */
static int ensure_sqlite_loaded(void) {
    if (SQLITE.handle) return 0;

    const char *lib_names[] = {
#ifdef _WIN32
        "sqlite3.dll",
        "winsqlite3.dll"   // Windows 10+ 内置
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
            SQLITE.sqlite3_open        = (sqlite3_open_fn)FIND_SYM(h, "sqlite3_open");
            SQLITE.sqlite3_close       = (sqlite3_close_fn)FIND_SYM(h, "sqlite3_close");
            SQLITE.sqlite3_exec        = (sqlite3_exec_fn)FIND_SYM(h, "sqlite3_exec");
            SQLITE.sqlite3_errmsg      = (sqlite3_errmsg_fn)FIND_SYM(h, "sqlite3_errmsg");
            SQLITE.sqlite3_free        = (sqlite3_free_fn)FIND_SYM(h, "sqlite3_free");
            SQLITE.sqlite3_update_hook = (sqlite3_update_hook_fn)FIND_SYM(h, "sqlite3_update_hook");

                if (SQLITE.sqlite3_open && SQLITE.sqlite3_update_hook) {
                    return 0;
                }
            FREE_LIB(h);
            memset(&SQLITE, 0, sizeof(SQLITE));
        }
    }
    fprintf(stderr, "[sqlite3_hook] 错误: 未找到 sqlite3 动态库\n");
    return -1;
}

/* ==================== 环形缓冲区 ==================== */

#define MAX_EVENTS 128
#define EVENT_BUF_SIZE 512

/** 单条事件记录 */
typedef struct {
    volatile int  has_event;
    char data[EVENT_BUF_SIZE];
} HookEvent;

/** 环形缓冲区上下文 */
typedef struct {
    sqlite3   *db;
    HookEvent  events[MAX_EVENTS];
    volatile int write_idx;
    volatile int read_idx;
    volatile int active;
} HookContext;

/* ==================== update_hook 回调 ==================== */

/**
 * sqlite3_update_hook 回调函数。
 * 将变更事件序列化为 JSON 写入环形缓冲区。
 */
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
             type_str,
             db_name ? db_name : "main",
             table_name ? table_name : "",
             (long long)row_id);
    ctx->events[wi].has_event = 1;
    ctx->write_idx = wi + 1;
}

/* ==================== 导出 API ==================== */

/**
 * 打开 SQLite 数据库并注册 update_hook。
 *
 * @param db_path  数据库文件路径（UTF-8）
 * @return         不透明句柄指针，失败返回 NULL
 */
HOOK_API void* hook_open(const char *db_path)
{
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

/**
 * 读取下一条事件（非阻塞）。
 *
 * @param handle   hook_open 返回的句柄
 * @return         事件 JSON 字符串（调用者需 free），无事件返回 NULL
 */
HOOK_API char* hook_poll(void *handle)
{
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
 * 读取下一条事件（阻塞，带超时）。
 *
 * @param handle     hook_open 返回的句柄
 * @param timeout_ms 超时毫秒数（<=0 表示无限等待）
 * @return           事件 JSON 字符串（调用者需 free），超时返回 NULL
 */
HOOK_API char* hook_wait(void *handle, int timeout_ms)
{
    if (!handle) return NULL;

    HookContext *ctx = (HookContext *)handle;
    int elapsed = 0;

    for (;;) {
        char *event = hook_poll(handle);
        if (event) return event;

        if (timeout_ms > 0 && elapsed >= timeout_ms) return NULL;
        sleep_ms(10);
        elapsed += 10;
    }
}

/**
 * 通过已注册 update_hook 的连接执行 SQL。
 *
 * 只有通过此函数执行的 INSERT/UPDATE/DELETE 才会触发 update_hook 回调，
 * 从而被 hook_poll/hook_wait 捕获。
 *
 * @param handle hook_open 返回的句柄
 * @param sql    UTF-8 编码的 SQL 语句
 * @return       成功返回 SQLITE_OK(0)，失败返回 SQLite 错误码
 */
HOOK_API int hook_exec(void *handle, const char *sql)
{
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

/**
 * 释放由 hook_poll / hook_wait 返回的 JSON 字符串。
 *
 * 用于跨 CRT 安全释放，Java FFI 侧应调用此函数而非 C 标准库 free()。
 *
 * @param ptr  hook_poll / hook_wait 返回的字符串指针
 */
HOOK_API void hook_free(void *ptr)
{
    if (ptr) free(ptr);
}

/**
 * 关闭数据库连接并释放资源。
 *
 * @param handle  hook_open 返回的句柄
 */
HOOK_API void hook_close(void *handle)
{
    if (!handle) return;

    HookContext *ctx = (HookContext *)handle;
    ctx->active = 0;
    SQLITE.sqlite3_update_hook(ctx->db, NULL, NULL);
    SQLITE.sqlite3_close(ctx->db);
    free(ctx);
}
