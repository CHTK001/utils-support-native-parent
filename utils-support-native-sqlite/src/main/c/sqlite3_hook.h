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

/**
 * 打开 SQLite 数据库并注册 sqlite3_update_hook 回调。
 *
 * 数据库打开后，所有通过该连接执行的 INSERT / UPDATE / DELETE
 * 操作均会触发回调，事件被序列化为 JSON 存入内部环形缓冲区。
 *
 * @param db_path  SQLite 数据库文件路径（UTF-8 编码）
 * @return         不透明句柄指针，失败返回 NULL
 */
HOOK_API void* hook_open(const char *db_path);

/**
 * 非阻塞轮询下一条变更事件。
 *
 * @param handle  hook_open 返回的句柄
 * @return        事件 JSON 字符串（调用者需 free），无事件返回 NULL
 *
 * JSON 格式：
 *   {"type":"INSERT|UPDATE|DELETE","database":"main","table":"users","rowId":42}
 */
HOOK_API char* hook_poll(void *handle);

/**
 * 阻塞等待下一条变更事件。
 *
 * @param handle     hook_open 返回的句柄
 * @param timeout_ms 超时毫秒数（<=0 表示无限等待）
 * @return           事件 JSON 字符串（调用者需 free），超时或失败返回 NULL
 */
HOOK_API char* hook_wait(void *handle, int timeout_ms);

/**
 * 释放由 hook_poll / hook_wait 返回的事件 JSON 字符串。
 *
 * 跨 CRT 安全释放，避免 Java FFI 侧直接调用 C 标准库 free()。
 *
 * @param ptr  hook_poll / hook_wait 返回的指针
 */
HOOK_API void hook_free(void *ptr);

/**
 * 通过已注册 update_hook 的连接执行 SQL。
 *
 * 只有通过此函数执行的 INSERT/UPDATE/DELETE 才会触发 update_hook 回调。
 *
 * @param handle hook_open 返回的句柄
 * @param sql    UTF-8 编码的 SQL 语句
 * @return       成功返回 0，失败返回 SQLite 错误码
 */
HOOK_API int hook_exec(void *handle, const char *sql);

/**
 * 关闭数据库连接并释放所有资源。
 *
 * 会注销 update_hook、关闭 sqlite3 连接、释放内部缓冲区。
 * 调用后 handle 不可再使用。
 *
 * @param handle  hook_open 返回的句柄
 */
HOOK_API void hook_close(void *handle);

#endif /* SQLITE3_HOOK_H */
