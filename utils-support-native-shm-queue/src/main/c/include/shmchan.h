#ifndef SHMCHAN_H
#define SHMCHAN_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#if defined(_WIN32) && defined(SHMQUEUE_BUILDING_DLL)
  #define SHMCHAN_API __declspec(dllexport)
#elif defined(_WIN32)
  #define SHMCHAN_API __declspec(dllimport)
#else
  #define SHMCHAN_API
#endif

#define SHMCHAN_MAGIC   0x534E4348u
#define SHMCHAN_VERSION 1u

#define SHMCHAN_STATE_EMPTY 0u
#define SHMCHAN_STATE_REQ   1u
#define SHMCHAN_STATE_RESP  2u

typedef struct shm_chan_ctx shm_chan_ctx;

SHMCHAN_API int shmc_create(const char *name, size_t shm_size,
                            uint32_t capacity, uint32_t slot_size,
                            shm_chan_ctx **out);
SHMCHAN_API int shmc_attach(const char *name, size_t shm_size, shm_chan_ctx **out);

SHMCHAN_API int shmc_acquire_empty(shm_chan_ctx *ctx, uint32_t *slot, void **ptr);
SHMCHAN_API int shmc_commit_req(shm_chan_ctx *ctx, uint32_t slot, uint32_t len);
SHMCHAN_API int shmc_poll_req(shm_chan_ctx *ctx, uint32_t *slot, void **ptr, uint32_t *len,
                              uint64_t timeout_ns);
SHMCHAN_API int shmc_commit_resp(shm_chan_ctx *ctx, uint32_t slot, uint32_t len);
SHMCHAN_API int shmc_poll_resp(shm_chan_ctx *ctx, uint32_t *slot, void **ptr, uint32_t *len,
                               uint64_t timeout_ns);
SHMCHAN_API void shmc_release(shm_chan_ctx *ctx, uint32_t slot);

SHMCHAN_API void *shmc_slot_ptr(shm_chan_ctx *ctx, uint32_t slot);
SHMCHAN_API uint32_t shmc_slot_state(shm_chan_ctx *ctx, uint32_t slot);
SHMCHAN_API uint32_t shmc_capacity(shm_chan_ctx *ctx);
SHMCHAN_API uint32_t shmc_slot_size(shm_chan_ctx *ctx);
SHMCHAN_API void shmc_destroy(shm_chan_ctx *ctx, int unlink);
SHMCHAN_API const char *shmc_strerror(int err);

#ifdef __cplusplus
}
#endif

#endif
