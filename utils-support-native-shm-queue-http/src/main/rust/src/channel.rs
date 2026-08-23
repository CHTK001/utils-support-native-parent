//! 单环 Zero-copy：单块 SHM，同槽 REQ→RESP 原地复用。
//!
//! 状态机：EMPTY → REQ（Rust 写请求） → RESP（Java 写响应） → EMPTY（Rust 回包后释放）
//! Rust 负责分配/释放槽；Java 在 Rust 分配的槽上原地覆写响应（zero-copy）。

use std::ffi::CString;
use std::os::raw::c_char;

#[repr(C)]
pub struct ShmCtx {
    _private: [u8; 0],
}

extern "C" {
    pub fn shmc_create(
        name: *const c_char,
        shm_size: usize,
        capacity: u32,
        slot_size: u32,
        ctx_out: *mut *mut ShmCtx,
    ) -> i32;
    pub fn shmc_acquire_empty(
        ctx: *mut ShmCtx,
        slot: *mut u32,
        ptr: *mut *mut u8,
    ) -> i32;
    pub fn shmc_commit_req(ctx: *mut ShmCtx, slot: u32, len: u32) -> i32;
    pub fn shmc_poll_req(
        ctx: *mut ShmCtx,
        slot: *mut u32,
        ptr: *mut *mut u8,
        len: *mut u32,
        timeout_ns: u64,
    ) -> i32;
    pub fn shmc_commit_resp(ctx: *mut ShmCtx, slot: u32) -> i32;
    pub fn shmc_poll_resp(
        ctx: *mut ShmCtx,
        slot: *mut u32,
        ptr: *mut *mut u8,
        len: *mut u32,
        timeout_ns: u64,
    ) -> i32;
    pub fn shmc_release(ctx: *mut ShmCtx, slot: u32);
    pub fn shmc_destroy(ctx: *mut ShmCtx, unlink: i32);
    pub fn shmc_slot_ptr(ctx: *mut ShmCtx, slot: u32) -> *mut u8;
    pub fn shmc_slot_size(ctx: *mut ShmCtx, slot: *mut u32) -> i32;
    pub fn shmc_slot_state(ctx: *mut ShmCtx, slot: u32) -> u32;

    pub fn shmq_slot_size(ctx: *mut ShmCtx, slot: *mut u32) -> i32;
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
        eprintln!("[ch] create: name='{}', cap={}, ss={}", name, capacity, slot_size);
        let n = CString::new(name).map_err(|_| -1)?;
        eprintln!("[ch] CString ok");
        let mut c: *mut ShmCtx = std::ptr::null_mut();
        eprintln!("[ch] calling shmc_create({:?}, 0, {}, {}, 2, {:p})", n.as_ptr(), capacity, slot_size, std::ptr::addr_of_mut!(c));
        let rc = unsafe { shmc_create(n.as_ptr(), 0, capacity, slot_size, &mut c) };
        eprintln!("[ch] shmc_create rc={}, ctx={:p}", rc, c);
        if rc != 0 {
            return Err(rc);
        }
        eprintln!("[ch] ctx created");
        Ok(Self { ctx: c as usize })
    }

    /// Rust: 获取一个 EMPTY 槽，写入请求数据，标记为 REQ。
    /// 返回：(slot_index, slot_ptr) 供 Rust 写数据。
    pub fn acquire_empty(&self) -> Result<(u32, *mut u8), i32> {
        let mut slot: u32 = 0;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let rc = unsafe {
            shmc_acquire_empty(self.ctx as *mut ShmCtx, &mut slot, &mut ptr)
        };
        if rc != 0 {
            return Err(rc);
        }
        Ok((slot, ptr))
    }

    /// Rust: 标记槽为 REQ（由 shmc_acquire_empty 后自动完成，也可显式调用）。
    pub fn commit_req(&self, slot: u32, len: u32) -> Result<(), i32> {
        let rc = unsafe { shmc_commit_req(self.ctx as *mut ShmCtx, slot, len) };
        if rc != 0 {
            return Err(rc);
        }
        Ok(())
    }

    /// Java: 轮询读取 REQ 槽，返回 (slot, ptr, len)。
    /// 超时返回错误。
    pub fn poll_req_timeout(&self, timeout_ns: u64) -> Result<(u32, *mut u8, u32), i32> {
        let mut slot: u32 = 0;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut len: u32 = 0;
        let rc = unsafe {
            shmc_poll_req(
                self.ctx as *mut ShmCtx,
                &mut slot,
                &mut ptr,
                &mut len,
                timeout_ns,
            )
        };
        if rc != 0 {
            return Err(rc);
        }
        Ok((slot, ptr, len))
    }

    /// Java: 在已有 slot 位置写入响应数据（原地覆写，zero-copy）。
    /// 数据布局：前 4 字节 = msg_type(u32=STATE_RESP)，第 4-8 字节 = body_len(u32)，第 8 字节后为 body。
    pub fn write_resp_direct(&self, slot: u32, data: &[u8]) -> Result<(), i32> {
        let cap_result = unsafe { shmc_slot_size(self.ctx as *mut ShmCtx, &mut 0) };
        let cap = if cap_result >= 0 { cap_result as usize } else { return Err(cap_result) };
        if data.len() + 8 > cap {
            return Err(-9);
        }
        let ptr = unsafe { shmc_slot_ptr(self.ctx as *mut ShmCtx, slot) };
        if ptr.is_null() {
            return Err(-10);
        }
        // 写入：[4字节状态 RESP] [4字节 body_len] [body...]
        unsafe {
            std::ptr::copy_nonoverlapping(
                &STATE_REQ as *const u32 as *const u8,
                ptr,
                4,
            );
            // 注意：下面写入 len，实际应写入 body_len
            // 这里写入 data.len() 作为 body_len
            let ln = data.len() as u32;
            std::ptr::copy_nonoverlapping(
                &ln as *const u32 as *const u8,
                ptr.add(4),
                4,
            );
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr.add(8), data.len());
        }
        // 标记为 RESP 状态
        let _ = unsafe { shmc_commit_resp(self.ctx as *mut ShmCtx, slot) };
        Ok(())
    }

    /// Java: 轮询读取 RESP 槽，返回 (slot, ptr, len)。
    pub fn poll_resp_timeout(&self, timeout_ns: u64) -> Result<(u32, *mut u8, u32), i32> {
        let mut slot: u32 = 0;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut len: u32 = 0;
        let rc = unsafe {
            shmc_poll_resp(
                self.ctx as *mut ShmCtx,
                &mut slot,
                &mut ptr,
                &mut len,
                timeout_ns,
            )
        };
        if rc != 0 {
            return Err(rc);
        }
        Ok((slot, ptr, len))
    }

    /// 释放槽回 EMPTY
    pub fn release(&self, slot: u32) {
        unsafe { shmc_release(self.ctx as *mut ShmCtx, slot) };
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        if self.ctx != 0 {
            unsafe { shmc_destroy(self.ctx as *mut ShmCtx, 0) };
            self.ctx = 0;
        }
    }
}// trigger rebuild 14:24:28
