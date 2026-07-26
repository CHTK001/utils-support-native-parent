use std::ffi::CString;
use std::os::raw::c_char;
use std::time::Instant;

fn run_bench(label: &str, root: &str, max_results: i32) -> i64 {
    let start = Instant::now();
    let count = unsafe {
        rust_filesearch::fast_scan(
            CString::new(root).unwrap().as_ptr(),
            max_results,
            Some(null_callback),
        )
    };
    let elapsed = start.elapsed().as_millis() as i64;
    println!("  {:<25} count={:<8} time={}ms", label, count, elapsed);
    elapsed
}

unsafe extern "C" fn null_callback(
    _path: *const c_char,
    _ext: *const c_char,
    _size: i64,
    _alloc: i64,
    _mtime: u64,
    _usn: u64,
    _parent: u64,
    _plen: i32,
    _is_dir: i32,
    _elen: i32,
    _attrs: u32,
) {
}

fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| "C:".to_string());
    let max_results: i32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(-1);

    println!("Benchmark: root={}, max_results={}", root, max_results);
    println!();

    // warmup
    let _ = run_bench("预热 (warmup)", &root, 10);

    // Mode 1 – full scan
    let t1 = run_bench("全量扫描 (fast_scan)", &root, max_results);

    // Mode 2 – second run (cached)
    let t2 = run_bench("全量扫描 (2nd run)", &root, max_results);

    if max_results < 0 {
        let speed = if t1 > 0 {
            let total_files = 0; // we don't have the count for speed calc
            format!("N/A")
        } else {
            "N/A".to_string()
        };
    }

    println!();
    println!("Done.");
}
