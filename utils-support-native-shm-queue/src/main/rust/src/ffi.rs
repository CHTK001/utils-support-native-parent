//! shmqueue C 库的动态加载绑定。

use libloading::Library;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;

/// 错误码（与 C 头文件对齐）
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[repr(i32)]
pub enum Error {
    #[error("OK")]
    Ok = 0,
    #[error("invalid argument")]
    InvalidArg = -1,
    #[error("out of memory")]
    NoMem = -2,
    #[error("open shm failed")]
    OpenShm = -3,
    #[error("truncate failed")]
    Truncate = -4,
    #[error("mmap failed")]
    Mmap = -5,
    #[error("header magic mismatch")]
    HeaderMagic = -6,
    #[error("header version mismatch")]
    HeaderVersion = -7,
    #[error("queue full")]
    QueueFull = -8,
    #[error("data too large for slot")]
    DataTooLarge = -9,
    #[error("write notify fd failed")]
    WriteFd = -10,
    #[error("read notify fd failed")]
    ReadFd = -11,
    #[error("recv timeout")]
    Timeout = -12,
    #[error("not supported on this platform")]
    NotSupported = -13,
    #[error("queue destroyed")]
    Destroyed = -14,
}

/// 库加载失败（非 C 错误码）
#[derive(Debug, thiserror::Error)]
#[error("libshmqueue load failure: {0}")]
pub struct LibError(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Mode {
    Spin = 0,
    Block = 1,
    Hybrid = 2,
}

impl Mode {
    pub fn from_raw(v: i32) -> Option<Mode> {
        match v {
            0 => Some(Mode::Spin),
            1 => Some(Mode::Block),
            2 => Some(Mode::Hybrid),
            _ => None,
        }
    }
}

impl From<i32> for Error {
    fn from(v: i32) -> Self {
        match v {
            0 => Error::Ok,
            -1 => Error::InvalidArg,
            -2 => Error::NoMem,
            -3 => Error::OpenShm,
            -4 => Error::Truncate,
            -5 => Error::Mmap,
            -6 => Error::HeaderMagic,
            -7 => Error::HeaderVersion,
            -8 => Error::QueueFull,
            -9 => Error::DataTooLarge,
            -10 => Error::WriteFd,
            -11 => Error::ReadFd,
            -12 => Error::Timeout,
            -13 => Error::NotSupported,
            -14 => Error::Destroyed,
            _ => Error::InvalidArg,
        }
    }
}

#[repr(C)]
pub struct shm_queue_ctx {
    _private: [u8; 0],
}

/// 不透明函数指针集合
#[allow(non_snake_case)]
#[derive(Clone)]
pub struct Fns {
    pub shmq_create: unsafe extern "C" fn(
        name: *const c_char,
        shm_size: usize,
        capacity: u32,
        slot_size: u32,
        mode: c_int,
        ctx_out: *mut *mut shm_queue_ctx,
    ) -> c_int,
    pub shmq_attach: unsafe extern "C" fn(
        name: *const c_char,
        shm_size: usize,
        capacity: u32,
        slot_size: u32,
        mode: c_int,
        ctx_out: *mut *mut shm_queue_ctx,
    ) -> c_int,
    pub shmq_send: unsafe extern "C" fn(
        ctx: *mut shm_queue_ctx,
        msg_type: u32,
        data: *const c_void,
        len: u32,
    ) -> c_int,
    pub shmq_recv: unsafe extern "C" fn(
        ctx: *mut shm_queue_ctx,
        msg_type: *mut u32,
        buf: *mut c_void,
        buf_cap: u32,
        len_out: *mut u32,
    ) -> c_int,
    pub shmq_recv_timeout: unsafe extern "C" fn(
        ctx: *mut shm_queue_ctx,
        msg_type: *mut u32,
        buf: *mut c_void,
        buf_cap: u32,
        len_out: *mut u32,
        timeout_ns: u64,
    ) -> c_int,
    pub shmq_get_notify_fd: unsafe extern "C" fn(ctx: *mut shm_queue_ctx, fd: *mut c_int) -> c_int,
    pub shmq_set_spin_ns: unsafe extern "C" fn(ctx: *mut shm_queue_ctx, spin_ns: u64) -> c_int,
    pub shmq_capacity: unsafe extern "C" fn(ctx: *mut shm_queue_ctx, out: *mut u32) -> c_int,
    pub shmq_slot_size: unsafe extern "C" fn(ctx: *mut shm_queue_ctx, out: *mut u32) -> c_int,
    pub shmq_mode: unsafe extern "C" fn(ctx: *mut shm_queue_ctx, out: *mut c_int) -> c_int,
    pub shmq_destroy: unsafe extern "C" fn(ctx: *mut shm_queue_ctx, unlink: c_int),
    pub shmq_strerror: unsafe extern "C" fn(err: c_int) -> *const c_char,
}

/// 持有底层动态库句柄，保证函数指针在库卸载前有效。
pub struct Lib {
    _handle: Library,
    pub fns: Fns,
}

fn default_lib_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "shmqueue.dll"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "libshmqueue.so"
    }
    #[cfg(target_os = "macos")]
    {
        "libshmqueue.dylib"
    }
}

