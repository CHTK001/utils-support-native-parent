//! libjpeg-turbo 门面 —— 把 TurboJPEG 的有状态句柄 API 收敛成扁平 C ABI，
//! 供 Java 侧通过 Panama FFM 直接下探调用。
//!
//! 链接的是 `build.rs` 指定的 `libturbojpeg`（由 `build.sh` / `build.ps1`
//! 从上游源码编译，含 SIMD 汇编核），因此动态库自身即完整的 libjpeg-turbo 实现。
//!
//! 所有导出函数：成功返回 0，失败返回非 0 错误码，
//! 失败原因用 `chua_tj_last_error()` 取回（线程局部）。

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_uchar, c_ulong, c_void};
use std::ptr;

/// TurboJPEG 句柄（不透明类型）。
type tjhandle = *mut c_void;

extern "C" {
    fn tjInitCompress() -> tjhandle;
    fn tjInitDecompress() -> tjhandle;
    fn tjDestroy(handle: tjhandle);
    fn tjGetErrorStr2(handle: tjhandle) -> *mut c_char;
    fn tjAlloc(bytes: usize) -> *mut c_uchar;
    fn tjFree(buffer: *mut c_uchar);
    fn tjCompress2(
        handle: tjhandle,
        src_buf: *const c_uchar,
        width: c_int,
        pitch: c_int,
        height: c_int,
        pixel_format: c_int,
        jpeg_buf: *mut *mut c_uchar,
        jpeg_size: *mut c_ulong,
        jpeg_subsamp: c_int,
        jpeg_qual: c_int,
        flags: c_int,
    ) -> c_int;
    fn tjDecompressHeader3(
        handle: tjhandle,
        jpeg_buf: *const c_uchar,
        jpeg_size: c_ulong,
        width: *mut c_int,
        height: *mut c_int,
        jpeg_subsamp: *mut c_int,
        jpeg_colorspace: *mut c_int,
    ) -> c_int;
    fn tjDecompress2(
        handle: tjhandle,
        jpeg_buf: *const c_uchar,
        jpeg_size: c_ulong,
        dst_buf: *mut c_uchar,
        width: c_int,
        pitch: c_int,
        height: c_int,
        pixel_format: c_int,
        flags: c_int,
    ) -> c_int;
}

/// 上游 libjpeg-turbo 版本，与 `build.ps1` / `build.sh` 拉取的源码版本一致。
const LJTB_VERSION: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(b"3.1.2\0") };

/// 错误码：句柄创建失败。
const ERR_HANDLE: c_int = -1;
/// 错误码：入参非法。
const ERR_ARG: c_int = -2;
/// 错误码：底层 TurboJPEG 报错。
const ERR_TURBOJPEG: c_int = -3;

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

/// 记录一次失败原因，供 `chua_tj_last_error()` 取回。
fn set_last_error(handle: tjhandle, context: &str) {
    let raw = unsafe {
        if handle.is_null() {
            String::from("<no handle>")
        } else {
            let p = tjGetErrorStr2(handle);
            if p.is_null() {
                String::from("<empty>")
            } else {
                CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        }
    };
    let combined = format!("{context}: {raw}");
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = CString::new(combined).unwrap_or_default();
    });
}

/// TurboJPEG 打包像素格式的每像素字节数。
fn pixel_size(pixel_format: c_int) -> Option<usize> {
    match pixel_format {
        // TJPF_RGB / TJPF_BGR
        0 | 1 => Some(3),
        // TJPF_RGBX / TJPF_BGRX / TJPF_XBGR / TJPF_XRGB
        2..=5 => Some(4),
        // TJPF_GRAY
        6 => Some(1),
        // TJPF_RGBA / TJPF_BGRA / TJPF_ABGR / TJPF_ARGB / TJPF_CMYK
        7..=11 => Some(4),
        _ => None,
    }
}

/// 4 字节行对齐后的行跨距，与 TurboJPEG 内部 `TJPAD` 一致。
fn padded_pitch(width: c_int, pixel_format: c_int) -> Option<c_int> {
    let ps = pixel_size(pixel_format)?;
    let raw = width.max(0) as usize * ps;
    Some(((raw + 3) & !3) as c_int)
}

/// 上游 libjpeg-turbo 版本号字符串（静态存储，调用方不需释放）。
#[no_mangle]
pub extern "C" fn chua_tj_version() -> *const c_char {
    LJTB_VERSION.as_ptr()
}

/// 最近一次失败的描述（线程局部，指向内部存储，调用方不需释放）。
#[no_mangle]
pub extern "C" fn chua_tj_last_error() -> *const c_char {
    LAST_ERROR.with(|cell| cell.borrow().as_ptr())
}

/// 释放由本库返回的缓冲区。
#[no_mangle]
pub unsafe extern "C" fn chua_tj_free(buffer: *mut c_void) {
    if !buffer.is_null() {
        tjFree(buffer as *mut c_uchar);
    }
}

