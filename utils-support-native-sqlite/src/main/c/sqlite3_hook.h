#ifndef SQLITE3_HOOK_H
#define SQLITE3_HOOK_H

#ifdef _WIN32
  #ifdef BUILDING_DLL
    #define HOOK_API __declspec(dllexport)
  #else
    #define HOOK_API __declspec(dllimport)
  #endif
#else
  #define HOOK_API __attribute__((visibility("default")))
#endif

/* ──────────────────────────────────────────────────────────────
 *  回调类型（真响应式核心）
 * ────────────────────────────────────────────────────────────── */

/**
 * 变更事件回调函数类型。
 * 被 OS 异步 I/O 完成时直接调用，无专用线程。
 *
 * @param json      JSON 格式的事件数据（UTF-8）
 * @param user_data 用户自定义指针
 */
typedef void (*hook_event_fn)(const char *json, void *user_data);

/**
 * 异步上下文句柄（内部结构，不透明）。
 */
typedef struct HookAsyncContext HookAsyncContext;

/* ──────────────────────────────────────────────────────────────
 *  同步 API（向后兼容）
 * ────────────────────────────────────────────────────────────── */

/**
 * 打开 SQLite 数据库并注册 sqlite3_update_hook 回调。
 * @param db_path  SQLite 数据库文件路径（UTF-8 编码）
 * @return         不透明句柄指针，失败返回 NULL
 */
HOOK_API void* hook_open(const char *db_path);

/**
 * 阻塞等待下一条变更事件，写入调用者提供的缓冲区。
 * @param handle     hook_open 返回的句柄
 * @param buf        输出缓冲区（由调用者分配）
 * @param buf_size   缓冲区大小
 * @param timeout_ms 超时毫秒（<=0 表示无限等待）
 * @return           写入字节数，超时返回 0，失败返回 -1
 */
HOOK_API int hook_wait(void *handle, char *buf, int buf_size, int timeout_ms);

/**
 * 非阻塞轮询事件（无事件时立即返回 0）。
 */
HOOK_API int hook_poll(void *handle, char *buf, int buf_size);

/**
 * 通过已注册 update_hook 的连接执行 SQL。
 * @param handle hook_open 返回的句柄
 * @param sql    UTF-8 编码的 SQL 语句
 * @return       成功返回 0，失败返回 SQLite 错误码
 */
HOOK_API int hook_exec(void *handle, const char *sql);

/**
 * 关闭同步句柄、注销 update_hook、关闭 pipe、释放所有资源。
 */
HOOK_API void hook_close(void *handle);

/* ──────────────────────────────────────────────────────────────
 *  异步 API（真响应式）
 *
 *  特点：
 *    - 零专用线程：使用 OS 原生异步 I/O（IOCP / io_uring）
 *    - 零轮询：事件通过回调直接推送
 *    - 背压：回调返回非零值可丢弃事件
 * ────────────────────────────────────────────────────────────── */

/**
 * 以异步模式打开 SQLite 数据库。
 *
 * @param db_path  SQLite 数据库文件路径（UTF-8 编码）
 * @param on_event 变更事件回调（被 OS 异步完成时调用）
 * @param user_data 透传给回调的用户指针
 * @return         异步上下文句柄，失败返回 NULL
 */
HOOK_API void* hook_open_async(const char *db_path, hook_event_fn on_event, void *user_data);

/**
 * 通过异步句柄执行 SQL（非阻塞）。
 * @param handle hook_open_async 返回的句柄
 * @param sql    UTF-8 编码的 SQL 语句
 * @return       成功返回 0，失败返回 SQLite 错误码
 */
HOOK_API int hook_exec_async(void *handle, const char *sql);

/**
 * 关闭异步句柄、释放所有资源（IOCP/io_uring/内存）。
 */
HOOK_API void hook_close_async(void *handle);

#endif
