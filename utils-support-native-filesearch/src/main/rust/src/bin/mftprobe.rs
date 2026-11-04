//! 调试探针：直调 mft::collect，旁路 lib.rs 的搜索 JSON 层。
//!
//! 用法: mftprobe.exe <扫描根目录> [最大返回数] [全局 glob 模式]
//!
//! 只在 MFT_DEBUG=1 时才向 stderr 输出中间状态。

use file_search::mft::collect;

/// 文件名匹配谓词（等价于 lib.rs 的 glob_match）
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

    let root = args.get(1).map(|s| s.as_str()).unwrap_or("C:\\");
    let max_results = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100);
    let pattern: Option<&str> = args.get(3).map(|s| s.as_str());

    let matcher = |name: &str| pattern.map_or(true, |p| glob_match(name, p));
    let matcher = &matcher;

    match collect(root, max_results, matcher) {
        Ok(entries) => {
            for e in entries.iter() {
                println!("S {}", e.path);
            }
            println!("COUNT {}", entries.len());
        }
        Err(e) => {
            println!("MFT 失败: {}", e);
        }
    }
}
