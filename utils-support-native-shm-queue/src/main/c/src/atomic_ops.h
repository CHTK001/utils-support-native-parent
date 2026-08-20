#ifndef SHMQUEUE_ATOMIC_OPS_H
#define SHMQUEUE_ATOMIC_OPS_H

#include <stdint.h>

#ifdef _MSC_VER
  #include <intrin.h>

  typedef volatile uint32_t atomic_u32;

  static inline uint32_t shmq_atomic_load_acquire(atomic_u32 *p) {
      uint32_t v = *p;
      _ReadBarrier();
      return v;
  }

  static inline void shmq_atomic_store_release(atomic_u32 *p, uint32_t v) {
      _WriteBarrier();
      *p = v;
  }

  static inline uint32_t shmq_atomic_load_relaxed(atomic_u32 *p) {
      return *p;
  }

  static inline void shmq_atomic_store_relaxed(atomic_u32 *p, uint32_t v) {
      *p = v;
  }

  static inline uint32_t shmq_atomic_fetch_add_relaxed(atomic_u32 *p, uint32_t d) {
      return (uint32_t)_InterlockedExchangeAdd((volatile long *)p, (long)d);
  }

  static inline int shmq_atomic_cas_acqrel(atomic_u32 *p, uint32_t expect, uint32_t desired) {
      return _InterlockedCompareExchange((volatile long *)p, (long)desired, (long)expect) == (long)expect;
  }
#else
  #include <stdatomic.h>

  typedef _Atomic uint32_t atomic_u32;

  static inline uint32_t shmq_atomic_load_acquire(atomic_u32 *p) {
      return atomic_load_explicit(p, memory_order_acquire);
  }

  static inline void shmq_atomic_store_release(atomic_u32 *p, uint32_t v) {
      atomic_store_explicit(p, v, memory_order_release);
  }

  static inline uint32_t shmq_atomic_load_relaxed(atomic_u32 *p) {
      return atomic_load_explicit(p, memory_order_relaxed);
  }

  static inline void shmq_atomic_store_relaxed(atomic_u32 *p, uint32_t v) {
      atomic_store_explicit(p, v, memory_order_relaxed);
  }

  static inline uint32_t shmq_atomic_fetch_add_relaxed(atomic_u32 *p, uint32_t d) {
      return atomic_fetch_add_explicit(p, d, memory_order_relaxed);
  }

  static inline int shmq_atomic_cas_acqrel(atomic_u32 *p, uint32_t expect, uint32_t desired) {
      return atomic_compare_exchange_weak_explicit(p, &expect, desired,
                                                   memory_order_acq_rel, memory_order_acquire);
  }
#endif

#endif /* SHMQUEUE_ATOMIC_OPS_H */
