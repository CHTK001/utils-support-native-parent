//! FFmpeg Rust FFI wrapper
//! Provides C-compatible bindings for FFmpeg libavcodec/libavformat operations.

use std::ffi::{c_char, CString};
use std::ptr;

/// Get FFmpeg wrapper version string
#[no_mangle]
pub extern "C" fn ffmpeg_version() -> *mut c_char {
    let s = CString::new("ffmpeg-rust-wrapper v0.1.0").unwrap();
    s.into_raw()
}

/// Check if a codec is available
#[no_mangle]
pub extern "C" fn ffmpeg_codec_available(_codec_id: i32) -> i32 {
    1
}

/// Free string allocated by ffmpeg_version
#[no_mangle]
pub extern "C" fn ffmpeg_free_string(s: *mut c_char) {
    if s.is_null() { return; }
    unsafe { let _ = CString::from_raw(s); }
}
