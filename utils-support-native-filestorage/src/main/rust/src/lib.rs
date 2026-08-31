//! Rust 文件存储处理器 native 库
//! 提供文件处理能力的 native 实现，含 HEIC/HEIF 预览转码

use std::ffi::{c_char, CStr, CString};
use std::os::raw::c_void;

// ==================== 内部状态 ====================

/// 文件存储处理器能力列表
const CAPABILITIES: &str = "resize,watermark,crop,rotate,filter,exclude,heic_decode";

/// 图像过滤器能力列表
const FILTER_CAPABILITIES: &str = "blur,sharpen,grayscale,flip,auto_orient";

/// 初始化状态
static mut INITIALIZED: bool = false;

// ==================== JNI 接口 ====================

/// 初始化
#[no_mangle]
pub extern "C" fn native_init() -> bool {
    unsafe {
        INITIALIZED = true;
        true
    }
}

/// 检查是否已初始化
#[no_mangle]
pub extern "C" fn native_is_initialized() -> bool {
    unsafe { INITIALIZED }
}

/// 获取版本
#[no_mangle]
pub extern "C" fn native_get_version() -> *mut c_char {
    unsafe {
        CString::new("1.1.0").unwrap().into_raw()
    }
}

/// 获取能力列表
#[no_mangle]
pub extern "C" fn native_get_capabilities() -> *mut c_char {
    unsafe {
        CString::new(CAPABILITIES).unwrap().into_raw()
    }
}

/// 解析参数 JSON
#[no_mangle]
pub extern "C" fn native_parse_params(params_json: *const c_char) -> *mut c_char {
    unsafe {
        if params_json.is_null() {
            return CString::new("{}").unwrap().into_raw();
        }
        let params = CStr::from_ptr(params_json).to_string_lossy().to_string();
        CString::new(params).unwrap().into_raw()
    }
}

/// 获取过滤器能力
#[no_mangle]
pub extern "C" fn native_get_filter_capabilities() -> *mut c_char {
    unsafe {
        CString::new(FILTER_CAPABILITIES).unwrap().into_raw()
    }
}

/// 获取过滤器链 JSON
#[no_mangle]
pub extern "C" fn native_get_filter_chain_json() -> *mut c_char {
    unsafe {
        CString::new("[]").unwrap().into_raw()
    }
}

/// 检查是否被排除
#[no_mangle]
pub extern "C" fn native_is_excluded(path: *const c_char, extension: *const c_char) -> bool {
    unsafe {
        if path.is_null() || extension.is_null() {
            return false;
        }
        let ext = CStr::from_ptr(extension).to_string_lossy().to_lowercase();
        matches!(ext.as_str(), "tmp" | "temp" | "log" | "cache")
    }
}

// ==================== HEIC/HEIF 解码 ====================

/// 将 HEIC/HEIF 图片解码为 PNG 字节数组。
///
/// # 参数
/// - `heic_data`: HEIC/HEIF 原始数据指针
/// - `len`: 数据长度
/// - `max_width`: 缩放目标最大宽度（0 表示不缩放）
///
/// # 返回
/// 解码后的 PNG 字节数组，通过 native_free_string 释放；失败返回 null。
#[no_mangle]
pub extern "C" fn native_heic_decode_to_png(
    heic_data: *const c_void,
    len: usize,
    max_width: u32,
) -> *mut c_char {
    if heic_data.is_null() || len == 0 {
        return std::ptr::null_mut();
    }
    unsafe {
        let data = std::slice::from_raw_parts(
            heic_data as *const u8,
            len,
        );
        match heif::HeifContext::read_from(data) {
            Ok(context) => {
                match context.primary_image_handle() {
                    Ok(handle) => {
                        let mut dec = match heif::Decoder::new(&handle) {
                            Ok(d) => d,
                            Err(e) => {
                                eprintln!("heif decode init failed: {:?}", e);
                                return std::ptr::null_mut();
                            }
                        };
                        let img = match dec.decode_image(heif::ColorSpace::Rgb, heif::Chroma::RGBA) {
                            Ok(i) => i,
                            Err(e) => {
                                eprintln!("heif decode failed: {:?}", e);
                                return std::ptr::null_mut();
                            }
                        };
                        let (width, height) = (img.width(), img.height());
                        let scaled = if max_width > 0 && width > max_width {
                            match img.scale_image(max_width, (max_width as f64 * height as f64 / width as f64) as u32, heif::ColorSpace::Rgb, heif::Chroma::RGBA) {
                                Ok(s) => s,
                                Err(e) => {
                                    eprintln!("heif scale failed: {:?}", e);
                                    return std::ptr::null_mut();
                                }
                            }
                        } else {
                            img
                        };
                        match scaled.write_as_png(None) {
                            Ok(png_bytes) => CString::into_raw(CString::new(png_bytes).unwrap()),
                            Err(e) => {
                                eprintln!("heif write png failed: {:?}", e);
                                std::ptr::null_mut()
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("heif primary image handle failed: {:?}", e);
                        std::ptr::null_mut()
                    }
                }
            }
            Err(e) => {
                eprintln!("heif read failed: {:?}", e);
                std::ptr::null_mut()
            }
        }
    }
}

/// 判断给定字节数据是否为 HEIC/HEIF 格式。
#[no_mangle]
pub extern "C" fn native_is_heic(data: *const c_void, len: usize) -> bool {
    if data.is_null() || len < 12 {
        return false;
    }
    unsafe {
        let bytes = std::slice::from_raw_parts(data as *const u8, len);
        // ftyp box type check: bytes[4..8] should be "ftyp"
        if &bytes[4..8] != b"ftyp" {
            return false;
        }
        // brand check: bytes[8..12]
        let brand = &bytes[8..12];
        matches!(brand, b"heic" | b"heix" | b"heim" | b"hevc" | b"mif1" | b"msf1")
    }
}

// ==================== Free 函数 ====================

#[no_mangle]
pub extern "C" fn native_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)) };
    }
}
