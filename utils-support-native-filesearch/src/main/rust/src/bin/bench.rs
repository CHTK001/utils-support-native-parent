//! 性能基准测试：NTFS MFT 直读 vs walkdir 回退。
//!
//! 用法: bench.exe [搜索根目录] [glob 模式]
//! 示例: bench.exe D: *
//!
//! 需要管理员权限（打开 \\.\X: 卷设备）。

use file_search::mft::collect;

/// 文件名匹配谓词（等价于 lib.rs 的 glob_match，避免依赖内部函数）
fn glob_match(name: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let nc: Vec<char> = name.chars().collect();
    let pc: Vec<char> = pattern.chars().collect();
    glob_match_impl(&nc, &pc, 0, 0)
}

fn glob_match_impl(name: &[char], pat: &[char], ni: usize, pi: usize) -> bool {
    if pi >= pat.len() {
        return ni >= name.len();
    }
    if ni < name.len() {
        if pat[pi] == '*' {
            return glob_match_impl(name, pat, ni, pi + 1)
                || glob_match_impl(name, pat, ni + 1, pi);
        } else if pat[pi] == '?' || pat[pi] == name[ni] {
            return glob_match_impl(name, pat, ni + 1, pi + 1);
        }
    }
    false
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = if args.len() > 1 { args[1].clone() } else { "D:".to_string() };
    let pattern = if args.len() > 2 { args[2].clone() } else { "*".to_string() };
    let max_results: i32 = if args.len() > 3 {
        args[3].parse().unwrap_or(0)
    } else {
        0
    };

    let matcher = |name: &str| glob_match(name, &pattern); // Send + Sync
    let matcher = &matcher;

    let t0 = std::time::Instant::now();
    let entries = match collect(&root, max_results, matcher) {
        Ok(e) => e,
        Err(e) => {
            println!("MFT 失败: {}", e);
            return;
        }
    };
    println!("MFT 直读: {:.2?}  | 条目数: {}", t0.elapsed(), entries.len());

    // walkdir 回退（与 lib.rs 的 search_to_json_walkdir 等价的逻辑）
    let t1 = std::time::Instant::now();
    let mut results: Vec<String> = Vec::new();
    use walkdir::WalkDir;
    for entry in WalkDir::new(&root)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.components().count() <= 1 {
            continue;
        }
        if entry.file_type().is_dir() {
            continue;
        }
        if !glob_match(
            path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            &pattern,
        ) {
            continue;
        }
        results.push(path.to_string_lossy().replace('\\', "/"));
        if max_results > 0 && results.len() >= max_results as usize {
            break;
        }
    }
    println!("walkdir 回退: {:.2?}  | 条目数: {}", t1.elapsed(), results.len());
}
