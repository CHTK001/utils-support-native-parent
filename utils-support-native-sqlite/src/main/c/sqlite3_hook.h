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
 * @param db_path  SQLite 数据库文件路径（UTF-8 编码）
 * @return         不透明句柄指针，失败返回 NULL
 */
HOOK_API void* hook_open(const char *db_path);

/**
 * 阻塞等待下一条变更事件，写入调用者提供的缓冲区。
 * OS 级挂起（Windows: WaitForSingleObject / Linux: select），无 CPU 轮询。
 *
 * @param handle     hook_open 返回的句柄
 * @param buf        输出缓冲区（由调用者分配）
 * @param buf_size   缓冲区大小
 * @param timeout_ms 超时毫秒（<=0 表示无限等待）
 * @return           写入字节数，超时返回 0，失败返回 -1
 */
HOOK_API int hook_wait(void *handle, char *buf, int buf_size, int timeout_ms);

/**
 * 通过已注册 update_hook 的连接执行 SQL。
 * @param handle hook_open 返回的句柄
 * @param sql    UTF-8 编码的 SQL 语句
 * @return       成功返回 0，失败返回 SQLite 错误码
 */
HOOK_API int hook_exec(void *handle, const char *sql);

/**
 * 关闭数据库连接、注销 update_hook、关闭 pipe、释放所有资源。
 */
HOOK_API void hook_close(void *handle);

#endif
