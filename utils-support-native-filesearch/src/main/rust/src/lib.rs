use std::ffi::{c_char, CStr, CString};
use std::os::raw::c_int;

mod mft;

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
     .replace('"', "\\\"")
     .replace('\n', "\\n")
     .replace('\r', "\\r")
     .replace('\t', "\\t")
}

fn glob_match(name: &str, pattern: &str) -> bool {
    if pattern == "*" { return true; }
    let nc: Vec<char> = name.chars().collect();
    let pc: Vec<char> = pattern.chars().collect();
    glob_match_impl(&nc, &pc, 0, 0)
}

fn glob_match_impl(name: &[char], pat: &[char], ni: usize, pi: usize) -> bool {
    if pi >= pat.len() { return ni >= name.len(); }
    if ni < name.len() {
        if pat[pi] == '*' {
            return glob_match_impl(name, pat, ni, pi+1) || glob_match_impl(name, pat, ni+1, pi);
        } else if pat[pi] == '?' || pat[pi] == name[ni] {
            return glob_match_impl(name, pat, ni+1, pi+1);
        }
    }
    false
}

fn get_last_modified(path: &std::path::Path) -> u64 {
    path.metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 搜索入口：Windows 优先走 NTFS MFT 直读，失败或非 Windows 回退 walkdir。
fn search_to_json(root: &str, pattern: Option<&str>, max_results: i32) -> String {
    #[cfg(windows)]
    {
        let matcher = |name: &str| pattern.map_or(true, |p| glob_match(name, p));
        if let Ok(entries) = mft::collect(root, max_results, &matcher) {
            return entries_to_json(&entries);
        }
    }
    search_to_json_walkdir(root, pattern, max_results)
}

/// 把 MFT 结果条目拼装为与 walkdir 一致的 JSON。
#[allow(dead_code)]
fn entries_to_json(entries: &[mft::SearchEntry]) -> String {
    let results: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "path": e.path,
                "size": e.size,
                "ext": e.ext,
                "modified": e.modified
            })
        })
        .collect();
    serde_json::json!({"rc": 0, "count": results.len(), "results": results}).to_string()
}

/// 递归遍历目录树并拼装搜索结果 JSON（walkdir 回退路径）。
///
/// <p>遍历深度固定为 3 层（见下方 {@code WalkDir::max_depth}），
/// 超深层级不会被收录；目录节点本身不进入结果，只收文件。</p>
fn search_to_json_walkdir(root: &str, pattern: Option<&str>, max_results: i32) -> String {
    use walkdir::WalkDir;
    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut count = 0i32;

    for entry in WalkDir::new(root)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if max_results > 0 && count >= max_results { break; }
        let path = entry.path();
        if path.components().count() <= 1 { continue; }
        if entry.file_type().is_dir() { continue; }

        if let Some(pat) = pattern {
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n, None => continue,
            };
            if !glob_match(name, pat) { continue; }
        }

        let size = entry.metadata().map(|m| m.len() as i64).unwrap_or(0);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let path_str = json_escape(&path.to_string_lossy().to_string().replace('\\', "/"));
        let modified = get_last_modified(path);

        results.push(serde_json::json!({
            "path": path_str,
            "size": size,
            "ext": ext,
            "modified": modified
        }));
        count += 1;
    }

    serde_json::json!({"rc": 0, "count": count, "results": results}).to_string()
}

// ==================== 已移除的 JNI 层 ====================
//
// 此处原有 getVersion / cancel / searchByName / getTree / searchBySize /
// searchByPath 六个 Java_... JNI 函数，但它们不符合 JNI 约定（缺少
// JNIEnv* / jclass 前导参数导致参数错位、getVersion 以 *const c_char 冒充
// jstring 返回并令 JVM 崩溃、回调形参被忽略、两个函数是返回 -1 的桩）。
// Java 侧 RustFileSearchBridge 已改走下方扁平 C ABI，这些符号不再被引用，
// 故整体删除以免误用。

// ==================== C ABI ====================

#[no_mangle]
pub unsafe extern "C" fn fast_get_version() -> *const c_char {
    b"1.0.0\0".as_ptr() as *const c_char
}

#[no_mangle]
pub unsafe extern "C" fn fast_search_cancel() {}

#[no_mangle]
pub unsafe extern "C" fn fast_search_by_name(
    root_dir: *const c_char, pattern: *const c_char, max_results: c_int,
    _callback: *mut std::os::raw::c_void,
) -> c_int {
    if root_dir.is_null() { return -1; }
    let root = CStr::from_ptr(root_dir).to_string_lossy().into_owned();
    let pat = if pattern.is_null() { None } else {
        Some(CStr::from_ptr(pattern).to_string_lossy().into_owned())
    };
    let json = search_to_json(&root, pat.as_deref(), max_results);
    serde_json::from_str::<serde_json::Value>(&json)
        .ok()
        .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
        .map(|c| c as c_int)
        .unwrap_or(-1)
}

// ==================== JSON 返回接口 ====================

#[no_mangle]
pub unsafe extern "system" fn Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawSearchByName(
    root_path: *const c_char,
    name_pattern: *const c_char,
    max_results: c_int,
) -> *mut c_char {
    if root_path.is_null() {
        return CString::new(r#"{"rc":-1,"error":"null root"}"#).unwrap().into_raw();
    }
    let root = CStr::from_ptr(root_path).to_string_lossy().into_owned();
    let pat = if name_pattern.is_null() { None } else {
        Some(CStr::from_ptr(name_pattern).to_string_lossy().into_owned())
    };
    let result = search_to_json(&root, pat.as_deref(), max_results);
    CString::new(result).unwrap().into_raw()
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_chua_filesearch_support_bridge_RustFileSearchBridge__rawGetTree(
    root_path: *const c_char,
    _max_depth: c_int,
    max_results: c_int,
) -> *mut c_char {
    if root_path.is_null() {
        return CString::new(r#"{"rc":-1,"error":"null root"}"#).unwrap().into_raw();
    }
    let root = CStr::from_ptr(root_path).to_string_lossy().into_owned();
    let result = search_to_json(&root, None, max_results);
    CString::new(result).unwrap().into_raw()
}