/// 把打包像素缓冲压缩成 JPEG。
///
/// `dst` 成功时被写入由本库分配的 JPEG 字节，调用方须用 `chua_tj_free` 释放。
#[no_mangle]
pub unsafe extern "C" fn chua_tj_compress(
    src: *const c_uchar,
    width: c_int,
    height: c_int,
    pitch: c_int,
    pixel_format: c_int,
    quality: c_int,
    subsamp: c_int,
    flags: c_int,
    dst: *mut *mut c_uchar,
    dst_size: *mut c_ulong,
) -> c_int {
    if src.is_null() || dst.is_null() || dst_size.is_null() {
        set_last_error(ptr::null_mut(), "chua_tj_compress: null argument");
        return ERR_ARG;
    }
    if width <= 0 || height <= 0 || pixel_size(pixel_format).is_none() {
        set_last_error(
            ptr::null_mut(),
            "chua_tj_compress: width/height must be positive and pixelFormat must be a TJPF_* value",
        );
        return ERR_ARG;
    }
    *dst = ptr::null_mut();
    *dst_size = 0;

    let handle = tjInitCompress();
    if handle.is_null() {
        set_last_error(handle, "tjInitCompress");
        return ERR_HANDLE;
    }
    let ret = tjCompress2(
        handle,
        src,
        width,
        pitch,
        height,
        pixel_format,
        dst,
        dst_size,
        subsamp,
        quality,
        flags,
    );
    tjDestroy(handle);
    if ret != 0 {
        set_last_error(ptr::null_mut(), "tjCompress2");
        *dst = ptr::null_mut();
        *dst_size = 0;
        return ERR_TURBOJPEG;
    }
    0
}

/// 解析 JPEG 头部，取回宽高与色度二次采样，不解码像素。
#[no_mangle]
pub unsafe extern "C" fn chua_tj_probe(
    jpeg: *const c_uchar,
    jpeg_size: c_ulong,
    width: *mut c_int,
    height: *mut c_int,
    subsamp: *mut c_int,
) -> c_int {
    if jpeg.is_null() || width.is_null() || height.is_null() || subsamp.is_null() {
        set_last_error(ptr::null_mut(), "chua_tj_probe: null argument");
        return ERR_ARG;
    }
    *width = 0;
    *height = 0;
    *subsamp = 0;

    let handle = tjInitDecompress();
    if handle.is_null() {
        set_last_error(handle, "tjInitDecompress");
        return ERR_HANDLE;
    }
    let mut colorspace = 0;
    let ret = tjDecompressHeader3(handle, jpeg, jpeg_size, width, height, subsamp, &mut colorspace);
    tjDestroy(handle);
    if ret != 0 {
        set_last_error(ptr::null_mut(), "tjDecompressHeader3");
        return ERR_TURBOJPEG;
    }
    0
}

/// 把 JPEG 解码为打包像素缓冲。
///
/// `dst` 由本库按 4 字节行对齐分配，宽高与行跨距写回出参，调用方须用 `chua_tj_free` 释放。
#[no_mangle]
pub unsafe extern "C" fn chua_tj_decompress(
    jpeg: *const c_uchar,
    jpeg_size: c_ulong,
    pixel_format: c_int,
    flags: c_int,
    dst: *mut *mut c_uchar,
    width: *mut c_int,
    height: *mut c_int,
    pitch: *mut c_int,
) -> c_int {
    if jpeg.is_null() || dst.is_null() || width.is_null() || height.is_null() || pitch.is_null() {
        set_last_error(ptr::null_mut(), "chua_tj_decompress: null argument");
        return ERR_ARG;
    }
    let ps = match pixel_size(pixel_format) {
        Some(ps) => ps,
        None => {
            set_last_error(
                ptr::null_mut(),
                "chua_tj_decompress: unsupported TJPF_* pixel format",
            );
            return ERR_ARG;
        }
    };
    *dst = ptr::null_mut();
    *width = 0;
    *height = 0;
    *pitch = 0;

    let handle = tjInitDecompress();
    if handle.is_null() {
        set_last_error(handle, "tjInitDecompress");
        return ERR_HANDLE;
    }
    let mut subsamp = 0;
    let mut colorspace = 0;
    let ret = tjDecompressHeader3(handle, jpeg, jpeg_size, width, height, &mut subsamp, &mut colorspace);
    if ret != 0 {
        set_last_error(handle, "tjDecompressHeader3");
        tjDestroy(handle);
        return ERR_TURBOJPEG;
    }
    let row_pitch = padded_pitch(*width, pixel_format).unwrap_or(*width * ps as c_int);
    let buffer = tjAlloc((row_pitch as usize) * ((*height).max(0) as usize));
    if buffer.is_null() {
        set_last_error(handle, "tjAlloc");
        tjDestroy(handle);
        return ERR_HANDLE;
    }
    let ret = tjDecompress2(handle, jpeg, jpeg_size, buffer, *width, row_pitch, *height, pixel_format, flags);
    tjDestroy(handle);
    if ret != 0 {
        tjFree(buffer);
        set_last_error(ptr::null_mut(), "tjDecompress2");
        *dst = ptr::null_mut();
        return ERR_TURBOJPEG;
    }
    *pitch = row_pitch;
    *dst = buffer;
    0
}
