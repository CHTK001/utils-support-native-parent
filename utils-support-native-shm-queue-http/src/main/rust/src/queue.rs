//! 共享内存环形队列封装（调用编译进本 cdylib 的 C 实现）。

use std::ffi::CString;
use std::os::raw::c_char;

/// 超时错误码（与 C 对齐）
pub const TIMEOUT: i32 = -12;

/// C 侧不透明上下文
#[repr(C)]
struct ShmCtx {
    _private: [u8; 0],
}

extern "C" {
    fn shmq_create(
        name: *const c_char,
        shm_size: usize,
        capacity: u32,
        slot_size: u32,
        mode: i32,
        ctx_out: *mut *mut ShmCtx,
    ) -> i32;
    fn shmq_send(ctx: *mut ShmCtx, msg_type: u32, data: *const u8, len: u32) -> i32;
    fn shmq_recv_timeout(
        ctx: *mut ShmCtx,
        msg_type: *mut u32,
        buf: *mut u8,
        buf_cap: u32,
        len_out: *mut u32,
        timeout_ns: u64,
    ) -> i32;
    fn shmq_destroy(ctx: *mut ShmCtx, unlink: i32);
}

/// 队列句柄。C 侧以共享内存 + 原子操作为同步原语，
/// 此处仅持有指针地址（usize），可安全跨线程传递。
#[derive(Clone)]
pub struct Queue {
    ctx: usize,
}

// C 实现自身处理并发同步，句柄可在线程间移动
unsafe impl Send for Queue {}
unsafe impl Sync for Queue {}

impl Queue {
    /// 创建（或复用）命名队列。mode 固定为 HYBRID(2) 以便支持超时接收。
    pub fn create(name: &str, capacity: u32, slot_size: u32) -> Result<Queue, i32> {
        let cname = CString::new(name).map_err(|_| -1)?;
        let mut ctx: *mut ShmCtx = std::ptr::null_mut();
        // 0 表示 shm_size 由 C 侧按 capacity*slot_size 自动计算
        let rc = unsafe { shmq_create(cname.as_ptr(), 0, capacity, slot_size, 2, &mut ctx) };
        if rc != 0 {
            return Err(rc);
        }
        Ok(Queue {
            ctx: ctx as usize,
        })
    }

    /// 发送一条消息
    pub fn send(&self, msg_type: u32, data: &[u8]) -> Result<(), i32> {
        let rc = unsafe {
            shmq_send(
                self.ctx as *mut ShmCtx,
                msg_type,
                data.as_ptr(),
                data.len() as u32,
            )
        };
        if rc != 0 {
            return Err(rc);
        }
        Ok(())
    }

    /// 超时接收，返回实际写入字节数
    pub fn recv_timeout(&self, buf: &mut [u8], timeout_ns: u64) -> Result<usize, i32> {
        let mut msg_type: u32 = 0;
        let mut len: u32 = 0;
        let rc = unsafe {
            shmq_recv_timeout(
                self.ctx as *mut ShmCtx,
                &mut msg_type,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut len,
                timeout_ns,
            )
        };
        if rc != 0 {
            return Err(rc);
        }
        Ok(len as usize)
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        if self.ctx != 0 {
            unsafe {
                shmq_destroy(self.ctx as *mut ShmCtx, 0);
            }
            self.ctx = 0;
        }
    }
}
