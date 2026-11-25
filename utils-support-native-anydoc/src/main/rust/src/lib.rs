//! anydoc-ffi: C-ABI dynamic-library wrapper around the firecrawl `anydoc`
//! crate (v0.2.5). Exposes document-to-Markdown conversion through a small
//! stable surface that any FFI consumer (Java FFM, ctypes, C/C#) can bind.
//!
//! Contract:
//! - Converters return a heap-allocated NUL-terminated UTF-8 C string on
//!   success (release with `anydoc_free_string`), or NULL on failure.
//! - On failure, `anydoc_last_error()` returns the message for THIS thread
//!   (valid until the next anydoc_* call on the same thread).
//! - No function panics across the FFI boundary (catch_unwind guard).

use std::cell::RefCell;
use std::ffi::{c_char, c_uchar, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::ptr;

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

fn set_last_error(msg: String) {
    let c = CString::new(msg)
        .unwrap_or_else(|_| CString::new("error message contained NUL").unwrap());
    LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(c));
}

/// Build the crate.
#[unsafe(no_mangle)]
pub extern "C" fn anydoc_version() -> *const c_char {
    concat!("anydoc-ffi 0.1.0 (anydoc 0.2.5)\0").as_ptr() as *const c_char
}

/// Last error message for the current thread, or NULL if none.
#[unsafe(no_mangle)]
pub extern "C" fn anydoc_last_error() -> *const c_char {
    LAST_ERROR.with(|slot| match slot.borrow().as_ref() {
        Some(c) => c.as_ptr(),
        None => ptr::null(),
    })
}

/// Free a string returned by anydoc_convert_file / anydoc_convert_bytes.
#[unsafe(no_mangle)]
pub extern "C" fn anydoc_free_string(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// Convert a document file (path in UTF-8) to GitHub-Flavored Markdown.
/// Format is detected from content; file extension is the fallback
/// (signature-less formats such as CSV rely on it).
#[unsafe(no_mangle)]
pub extern "C" fn anydoc_convert_file(path: *const c_char) -> *mut c_char {
    if path.is_null() {
        set_last_error("path pointer is null".to_string());
        return ptr::null_mut();
    }
    let path = unsafe { CStr::from_ptr(path) };
    let path = match path.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("path is not valid UTF-8".to_string());
            return ptr::null_mut();
        }
    };
    run(|| anydoc::to_markdown(Path::new(path)))
}

/// Convert an in-memory document to Markdown.
/// `format` may be NULL (auto-detect from content signature) or a C string
/// naming an extension: doc docx docm odt pdf ppt pptx pps pot rtf epub
/// xlsx xlsm xlsb xls ods odp csv.
#[unsafe(no_mangle)]
pub extern "C" fn anydoc_convert_bytes(
    data: *const c_uchar,
    len: usize,
    format: *const c_char,
) -> *mut c_char {
    if data.is_null() || len == 0 {
        set_last_error("input bytes are empty".to_string());
        return ptr::null_mut();
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    let format = if format.is_null() {
        None
    } else {
        let name = unsafe { CStr::from_ptr(format) }.to_string_lossy().into_owned();
        match anydoc::Format::from_extension(&name) {
            Some(f) => Some(f),
            None => {
                set_last_error(format!("unknown format name: {name}"));
                return ptr::null_mut();
            }
        }
    };
    run(|| anydoc::to_markdown_bytes(bytes, format))
}

fn run<F>(f: F) -> *mut c_char
where
    F: FnOnce() -> Result<String, anydoc::ConvertError>,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(markdown)) => match CString::new(markdown) {
            Ok(c) => c.into_raw(),
            Err(e) => {
                set_last_error(format!("markdown output contained NUL: {e}"));
                ptr::null_mut()
            }
        },
        Ok(Err(err)) => {
            set_last_error(err.to_string());
            ptr::null_mut()
        }
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "panic during conversion".to_string()
            };
            set_last_error(format!("panic: {msg}"));
            ptr::null_mut()
        }
    }
}
