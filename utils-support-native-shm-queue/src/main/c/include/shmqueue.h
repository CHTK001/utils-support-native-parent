#ifndef SHMQUEUE_H
#define SHMQUEUE_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* 导出宏：Windows 构建 DLL 时导出符号，其他平台为空 */
#if defined(_WIN32) && defined(SHMQUEUE_BUILDING_DLL)
  #define SHMQ_API __declspec(dllexport)
#elif defined(_WIN32)
  #define SHMQ_API __declspec(dllimport)
#else
  #define SHMQ_API
#endif

/* 魔术字 'SHMQ' 大端序：用于识别合法共享内存段 */
#define SHMQ_MAGIC   0x53484D51u
/* 版本 2：加入每槽 state 发布标记（CAS 无锁 MPSC），布局变化需升版本避免误兼容旧段 */
#define SHMQ_VERSION 2u

/* 队列等待/通知模式 */
enum shmq_wait_mode {
    SHMQ_WAIT_SPIN   = 0,
    SHMQ_WAIT_BLOCK  = 1,
    SHMQ_WAIT_HYBRID = 2
};

/* 错误码 */
#define SHMQ_OK                       0
#define SHMQ_ERR_INVALID_ARG         -1
#define SHMQ_ERR_NOMEM               -2
#define SHMQ_ERR_OPEN_SHM            -3
#define SHMQ_ERR_TRUNCATE            -4
#define SHMQ_ERR_MMAP                -5
#define SHMQ_ERR_HEADER_MAGIC        -6
#define SHMQ_ERR_HEADER_VERSION      -7
#define SHMQ_ERR_QUEUE_FULL          -8
#define SHMQ_ERR_DATA_TOO_LARGE      -9
#define SHMQ_ERR_WRITE_FD           -10
#define SHMQ_ERR_READ_FD            -11
#define SHMQ_ERR_TIMEOUT            -12
#define SHMQ_ERR_NOT_SUPPORTED      -13
#define SHMQ_ERR_DESTROYED          -14

/* 不透明上下文 */
typedef struct shm_queue_ctx shm_queue_ctx;

/* 默认参数 */
#define SHMQ_MIN_SLOT_SIZE     16
#define SHMQ_DEFAULT_SLOT_SIZE 1024
#define SHMQ_DEFAULT_CAPACITY  1024
#define SHMQ_DEFAULT_SPIN_NS   1000

SHMQ_API int shmq_create(const char *name,
                         size_t   shm_size,
                         uint32_t capacity,
                         uint32_t slot_size,
                         int      mode,
                         shm_queue_ctx **ctx_out);

SHMQ_API int shmq_attach(const char *name,
                         size_t   shm_size,
                         uint32_t capacity,
                         uint32_t slot_size,
                         int      mode,
                         shm_queue_ctx **ctx_out);

SHMQ_API int shmq_send(shm_queue_ctx *ctx,
                       uint32_t      msg_type,
                       const void   *data,
                       uint32_t      len);

SHMQ_API int shmq_recv(shm_queue_ctx *ctx,
                       uint32_t      *msg_type,
                       void          *buf,
                       uint32_t       buf_cap,
                       uint32_t      *len_out);

SHMQ_API int shmq_recv_timeout(shm_queue_ctx *ctx,
                               uint32_t      *msg_type,
                               void          *buf,
                               uint32_t       buf_cap,
                               uint32_t      *len_out,
                               uint64_t       timeout_ns);

SHMQ_API int shmq_get_notify_fd(shm_queue_ctx *ctx, int *fd);

#ifdef _WIN32
SHMQ_API int shmq_get_notify_handle(shm_queue_ctx *ctx, void **handle_out);
#endif

SHMQ_API int shmq_set_spin_ns(shm_queue_ctx *ctx, uint64_t spin_ns);

SHMQ_API int shmq_capacity(shm_queue_ctx *ctx, uint32_t *out);
SHMQ_API int shmq_slot_size(shm_queue_ctx *ctx, uint32_t *out);
SHMQ_API int shmq_mode(shm_queue_ctx *ctx, int *out);

/* Zero-copy 单缓冲直访：可直接读写同一块槽内内存（消除双环拷贝） */
SHMQ_API void *shmq_slot_ptr(shm_queue_ctx *ctx, uint32_t slot);
SHMQ_API uint32_t shmq_slot_state(shm_queue_ctx *ctx, uint32_t slot);
SHMQ_API void shmq_set_slot_state(shm_queue_ctx *ctx, uint32_t slot, uint32_t state);

SHMQ_API void shmq_destroy(shm_queue_ctx *ctx, int unlink);

SHMQ_API int shmq_unlink(const char *name);

SHMQ_API const char *shmq_strerror(int err);

#ifdef __cplusplus
}
#endif

#endif /* SHMQUEUE_H */
