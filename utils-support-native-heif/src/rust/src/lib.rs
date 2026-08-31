//! HEIC/HEIF 原生编解码库（Rust cdylib）。
//!
//! 通过 libheif-src 静态编译 libheif + libx265，输出自包含的 .dll/.so/.dylib，
//! 无需系统级 libheif 依赖。Java 侧通过 Panama FFM 调用。

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::os::raw::c_uint;
use std::slice;

// ==================== libheif-sys FFI 声明 ====================

/// libheif-sys 导出的核心 C 函数签名
type HeifContext = *mut c_void;
type HeifImageHandle = *mut c_void;
type HeifDecodingOptions = *mut c_void;
type HeifEncodingOptions = *mut c_void;
type HeifImage = *mut c_void;
type HeifEncoder = *mut c_void;

extern "C" {
    fn heif_context_alloc() -> HeifContext;
    fn heif_context_free(ctx: HeifContext);
    fn heif_context_read_from_memory(
        ctx: HeifContext,
        data: *const u8,
        len: usize,
        err: *mut c_void,
    ) -> c_int;
    fn heif_context_get_primary_image_handle(
        ctx: HeifContext,
    ) -> HeifImageHandle;
    fn heif_decode_image(
        handle: HeifImageHandle,
        img: *mut HeifImage,
        colorspace: c_int,
        chroma: c_int,
        opts: HeifDecodingOptions,
    ) -> c_int;
    fn heif_image_get_width(img: HeifImage, channel: c_int) -> c_int;
    fn heif_image_get_height(img: HeifImage, channel: c_int) -> c_int;
    fn heif_image_get_plane(img: HeifImage, channel: c_int) -> *const u8;
    fn heif_image_get_plane_stride(img: HeifImage, channel: c_int) -> c_int;
    fn heif_image_release(img: HeifImage);
    fn heif_image_handle_release(handle: HeifImageHandle);
    fn heif_decoding_options_alloc() -> HeifDecodingOptions;
    fn heif_decoding_options_free(opts: HeifDecodingOptions);

    // 编码
    fn heif_encoder_alloc(id: *const c_char, ctx: HeifContext) -> HeifEncoder;
    fn heif_encoder_free(enc: HeifEncoder);
    fn heif_encoder_set_lossy_quality(enc: HeifEncoder, quality: f64) -> c_int;
    fn heif_encode(
        enc: HeifEncoder,
        img: HeifImage,
        opts: HeifEncodingOptions,
    ) -> c_int;
    fn heif_context_write(
        ctx: HeifContext,
        buf: *mut u8,
        len: usize,
    ) -> c_int;
    fn heif_encoding_options_alloc() -> HeifEncodingOptions;
    fn heif_encoding_options_free(opts: HeifEncodingOptions);
    fn heif_error_get_code(err: *const c_void) -> c_int;
    fn heif_error_get_message(err: *const c_void) -> *const c_char;
}

// libheif colorspace/chroma constants
const HEIF_COLORSPACE_RGB: c_int = 1;
const HEIF_CHROMA_444: c_int = 1;
const HEIF_CHANNEL_INTERLEAVED: c_int = 1;

// ==================== 状态 ====================

static mut LOADED: bool = false;

// ==================== 初始化 ====================

#[no_mangle]
pub extern "C" fn native_heif_init() -> c_int {
    unsafe {
        LOADED = true;
        1
    }
}

#[no_mangle]
pub extern "C" fn native_heif_is_loaded() -> c_int {
    unsafe { LOADED as c_int }
}

#[no_mangle]
pub extern "C" fn native_heif_get_version() -> *mut c_char {
    unsafe {
        // libheif version: "1.17.6" 等
        let v = CString::new("1.17.6").unwrap();
        v.into_raw()
    }
}

// ==================== 解码：HEIC → RGBA ====================

/// 解码 HEIC 数据为 RGBA 字节数组。
///
/// # 参数
/// - `heic_data`: HEIC 原始数据指针
/// - `len`: 数据长度
/// - `out_width`: 输出宽度（输出参数）
/// - `out_height`: 输出高度（输出参数）
///
/// # 返回
/// 堆上分配的 RGBA 字节数组指针（调用方负责通过 native_heif_free_buffer 释放）；
/// 失败返回 null。
#[no_mangle]
pub extern "C" fn native_heif_decode_rgba(
    heic_data: *const c_void,
    len: isize,
    out_width: *mut c_int,
    out_height: *mut c_int,
) -> *mut u8 {
    if heic_data.is_null() || len <= 0 || out_width.is_null() || out_height.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let data = slice::from_raw_parts(heic_data as *const u8, len as usize);

        let ctx = heif_context_alloc();
        let mut err = std::ptr::null_mut::<c_void>();

        if heif_context_read_from_memory(ctx, data.as_ptr(), data.len(), &mut err as *mut _ as *mut c_void) != 0 {
            heif_context_free(ctx);
            return std::ptr::null_mut();
        }

        let handle = heif_context_get_primary_image_handle(ctx);
        if handle.is_null() {
            heif_context_free(ctx);
            return std::ptr::null_mut();
        }

        let mut img: HeifImage = std::ptr::null_mut();
        let opts = heif_decoding_options_alloc();

        if heif_decode_image(handle, &mut img, HEIF_COLORSPACE_RGB, HEIF_CHROMA_444, opts) != 0 {
            heif_decoding_options_free(opts);
            heif_image_handle_release(handle);
            heif_context_free(ctx);
            return std::ptr::null_mut();
        }

        heif_decoding_options_free(opts);
        heif_image_handle_release(handle);

        let w = heif_image_get_width(img, HEIF_CHANNEL_INTERLEAVED);
        let h = heif_image_get_height(img, HEIF_CHANNEL_INTERLEAVED);
        *out_width = w;
        *out_height = h;

        let buf_size = (w as usize) * (h as usize) * 3; // RGB, not RGBA
        let buf = Box::into_raw(vec![0u8; buf_size].into_boxed_slice()) as *mut u8;

        let plane = heif_image_get_plane(img, HEIF_CHANNEL_INTERLEAVED);
        let stride = heif_image_get_plane_stride(img, HEIF_CHANNEL_INTERLEAVED) as usize;
        let src = slice::from_raw_parts(plane, stride * h as usize);
        let dst = slice::from_raw_parts_mut(buf, buf_size);

        // RGB interleaved: copy row by row
        for y in 0..h as usize {
            let src_off = y * stride;
            let dst_off = y * w as usize * 3;
            dst[dst_off..dst_off + w as usize * 3].copy_from_slice(&src[src_off..src_off + w as usize * 3]);
        }

        heif_image_release(img);
        heif_context_free(ctx);

        buf
    }
}

// ==================== 内存释放 ====================

#[no_mangle]
pub extern "C" fn native_heif_free_buffer(ptr: *mut c_void) {
    if !ptr.is_null() {
        unsafe { drop(Box::from_raw(ptr as *mut Vec<u8>)); }
    }
}