fn with_cstr<F, T>(name: &str, f: F) -> Result<T, Error>
where
    F: FnOnce(*const c_char) -> T,
{
    let c = CString::new(name).map_err(|_| Error::InvalidArg)?;
    Ok(f(c.as_ptr()))
}

impl Lib {
    /// 加载并绑定全部符号。`explicit_path` 可指定动态库完整路径，
    /// 否则按平台默认名在加载器搜索路径中查找。
    pub fn load() -> Result<Lib, LibError> {
        Self::load_from(None)
    }

    pub fn load_from(explicit_path: Option<&Path>) -> Result<Lib, LibError> {
        let path_str = explicit_path.map(|p| p.to_string_lossy().into_owned());
        let err_msg;
        unsafe {
            let lib = match &path_str {
                Some(p) => Library::new(p).map_err(|e| {
                    LibError(format!("{}: {}", p, e))
                }),
                None => Library::new(default_lib_name()).map_err(|e| {
                    LibError(format!("{}: {}", default_lib_name(), e))
                }),
            };
            let lib = match lib {
                Ok(l) => l,
                Err(e) => return Err(e),
            };

            macro_rules! sym {
                ($name:literal, $ty:ty) => {
                    match lib.get::<$ty>($name.as_bytes()) {
                        Ok(s) => *s,
                        Err(e) => {
                            err_msg = format!("symbol {}: {}", $name, e);
                            return Err(LibError(err_msg));
                        }
                    }
                };
            }

            let fns = Fns {
                shmq_create: sym!("shmq_create", unsafe extern "C" fn(*const c_char, usize, u32, u32, c_int, *mut *mut shm_queue_ctx) -> c_int),
                shmq_attach: sym!("shmq_attach", unsafe extern "C" fn(*const c_char, usize, u32, u32, c_int, *mut *mut shm_queue_ctx) -> c_int),
                shmq_send: sym!("shmq_send", unsafe extern "C" fn(*mut shm_queue_ctx, u32, *const c_void, u32) -> c_int),
                shmq_recv: sym!("shmq_recv", unsafe extern "C" fn(*mut shm_queue_ctx, *mut u32, *mut c_void, u32, *mut u32) -> c_int),
                shmq_recv_timeout: sym!("shmq_recv_timeout", unsafe extern "C" fn(*mut shm_queue_ctx, *mut u32, *mut c_void, u32, *mut u32, u64) -> c_int),
                shmq_get_notify_fd: sym!("shmq_get_notify_fd", unsafe extern "C" fn(*mut shm_queue_ctx, *mut c_int) -> c_int),
                shmq_set_spin_ns: sym!("shmq_set_spin_ns", unsafe extern "C" fn(*mut shm_queue_ctx, u64) -> c_int),
                shmq_capacity: sym!("shmq_capacity", unsafe extern "C" fn(*mut shm_queue_ctx, *mut u32) -> c_int),
                shmq_slot_size: sym!("shmq_slot_size", unsafe extern "C" fn(*mut shm_queue_ctx, *mut u32) -> c_int),
                shmq_mode: sym!("shmq_mode", unsafe extern "C" fn(*mut shm_queue_ctx, *mut c_int) -> c_int),
                shmq_destroy: sym!("shmq_destroy", unsafe extern "C" fn(*mut shm_queue_ctx, c_int)),
                shmq_strerror: sym!("shmq_strerror", unsafe extern "C" fn(c_int) -> *const c_char),
            };
            return Ok(Lib { _handle: lib, fns });
        }
    }

    pub fn create(
        &self,
        name: &str,
        capacity: u32,
        slot_size: u32,
        mode: Mode,
    ) -> Result<*mut shm_queue_ctx, Error> {
        with_cstr(name, |c| {
            let mut ctx: *mut shm_queue_ctx = std::ptr::null_mut();
            let rc = unsafe {
                (self.fns.shmq_create)(c, 0, capacity, slot_size, mode as i32, &mut ctx)
            };
            if rc != 0 {
                return Err(Error::from(rc));
            }
            Ok(ctx)
        })
    }

    pub fn attach(
        &self,
        name: &str,
        capacity: u32,
        slot_size: u32,
        mode: Mode,
    ) -> Result<*mut shm_queue_ctx, Error> {
        with_cstr(name, |c| {
            let mut ctx: *mut shm_queue_ctx = std::ptr::null_mut();
            let rc = unsafe {
                (self.fns.shmq_attach)(c, 0, capacity, slot_size, mode as i32, &mut ctx)
            };
            if rc != 0 {
                return Err(Error::from(rc));
            }
            Ok(ctx)
        })
    }

    pub fn strerror(&self, err: c_int) -> String {
        unsafe {
            let p = (self.fns.shmq_strerror)(err);
            if p.is_null() {
                return "unknown".into();
            }
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

/// 确保底层动态库可加载（初始化检测）。
pub fn ensure_sys_loaded() {
    if let Err(e) = Lib::load() {
        eprintln!("[shmqueue] lib load warning: {}", e);
    }
}
