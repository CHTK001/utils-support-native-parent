//! 单环 Zero-copy：单块 SHM，同槽 REQ→RESP 原地复用。
//!
//! 状态机：EMPTY → REQ（Rust 写请求） → RESP（Java 写响应） → EMPTY（Rust 回包后释放）
//! Rust 负责分配/释放槽；Java 在 Rust 分配的槽上原地覆写响应。

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
    fn shmq_slot_ptr(ctx: *mut ShmCtx, slot: u32) -> *mut u8;
    fn shmq_slot_state(ctx: *mut ShmCtx, slot: u32) -> u32;
    fn shmq_set_slot_state(ctx: *mut ShmCtx, slot: u32, state: u32);
    fn shmq_slot_size(ctx: *mut ShmCtx, slot: *mut u32) -> i32;
}

const STATE_REQ: u32 = 1;
const STATE_RESP: u32 = 2;

#[derive(Clone)]
pub struct Channel {
    ctx: usize,
}
unsafe impl Send for Channel {}
unsafe impl Sync for Channel {}

impl Channel {
    pub fn create(name: &str, capacity: u32, slot_size: u32) -> Result<Self, i32> {
        let n = CString::new(name).map_err(|_| -1)?;
        let mut c: *mut ShmCtx = std::ptr::null_mut();
        let rc = unsafe { shmq_create(n.as_ptr(), 0, capacity, slot_size, 2, &mut c) };
        if rc != 0 {
            return Err(rc);
        }
        Ok(Self { ctx: c as usize })
    }

    pub fn send_req(&self, data: &[u8]) -> Result<u32, i32> {
        let rc = unsafe { shmq_send(self.ctx as *mut ShmCtx, STATE_REQ, data.as_ptr(), data.len() as u32) };
        if rc != 0 {
            return Err(rc);
        }
        Ok(0)
    }

    pub fn send(&self, data: &[u8]) -> Result<(), i32> {
        self.send_req(data).map(|_| ())
    }

    pub fn recv_timeout(&self, buf: &mut [u8], ns: u64) -> Result<usize, i32> {
        self.recv_any_timeout(buf, ns, STATE_REQ)
    }

    pub fn recv_any_timeout(&self, buf: &mut [u8], ns: u64, expected_msg_type: u32) -> Result<usize, i32> {
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
        if rc != 0 {
            return Err(rc);
        }
        if mt != expected_msg_type {
            let _ = self.commit_resp_slot(0, &[]);
        }
        Ok(len as usize)
    }

    pub fn write_resp_direct(&self, slot: u32, data: &[u8]) {
        unsafe {
            let ptr = shmq_slot_ptr(self.ctx as *mut ShmCtx, slot);
            if ptr.is_null() {
                return;
            }
            let cap = {
                let mut c: u32 = 0;
                let _ = shmq_slot_size(self.ctx as *mut ShmCtx, &mut c);
                c as usize
            };
            if data.len() + 8 > cap {
                return;
            }
            let mt: u32 = STATE_RESP;
            let ln: u32 = data.len() as u32;
            std::ptr::copy_nonoverlapping(&mt as *const u32 as *const u8, ptr, 4);
            std::ptr::copy_nonoverlapping(&ln as *const u32 as *const u8, ptr.add(4), 4);
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr.add(8), data.len());
            shmq_set_slot_state(self.ctx as *mut ShmCtx, slot, STATE_RESP);
        }
    }

    fn commit_resp_slot(&self, slot: u32, data: &[u8]) -> i32 {
        unsafe {
            shmq_set_slot_state(self.ctx as *mut ShmCtx, slot, 1);
            let rc = shmq_send(self.ctx as *mut ShmCtx, STATE_RESP, data.as_ptr(), data.len() as u32);
            if rc != 0 {
                return rc;
            }
            Ok(())
        }
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
