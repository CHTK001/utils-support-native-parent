use std::ffi::{c_char, CStr, CString};
use std::os::raw::c_void;

const CAPABILITIES: &str = "resize,watermark,crop,rotate,filter,exclude,heic_detect";
const FILTER_CAPABILITIES: &str = "blur,sharpen,grayscale,flip,auto_orient";
static mut INITIALIZED: bool = false;

#[no_mangle]
pub extern "C" fn native_init() -> bool {
    unsafe { INITIALIZED = true; true }
}

#[no_mangle]
pub extern "C" fn native_is_initialized() -> bool {
    unsafe { INITIALIZED }
}

#[no_mangle]
pub extern "C" fn native_get_version() -> *mut c_char {
    unsafe { CString::new("1.1.0").unwrap().into_raw() }
}

#[no_mangle]
pub extern "C" fn native_get_capabilities() -> *mut c_char {
    unsafe { CString::new(CAPABILITIES).unwrap().into_raw() }
}

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

#[no_mangle]
pub extern "C" fn native_get_filter_capabilities() -> *mut c_char {
    unsafe { CString::new(FILTER_CAPABILITIES).unwrap().into_raw() }
}

#[no_mangle]
pub extern "C" fn native_get_filter_chain_json() -> *mut c_char {
    unsafe { CString::new("[]").unwrap().into_raw() }
}

#[no_mangle]
pub extern "C" fn native_is_excluded(path: *const c_char, extension: *const c_char) -> bool {
    unsafe {
        if path.is_null() || extension.is_null() { return false; }
        let ext = CStr::from_ptr(extension).to_string_lossy().to_lowercase();
        matches!(ext.as_str(), "tmp" | "temp" | "log" | "cache")
    }
}

#[no_mangle]
pub extern "C" fn native_heic_decode_to_png(
    _heic_data: *const c_void,
    _len: usize,
    _max_width: u32,
) -> *mut c_char {
    // HEIF/HEIC decoding requires libheif - return null placeholder
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn native_is_heic(data: *const c_void, len: usize) -> bool {
    if data.is_null() || len < 12 { return false; }
    unsafe {
        let bytes = std::slice::from_raw_parts(data as *const u8, len);
        if &bytes[4..8] != b"ftyp" { return false; }
        let brand = &bytes[8..12];
        matches!(brand, b"heic" | b"heix" | b"heim" | b"hevc" | b"mif1" | b"msf1")
    }
}

#[no_mangle]
pub extern "C" fn native_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)) };
    }
}
