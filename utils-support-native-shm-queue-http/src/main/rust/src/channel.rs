//! 單環 Zero-copy 通道
//!
//! 一塊共享內存、同一槽位輪轉：空 → REQ_READY → RESP_READY → 空。

use std::ffi::CString;
use std::os::raw::c_char;

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

#[derive(Clone)]
pub struct Channel {
    ctx: usize,
}
unsafe impl Send for Channel {}
unsafe impl Sync for Channel {}

impl Channel {
    pub fn create(name: &str, capacity: u32, slot_size: u32) -> Result<Channel, i32> {
        let cname = CString::new(name).map_err(|_| -1)?;
        let mut ctx: *mut ShmCtx = std::ptr::null_mut();
        let rc = unsafe { shmq_create(cname.as_ptr(), 0, capacity, slot_size, 2, &mut ctx) };
        if rc != 0 {
            return Err(rc);
        }
        Ok(Channel { ctx: ctx as usize })
    }

    pub fn send(&self, data: &[u8]) -> Result<(), i32> {
        let rc = unsafe { shmq_send(self.ctx as *mut ShmCtx, 1, data.as_ptr(), data.len() as u32) };
        if rc != 0 { Err(rc) } else { Ok(()) }
    }

    pub fn recv_timeout(&self, buf: &mut [u8], ns: u64) -> Result<usize, i32> {
        let mut mt: u32 = 0;
        let mut len: u32 = 0;
        let rc = unsafe {
            shmq_recv_timeout(
                self.ctx as *mut ShmCtx,
                &mut mt,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut len,
                ns,
            )
        };
        if rc != 0 { Err(rc) } else { Ok(len as usize) }
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        if self.ctx != 0 {
            unsafe { shmq_destroy(self.ctx as *mut ShmCtx, 0) };
            self.ctx = 0;
        }
    }
}
