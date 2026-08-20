//! shmqueue 的安全 Rust 封装。

use crate::ffi::{self, Mode, shm_queue_ctx};
use std::ptr;
use std::sync::Arc;

/// 从队列取出的消息
#[derive(Debug, Clone)]
pub struct Message {
    pub msg_type: u32,
    pub data: Vec<u8>,
}

/// SPSC 共享内存环形队列（单生产者/单消费者）。
///
/// `create` 创建队列（首个进程，负责初始化），其余进程用 `attach` 连接。
/// 队列句柄可在进程内跨线程传递（底层以共享内存为同步原语），
/// 但生产者侧只应在一个线程写、消费者侧只应在一个线程读。
pub struct ShmQueue {
    lib: Arc<ffi::Lib>,
    ctx: *mut shm_queue_ctx,
}

// 共享内存句柄本身不携带线程局部状态，可以跨线程移动；
// 但 C 侧 SPSC 语义要求收发分别在各自的线程上，因此仅实现 Send。
unsafe impl Send for ShmQueue {}

impl ShmQueue {
    /// 创建共享内存队列（作为创建者）。
    pub fn create(name: &str, capacity: u32, slot_size: u32, mode: Mode) -> Result<Self, ffi::Error> {
        let lib = Arc::new(ffi::Lib::load().map_err(|e| {
            eprintln!("[shmqueue] load failed: {}", e);
            ffi::Error::InvalidArg
        })?);
        let ctx = lib.create(name, capacity, slot_size, mode)?;
        Ok(ShmQueue { lib, ctx })
    }

    /// 附加到已存在的共享内存队列。
    pub fn attach(name: &str, capacity: u32, slot_size: u32, mode: Mode) -> Result<Self, ffi::Error> {
        let lib = Arc::new(ffi::Lib::load().map_err(|e| {
            eprintln!("[shmqueue] load failed: {}", e);
            ffi::Error::InvalidArg
        })?);
        let ctx = lib.attach(name, capacity, slot_size, mode)?;
        Ok(ShmQueue { lib, ctx })
    }

    /// 发送一条消息。data 不能超过 slot_size - 8。
    pub fn send(&self, msg_type: u32, data: &[u8]) -> Result<(), ffi::Error> {
        let rc = unsafe {
            (self.lib.fns.shmq_send)(
                self.ctx,
                msg_type,
                data.as_ptr() as *const _,
                data.len() as u32,
            )
        };
        if rc != 0 {
            return Err(ffi::Error::from(rc));
        }
        Ok(())
    }

    /// 接收一条消息，data 会被复制到新分配的 Vec。
    pub fn recv(&self, slot_cap: u32) -> Result<Message, ffi::Error> {
        self.recv_impl(slot_cap, 0)
    }

    /// 带超时接收（timeout_ns 纳秒）。超时返回 Err(Error::Timeout)。
    pub fn recv_timeout(&self, slot_cap: u32, timeout_ns: u64) -> Result<Message, ffi::Error> {
        self.recv_impl(slot_cap, timeout_ns)
    }

    fn recv_impl(&self, slot_cap: u32, timeout_ns: u64) -> Result<Message, ffi::Error> {
        let mut buf = vec![0u8; slot_cap as usize];
        let mut mt: u32 = 0;
        let mut len: u32 = 0;
        let rc = unsafe {
            if timeout_ns > 0 {
                (self.lib.fns.shmq_recv_timeout)(
                    self.ctx,
                    &mut mt,
                    buf.as_mut_ptr() as *mut _,
                    slot_cap,
                    &mut len,
                    timeout_ns,
                )
            } else {
                (self.lib.fns.shmq_recv)(self.ctx, &mut mt, buf.as_mut_ptr() as *mut _, slot_cap, &mut len)
            }
        };
        if rc != 0 {
            return Err(ffi::Error::from(rc));
        }
        buf.truncate(len as usize);
        Ok(Message { msg_type: mt, data: buf })
    }

    /// 取得通知 fd（Linux/Unix 上 eventfd；SPIN 模式返回 InvalidArg）。
    pub fn notify_fd(&self) -> Result<i32, ffi::Error> {
        let mut fd: i32 = -1;
        let rc = unsafe { (self.lib.fns.shmq_get_notify_fd)(self.ctx, &mut fd) };
        if rc != 0 {
            return Err(ffi::Error::from(rc));
        }
        Ok(fd)
    }

    pub fn set_spin_ns(&self, spin_ns: u64) -> Result<(), ffi::Error> {
        let rc = unsafe { (self.lib.fns.shmq_set_spin_ns)(self.ctx, spin_ns) };
        if rc != 0 {
            return Err(ffi::Error::from(rc));
        }
        Ok(())
    }

    pub fn capacity(&self) -> Result<u32, ffi::Error> {
        let mut out: u32 = 0;
        let rc = unsafe { (self.lib.fns.shmq_capacity)(self.ctx, &mut out) };
        if rc != 0 {
            return Err(ffi::Error::from(rc));
        }
        Ok(out)
    }

    pub fn slot_size(&self) -> Result<u32, ffi::Error> {
        let mut out: u32 = 0;
        let rc = unsafe { (self.lib.fns.shmq_slot_size)(self.ctx, &mut out) };
        if rc != 0 {
            return Err(ffi::Error::from(rc));
        }
        Ok(out)
    }

    pub fn mode(&self) -> Result<Mode, ffi::Error> {
        let mut out: i32 = -1;
        let rc = unsafe { (self.lib.fns.shmq_mode)(self.ctx, &mut out) };
        if rc != 0 {
            return Err(ffi::Error::from(rc));
        }
        Mode::from_raw(out).ok_or(ffi::Error::InvalidArg)
    }
}

impl Drop for ShmQueue {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe {
                (self.lib.fns.shmq_destroy)(self.ctx, 0);
            }
            self.ctx = ptr::null_mut();
        }
    }
}
