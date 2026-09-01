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
 * 同时创建跨平台 pipe（Windows: CreatePipe，POSIX: pipe()），
 * 用于向 Java FFI 侧发送事件通知。
 *
 * @param db_path  SQLite 数据库文件路径（UTF-8 编码）
 * @return         不透明句柄指针，失败返回 NULL
 */
HOOK_API void* hook_open(const char *db_path);

/**
 * 非阻塞轮询下一条变更事件，写入调用者提供的缓冲区。
 *
 * JSON 格式：
 *   {"type":"INSERT|UPDATE|DELETE","database":"main","table":"users","rowId":42}
 *
 * @param handle     hook_open 返回的句柄
 * @param buf        输出缓冲区（由调用者分配）
 * @param buf_size   缓冲区大小
 * @return           写入的字节数（不含null），0表示无事件
 */
HOOK_API int hook_poll(void *handle, char *buf, int buf_size);

/**
 * 阻塞等待下一条变更事件（OS 级别挂起，无 CPU 轮询）。
 *
 * <p>实现机制：</p>
 * <ul>
 *   <li>Windows: WaitForSingleObject(pipe_read_handle, timeout)</li>
 *   <li>Linux/macOS: select() on pipe read fd</li>
 *   <li>C 侧 hook 回调写入 pipe 信号字节后，阻塞立即解除</li>
 * </ul>
 *
 * @param handle     hook_open 返回的句柄
 * @param timeout_ms 超时毫秒数（<=0 表示无限等待）
 * @return           事件 JSON 字符串（调用者需 free），超时返回 NULL
 */
HOOK_API char* hook_wait(void *handle, int timeout_ms);

/**
 * 释放由 hook_poll / hook_wait 返回的事件 JSON 字符串。
 */
HOOK_API void hook_free(void *ptr);

/**
 * 通过已注册 update_hook 的连接执行 SQL。
 * 写操作触发 hook 回调，事件可通过 hook_pipe_read 捕获。
 *
 * @param handle hook_open 返回的句柄
 * @param sql    UTF-8 编码的 SQL 语句
 * @return       成功返回 0，失败返回 SQLite 错误码
 */
HOOK_API int hook_exec(void *handle, const char *sql);

/**
 * 关闭 pipe 读写端（不影响数据库连接）。
 * 在 hook_close 之前调用以清理 pipe 资源。
 */
HOOK_API void hook_pipe_close(void *handle);

/**
 * 关闭数据库连接、注销 update_hook、关闭 pipe、释放所有资源。
 * 调用后 handle 不可再使用。
 */
HOOK_API void hook_close(void *handle);

#endif /* SQLITE3_HOOK_H */
