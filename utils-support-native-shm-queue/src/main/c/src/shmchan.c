#include "shmchan.h"
#include "atomic_ops.h"
#include "shmqueue.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <errno.h>
#ifdef _WIN32
  #include <windows.h>
  typedef HANDLE shm_handle_t;
  typedef HANDLE notify_handle_t;
  #define INVALID_H ((HANDLE)(intptr_t)-1)
  #define close_h(h) CloseHandle(h)
#else
  #include <fcntl.h>
  #include <unistd.h>
  #include <sys/mman.h>
  #include <sys/stat.h>
  #include <sys/eventfd.h>
  #include <poll.h>
  #include <time.h>
  #if defined(__i386__) || defined(__x86_64__)
    #include <immintrin.h>
  #endif
  typedef int shm_handle_t;
  typedef int notify_handle_t;
  #define INVALID_H (-1)
  #define close_h(h) do { if ((h)>=0) ::close(h); } while(0)
#endif
#define HEADER_SIZE 64
#define NAME_LEN 32
#define ACQUIRING 3u
struct shm_chan_hdr { uint32_t magic; uint32_t version; uint32_t capacity; uint32_t slot_size; uint32_t _pad[4]; char notify_name[NAME_LEN]; };
struct shm_chan_ctx { char name[256]; size_t size; uint32_t capacity; uint32_t slot_size; shm_handle_t shm_fd; notify_handle_t notify_fd; void *map_addr; struct shm_chan_hdr *hdr; atomic_u32 *state; uint8_t *slots; int is_creator; };
static size_t state_off(void){ return (HEADER_SIZE+7u)&~(size_t)7u; }
static size_t total_sz(uint32_t cap,uint32_t ss){ return state_off() + (size_t)cap*4u + (size_t)cap*ss; }
static inline uint8_t* slot_ptr(shm_chan_ctx *c,uint32_t s){ return c->slots + (size_t)s*c->slot_size; }
static int open_or_create(const char *name,size_t sz,int create,shm_handle_t *out){
#ifdef _WIN32
 DWORD hi=(DWORD)(sz>>32),lo=(DWORD)(sz&0xFFFFFFFFu);
 HANDLE h=CreateFileMappingA(INVALID_HANDLE_VALUE,NULL,PAGE_READWRITE,hi,lo,name);
 if(!h) return SHMQ_ERR_OPEN_SHM;
 if(!create){ if(GetLastError()!=ERROR_ALREADY_EXISTS){CloseHandle(h);return SHMQ_ERR_OPEN_SHM;} *out=h;return SHMQ_OK; }
 *out=h;return SHMQ_OK;
#else
 int fd;
 if(create){ fd=shm_open(name,O_RDWR|O_CREAT|O_EXCL,0600); if(fd<0){ if(errno==EEXIST){ fd=shm_open(name,O_RDWR,0600); if(fd<0) return SHMQ_ERR_OPEN_SHM; *out=fd;return SHMQ_OK;} return SHMQ_ERR_OPEN_SHM;} if(ftruncate(fd,(off_t)sz)!=0){::close(fd);shm_unlink(name);return SHMQ_ERR_TRUNCATE;}}
 else{ fd=shm_open(name,O_RDWR,0600); if(fd<0) return SHMQ_ERR_OPEN_SHM; }
 *out=fd;return SHMQ_OK;
#endif
}
static int map_shm(shm_handle_t fd,size_t sz,void **a){
#ifdef _WIN32
 (void)fd; void *p=MapViewOfFile(fd,FILE_MAP_ALL_ACCESS,0,0,sz); if(!p) return SHMQ_ERR_MMAP; *a=p;return SHMQ_OK;
#else
 void *p=mmap(NULL,sz,PROT_READ|PROT_WRITE,MAP_SHARED,fd,0); if(p==MAP_FAILED) return SHMQ_ERR_MMAP; *a=p;return SHMQ_OK;
#endif
}
static void unmap_shm(void *a,size_t sz){
#ifdef _WIN32
 (void)sz; UnmapViewOfFile(a);
#else
 munmap(a,sz);
#endif
}
static int create_notify(const char *name,int create,notify_handle_t *out){
#ifdef _WIN32
 (void)create; char n[256]; snprintf(n,sizeof(n),"Local\\shmc_%s",name); for(size_t i=8;i<strlen(n);++i) if(n[i]=='/'||n[i]=='\\') n[i]='_';
 HANDLE h=CreateEventA(NULL,FALSE,FALSE,n); if(!h) return SHMQ_ERR_OPEN_SHM; *out=h;return SHMQ_OK;
#else
 (void)name;(void)create; int fd=eventfd(0,EFD_NONBLOCK|EFD_CLOEXEC); if(fd<0) return SHMQ_ERR_OPEN_SHM; *out=fd;return SHMQ_OK;
#endif
}
static int notify_wake(notify_handle_t fd){
#ifdef _WIN32
  return SetEvent(fd)?SHMQ_OK:SHMQ_ERR_WRITE_FD;
#else
 uint64_t one=1; return (write(fd,&one,sizeof(one))==(ssize_t)sizeof(one))?SHMQ_OK:SHMQ_ERR_WRITE_FD;
#endif
}
static int notify_wait(notify_handle_t fd,uint64_t ns){
#ifdef _WIN32
  /* Windows 定时器默认节拍 10~15.6ms，WaitForSingleObject 小超时会被拉长，
     造成 SHM 轮询固定延迟。改为 0 超时探测 + QPC 自旋，保证亚毫秒级响应。 */
  DWORD r=WaitForSingleObject(fd,0);
  if(r==WAIT_OBJECT_0) return SHMQ_OK;
  if(ns==0) return SHMQ_ERR_TIMEOUT;
  LARGE_INTEGER f,s,n; QueryPerformanceFrequency(&f); QueryPerformanceCounter(&s);
  for(;;){
    r=WaitForSingleObject(fd,0);
    if(r==WAIT_OBJECT_0) return SHMQ_OK;
    QueryPerformanceCounter(&n);
    uint64_t el=(uint64_t)((uint64_t)(n.QuadPart-s.QuadPart)*1000000000ull/(uint64_t)f.QuadPart);
    if(el>=ns) return SHMQ_ERR_TIMEOUT;
    SwitchToThread();
  }
#else
 struct pollfd p={fd,POLLIN,0}; int ms=(int)(ns/1000000ull); if(ms==0&&ns>0) ms=1;
 int r; do{r=poll(&p,1,ms);}while(r<0&&errno==EINTR); if(r==0) return SHMQ_ERR_TIMEOUT; if(r<0) return SHMQ_ERR_READ_FD;
 uint64_t v; ssize_t n; do{n=read(fd,&v,sizeof(v));}while(n<0&&errno==EINTR); return SHMQ_OK;
#endif
}
static inline void cpu_pause(void){
#ifdef _WIN32
 YieldProcessor();
#else
#if defined(__i386__)||defined(__x86_64__)
 _mm_pause();
#else
 __asm__ __volatile__("":::"memory");
#endif
#endif
}
int shmc_create(const char *name,size_t shm_size,uint32_t cap,uint32_t ss,shm_chan_ctx **out){
 if(!name||!out) return SHMQ_ERR_INVALID_ARG; if(cap<4) return SHMQ_ERR_INVALID_ARG; if(ss<64) return SHMQ_ERR_INVALID_ARG;
 if(shm_size==0) shm_size=total_sz(cap,ss);
 shm_chan_ctx *c=(shm_chan_ctx*)calloc(1,sizeof(*c)); if(!c) return SHMQ_ERR_NOMEM;
 c->shm_fd=INVALID_H; c->notify_fd=INVALID_H; c->capacity=cap; c->slot_size=ss; c->size=shm_size; c->is_creator=1;
 snprintf(c->name,sizeof(c->name),"%s",name);
 int rc=open_or_create(name,shm_size,1,&c->shm_fd);
 if(rc!=SHMQ_OK){ rc=open_or_create(name,shm_size,0,&c->shm_fd); if(rc!=SHMQ_OK){free(c);return rc;} c->is_creator=0; }
 rc=map_shm(c->shm_fd,shm_size,&c->map_addr); if(rc!=SHMQ_OK){close_h(c->shm_fd);free(c);return rc;}
 c->hdr=(struct shm_chan_hdr*)c->map_addr; c->state=(atomic_u32*)((uint8_t*)c->map_addr+state_off()); c->slots=(uint8_t*)c->state+(size_t)cap*4u;
 uint32_t magic=shmq_atomic_load_relaxed((atomic_u32*)&c->hdr->magic);
 if(magic==SHMCHAN_MAGIC){ if(c->hdr->version!=SHMCHAN_VERSION){unmap_shm(c->map_addr,shm_size);close_h(c->shm_fd);free(c);return SHMQ_ERR_HEADER_VERSION;}
  if(c->hdr->capacity!=cap||c->hdr->slot_size!=ss){unmap_shm(c->map_addr,shm_size);close_h(c->shm_fd);free(c);return SHMQ_ERR_INVALID_ARG;} c->is_creator=0;
 } else if(magic==0){ c->hdr->magic=SHMCHAN_MAGIC; c->hdr->version=SHMCHAN_VERSION; c->hdr->capacity=cap; c->hdr->slot_size=ss;
  for(uint32_t i=0;i<cap;++i) shmq_atomic_store_relaxed(&c->state[i],SHMCHAN_STATE_EMPTY);
 } else { unmap_shm(c->map_addr,shm_size);close_h(c->shm_fd);free(c);return SHMQ_ERR_HEADER_MAGIC; }
 rc=create_notify(name,c->is_creator,&c->notify_fd); if(rc!=SHMQ_OK){unmap_shm(c->map_addr,shm_size);close_h(c->shm_fd);free(c);return rc;}
 *out=c; return SHMQ_OK;
}
int shmc_attach(const char *name,size_t shm_size,shm_chan_ctx **out){
 if(!name||!out) return SHMQ_ERR_INVALID_ARG; (void)shm_size;
 shm_chan_ctx *c=(shm_chan_ctx*)calloc(1,sizeof(*c)); if(!c) return SHMQ_ERR_NOMEM;
 c->shm_fd=INVALID_H; c->notify_fd=INVALID_H; snprintf(c->name,sizeof(c->name),"%s",name);
 int rc=open_or_create(name,HEADER_SIZE,0,&c->shm_fd); if(rc!=SHMQ_OK){free(c);return rc;}
 void *hm=NULL; rc=map_shm(c->shm_fd,HEADER_SIZE,&hm); if(rc!=SHMQ_OK){close_h(c->shm_fd);free(c);return rc;}
 struct shm_chan_hdr *h=(struct shm_chan_hdr*)hm;
 if(shmq_atomic_load_relaxed((atomic_u32*)&h->magic)!=SHMCHAN_MAGIC){unmap_shm(hm,HEADER_SIZE);close_h(c->shm_fd);free(c);return SHMQ_ERR_HEADER_MAGIC;}
 if(h->version!=SHMCHAN_VERSION){unmap_shm(hm,HEADER_SIZE);close_h(c->shm_fd);free(c);return SHMQ_ERR_HEADER_VERSION;}
 c->capacity=h->capacity; c->slot_size=h->slot_size; size_t real=total_sz(c->capacity,c->slot_size); c->size=real;
 unmap_shm(hm,HEADER_SIZE);
 rc=map_shm(c->shm_fd,real,&c->map_addr); if(rc!=SHMQ_OK){close_h(c->shm_fd);free(c);return rc;}
 c->hdr=(struct shm_chan_hdr*)c->map_addr; c->state=(atomic_u32*)((uint8_t*)c->map_addr+state_off()); c->slots=(uint8_t*)c->state+(size_t)c->capacity*4u;
 rc=create_notify(name,0,&c->notify_fd); if(rc!=SHMQ_OK){unmap_shm(c->map_addr,real);close_h(c->shm_fd);free(c);return rc;}
 *out=c; return SHMQ_OK;
}
int shmc_acquire_empty(shm_chan_ctx *c,uint32_t *slot,void **ptr){
 if(!c||!slot||!ptr) return SHMQ_ERR_INVALID_ARG;
 for(uint32_t i=0;i<c->capacity;++i){ uint32_t st=shmq_atomic_load_acquire(&c->state[i]); if(st==SHMCHAN_STATE_EMPTY){ if(shmq_atomic_cas_acqrel(&c->state[i],SHMCHAN_STATE_EMPTY,ACQUIRING)){ *slot=i; *ptr=slot_ptr(c,i); return SHMQ_OK; } } }
 return SHMQ_ERR_QUEUE_FULL;
}
int shmc_commit_req(shm_chan_ctx *c,uint32_t slot,uint32_t len){
 if(!c||slot>=c->capacity) return SHMQ_ERR_INVALID_ARG; if(len>c->slot_size) return SHMQ_ERR_DATA_TOO_LARGE;
 uint8_t *p=slot_ptr(c,slot); memcpy(p,&len,4);
 shmq_atomic_store_release(&c->state[slot],SHMCHAN_STATE_REQ);
 (void)notify_wake(c->notify_fd); return SHMQ_OK;
}
int shmc_poll_req(shm_chan_ctx *c,uint32_t *slot,void **ptr,uint32_t *len,uint64_t to){
  if(!c||!slot||!ptr||!len) return SHMQ_ERR_INVALID_ARG;
  uint64_t start=0;
#ifdef _WIN32
  LARGE_INTEGER f,s; QueryPerformanceFrequency(&f); QueryPerformanceCounter(&s); start=s.QuadPart;
#else
  struct timespec ts; clock_gettime(CLOCK_MONOTONIC,&ts); start=(uint64_t)ts.tv_sec*1000000000ull+ts.tv_nsec;
#endif
  while(1){
  for(uint32_t i=0;i<c->capacity;++i){ if(shmq_atomic_load_acquire(&c->state[i])==SHMCHAN_STATE_REQ){ if(!shmq_atomic_cas_acqrel(&c->state[i],SHMCHAN_STATE_REQ,SHMCHAN_STATE_POLLED)) continue; uint8_t *p=slot_ptr(c,i); uint32_t l; memcpy(&l,p,4); *slot=i; *ptr=p+4; *len=l; return SHMQ_OK; } }
#ifdef _WIN32
  /* 纯扫描自旋：不依赖 event/定时器，cpu_pause 短暂退避保证亚毫秒发现 */
  { LARGE_INTEGER n; for(int k=0;k<200;++k){ cpu_pause(); } QueryPerformanceCounter(&n); uint64_t elapsed=(uint64_t)(((n.QuadPart-s.QuadPart)*1000000000ull)/(uint64_t)f.QuadPart); if(to!=0&&elapsed>=to) return SHMQ_ERR_TIMEOUT; }
#else
  uint64_t now=0;
  struct timespec n; clock_gettime(CLOCK_MONOTONIC,&n); now=(uint64_t)n.tv_sec*1000000000ull+n.tv_nsec;
  uint64_t elapsed=now-start;
  if(to!=0 && elapsed>=to) return SHMQ_ERR_TIMEOUT;
  uint64_t remain= to==0? 2000000ull : (to>elapsed? to-elapsed:0);
  if(remain>2000000ull) remain=2000000ull;
  int rc=notify_wait(c->notify_fd,remain);
  if(rc==SHMQ_ERR_TIMEOUT && to!=0 && elapsed+remain>=to) return SHMQ_ERR_TIMEOUT;
#endif
  }
}
int shmc_commit_resp(shm_chan_ctx *c,uint32_t slot,uint32_t len){
 if(!c||slot>=c->capacity) return SHMQ_ERR_INVALID_ARG;
 uint8_t *p=slot_ptr(c,slot); memcpy(p,&len,4);
 shmq_atomic_store_release(&c->state[slot],SHMCHAN_STATE_RESP);
 (void)notify_wake(c->notify_fd); return SHMQ_OK;
}
int shmc_poll_resp(shm_chan_ctx *c,uint32_t *slot,void **ptr,uint32_t *len,uint64_t to){
 if(!c||!slot||!ptr||!len) return SHMQ_ERR_INVALID_ARG;
 uint64_t start=0;
#ifdef _WIN32
 LARGE_INTEGER f,s; QueryPerformanceFrequency(&f); QueryPerformanceCounter(&s); start=s.QuadPart;
#else
 struct timespec ts; clock_gettime(CLOCK_MONOTONIC,&ts); start=(uint64_t)ts.tv_sec*1000000000ull+ts.tv_nsec;
#endif
 while(1){
  for(uint32_t i=0;i<c->capacity;++i){ if(shmq_atomic_load_acquire(&c->state[i])==SHMCHAN_STATE_RESP){ uint8_t *p=slot_ptr(c,i); uint32_t l; memcpy(&l,p,4); *slot=i; *ptr=p+4; *len=l; return SHMQ_OK; } }
  uint64_t now=0;
#ifdef _WIN32
  LARGE_INTEGER n; QueryPerformanceCounter(&n); now=n.QuadPart;
  uint64_t elapsed=(uint64_t)(((now-start)*1000000000ull)/(uint64_t)f.QuadPart);
#else
  struct timespec n; clock_gettime(CLOCK_MONOTONIC,&n); now=(uint64_t)n.tv_sec*1000000000ull+n.tv_nsec;
  uint64_t elapsed=now-start;
#endif
  if(to!=0 && elapsed>=to) return SHMQ_ERR_TIMEOUT;
  uint64_t remain= to==0? 2000000ull : (to>elapsed? to-elapsed:0);
  if(remain>2000000ull) remain=2000000ull;
  int rc=notify_wait(c->notify_fd,remain);
  if(rc==SHMQ_ERR_TIMEOUT && to!=0 && elapsed+remain>=to) return SHMQ_ERR_TIMEOUT;
 }
}
void shmc_release(shm_chan_ctx *c,uint32_t slot){ if(!c||slot>=c->capacity) return; shmq_atomic_store_release(&c->state[slot],SHMCHAN_STATE_EMPTY); (void)notify_wake(c->notify_fd); }
void *shmc_slot_ptr(shm_chan_ctx *c,uint32_t slot){ if(!c||slot>=c->capacity) return NULL; return slot_ptr(c,slot); }
uint32_t shmc_slot_state(shm_chan_ctx *c,uint32_t slot){ if(!c||slot>=c->capacity) return SHMCHAN_STATE_EMPTY; return shmq_atomic_load_acquire(&c->state[slot]); }
uint32_t shmc_capacity(shm_chan_ctx *c){ return c?c->capacity:0; }
uint32_t shmc_slot_size(shm_chan_ctx *c){ return c?c->slot_size:0; }
void shmc_destroy(shm_chan_ctx *c,int unlink){ if(!c) return; if(unlink&&c->is_creator){
#ifndef _WIN32
 shm_unlink(c->name);
#endif
 } if(c->map_addr) unmap_shm(c->map_addr,c->size); if(c->shm_fd!=INVALID_H) close_h(c->shm_fd); if(c->notify_fd!=INVALID_H) close_h(c->notify_fd); free(c); }
const char *shmc_strerror(int e){ return shmq_strerror(e); }
