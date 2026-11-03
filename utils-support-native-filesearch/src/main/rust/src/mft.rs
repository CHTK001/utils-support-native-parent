//! Windows NTFS MFT 直读搜索。
//!
//! <p>直接打开卷设备 {@code \\.\X:}，读取 NTFS 引导扇区与 {@code $MFT}，
//! 将整份 MFT 记录顺序读入连续 buffer 后，以 rayon 并行逐条解析，
//! 重建路径并返回文件大小、修改时间。仅 Windows 且通常需要管理员权限。</p>
//!
//! <p>解析失败时由调用方回退到 {@code walkdir}，因此本模块只保证"尽力而为"：
//! 任何异常都返回 {@link Err}，绝不 panic 到 FFI 边界。</p>

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use rayon::prelude::*;

#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;

/// 搜索结果条目（与 walkdir 路径共用的输出结构）。
pub struct SearchEntry {
    /// 完整路径（正斜杠）
    pub path: String,
    /// 文件大小（字节）
    pub size: u64,
    /// 最后修改时间（Unix 毫秒）
    pub modified: u64,
    /// 扩展名（小写，无扩展名时为空串）
    pub ext: String,
}

/// MFT 记录解析结果。
struct RecordInfo {
    /// 父目录 MFT 记录号
    parent: u64,
    /// 文件名
    name: String,
    /// 是否目录
    is_dir: bool,
    /// 大小（未命名 $DATA）
    size: u64,
    /// 修改时间（Unix 毫秒）
    modified: u64,
}

/// rayon 并行解析时每组的攒包结果。
struct ParChunk {
    dirs: Vec<(u64, RecordInfo)>,
    files: Vec<(u64, RecordInfo)>,
}

/// 非 Windows 平台：MFT 直读不可用。
#[cfg(not(windows))]
pub fn collect(
    _root: &str,
    _max_results: i32,
    _name_match: &(dyn Fn(&str) -> bool + Send + Sync),
) -> Result<Vec<SearchEntry>, String> {
    Err("NTFS MFT 直读仅支持 Windows".to_string())
}

/// Windows 平台：执行 MFT 直读搜索。
///
/// @param root        搜索根目录（盘符，如 `C:\` 或 `C:\Users`）
/// @param max_results 最大返回数，<= 0 表示不限
/// @param name_match  文件名匹配谓词（由调用方传入 glob 逻辑，Send）
/// @return 命中条目；解析失败返回 `Err` 供调用方回退 walkdir
#[cfg(windows)]
pub fn collect(
    root: &str,
    max_results: i32,
    name_match: &(dyn Fn(&str) -> bool + Send + Sync),
) -> Result<Vec<SearchEntry>, String> {
    /// 读取小端 u16 / u32 / u64 / i64 的辅助。
    fn u16_at(b: &[u8], o: usize) -> Option<u16> {
        if o + 2 <= b.len() {
            Some(u16::from_le_bytes([b[o], b[o + 1]]))
        } else {
            None
        }
    }
    fn u32_at(b: &[u8], o: usize) -> Option<u32> {
        if o + 4 <= b.len() {
            Some(u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]))
        } else {
            None
        }
    }
    fn u64_at(b: &[u8], o: usize) -> Option<u64> {
        if o + 8 <= b.len() {
            let mut a = [0u8; 8];
            a.copy_from_slice(&b[o..o + 8]);
            Some(u64::from_le_bytes(a))
        } else {
            None
        }
    }

    /// $MFT 的 $DATA 运行列表（逻辑簇号，簇数）。
    #[derive(Clone, Copy)]
    struct Run {
        lcn: i64,
        len: u64,
    }

    /// 把原始指针对象包一层 `Send`，让 worker 可以捧走指向 `data` 的指针。
    /// 每个 worker 只写 `data` 自己的非重叠区间，任务返回后指针便失效，
    /// 所以只有 Send 没有 Sync；参与 Join 后的 `data` 读写的是父线程。
    struct SendPtr(std::ptr::NonNull<u8>);

    unsafe impl Send for SendPtr {}

    impl SendPtr {
        /// 把改包装为：只返回 worker 负责区间的 `&mut [u8]`，
        /// worker 内部不直接触碰 `NonNull` 字段，避免闭包按 FIELD 拆分来搬运 `NonNull`
        /// （`NonNull<u8>` 不满足 `Send`，会复现 `SendPtr` 的 `Send` 检查失效）。
        fn slice(&self, lo: usize, hi: usize) -> &mut [u8] {
            unsafe { std::slice::from_raw_parts_mut(self.0.as_ptr().add(lo), hi - lo) }
        }
    }

    /// 解码 NTFS 运行列表（runlist）。
    fn decode_runs(data: &[u8]) -> Vec<Run> {
        let mut runs = Vec::new();
        let mut i = 0usize;
        let mut prev_lcn: i64 = 0;
        while i < data.len() {
            let header = data[i];
            i += 1;
            if header == 0 {
                break;
            }
            let len_bytes = (header & 0x0F) as usize;
            let off_bytes = (header >> 4) as usize;
            if len_bytes == 0 || i + len_bytes + off_bytes > data.len() {
                break;
            }
            let mut l: u64 = 0;
            for k in 0..len_bytes {
                l |= (data[i + k] as u64) << (8 * k);
            }
            i += len_bytes;
            let mut o: i64 = 0;
            for k in 0..off_bytes {
                o |= (data[i + k] as i64) << (8 * k);
            }
            if off_bytes > 0 && off_bytes < 8 {
                let shift = 64 - 8 * off_bytes;
                o = (o << shift) >> shift;
            }
            i += off_bytes;
            prev_lcn += o;
            runs.push(Run { lcn: prev_lcn, len: l });
        }
        runs
    }

    /// 应用 MFT 记录的更新序列（fixup）。
    fn apply_fixup(record: &mut [u8], sector_size: usize) {
        let usa_off = match u16_at(record, 0x04) {
            Some(v) => v as usize,
            None => return,
        };
        let usa_count = match u16_at(record, 0x06) {
            Some(v) => v as usize,
            None => return,
        };
        if usa_count == 0 || usa_off + usa_count * 2 > record.len() {
            return;
        }
        for i in 1..usa_count {
            let sector_end = i * sector_size;
            if sector_end >= record.len() || sector_end < 2 {
                break;
            }
            let src = usa_off + i * 2;
            record[sector_end - 2] = record[src];
            record[sector_end - 1] = record[src + 1];
        }
    }

    /// 遍历记录属性，返回 (类型, 是否非驻留, 属性起始偏移, 属性长度) 列表。
    fn attr_headers(record: &[u8]) -> Vec<(u32, bool, usize, usize)> {
        let mut out = Vec::new();
        let first = match u16_at(record, 0x14) {
            Some(v) => v as usize,
            None => return out,
        };
        let mut p = first;
        while p + 8 <= record.len() {
            let atype = match u32_at(record, p) {
                Some(v) => v,
                None => break,
            };
            if atype == 0xFFFF_FFFF {
                break;
            }
            let alen = match u32_at(record, p + 4) {
                Some(v) => v as usize,
                None => break,
            };
            if alen < 16 || p + alen > record.len() {
                break;
            }
            let non_res = record[p + 8] != 0;
            out.push((atype, non_res, p, alen));
            p += alen;
        }
        out
    }

    /// 文件时间（FILETIME，100ns since 1601）转 Unix 毫秒。
    fn filetime_to_unix_millis(ft: u64) -> u64 {
        if ft < 116_444_736_000_000_000 {
            return 0;
        }
        (ft - 116_444_736_000_000_000) / 10_000
    }

    /// 解析单条 MFT 记录。
    fn parse_record(record: &[u8]) -> Option<RecordInfo> {
        if record.len() < 48 || &record[0..4] != b"FILE" {
            return None;
        }
        if u16_at(record, 0x16)? & 0x0001 == 0 {
            return None;
        }
        if u64_at(record, 0x20)? != 0 {
            return None;
        }
        let is_dir = u16_at(record, 0x16)? & 0x0002 != 0;

        let mut parent: u64 = 0;
        let mut name = String::new();
        let mut dos_name = String::new();
        let mut have_name = false;
        let mut size: u64 = 0;
        let mut modified: u64 = 0;
        let mut have_std = false;

        for (atype, non_res, p, _alen) in attr_headers(record) {
            if atype == 0x10 && !non_res {
                let clen = u32_at(record, p + 16)? as usize;
                let coff = u16_at(record, p + 20)? as usize;
                if p + coff + clen <= record.len() && clen >= 36 {
                    let c = &record[p + coff..p + coff + clen];
                    modified = filetime_to_unix_millis(u64_at(c, 8)?);
                    have_std = true;
                }
            } else if atype == 0x30 && !non_res {
                let clen = u32_at(record, p + 16)? as usize;
                let coff = u16_at(record, p + 20)? as usize;
                if p + coff + clen <= record.len() && clen >= 66 {
                    let c = &record[p + coff..p + coff + clen];
                    let ns = c[0x41];
                    let nlen = c[0x40] as usize;
                    if 66 + nlen * 2 <= c.len() && nlen > 0 {
                        let units: Vec<u16> = (0..nlen)
                            .map(|k| u16::from_le_bytes([c[66 + k * 2], c[66 + k * 2 + 1]]))
                            .collect();
                        let decoded = String::from_utf16_lossy(&units);
                        // 命名空间 2 为 DOS 8.3 短名：仅在没有 Win32 长名时兜底
                        if ns == 2 {
                            if dos_name.is_empty() {
                                dos_name = decoded;
                            }
                        } else if !have_name {
                            parent = u64_at(c, 0).unwrap_or(0) & 0x0000_FFFF_FFFF_FFFF;
                            name = decoded;
                            have_name = true;
                            if !have_std {
                                modified = filetime_to_unix_millis(u64_at(c, 0x10).unwrap_or(0));
                            }
                        }
                    }
                }
            } else if atype == 0x80 && !is_dir {
                let name_len = record[p + 9] as usize;
                if name_len == 0 {
                    if non_res {
                        size = u64_at(record, p + 48).unwrap_or(0);
                    } else {
                        size = u32_at(record, p + 16).unwrap_or(0) as u64;
                    }
                }
            }
        }

        if !have_name {
            if dos_name.is_empty() {
                return None;
            }
            name = dos_name;
        }
        Some(RecordInfo {
            parent,
            name,
            is_dir,
            size,
            modified,
        })
    }

    /// 从 $MFT 自身的记录中解析未命名 $DATA 的运行列表与真实大小。
    fn parse_mft_data(record: &[u8]) -> Option<(Vec<Run>, u64)> {
        if record.len() < 48 || &record[0..4] != b"FILE" {
            return None;
        }
        for (atype, non_res, p, _alen) in attr_headers(record) {
            if atype == 0x80 && non_res {
                let name_len = record[p + 9] as usize;
                if name_len != 0 {
                    continue;
                }
                let runs_off = u16_at(record, p + 32)? as usize;
                let real_size = u64_at(record, p + 48)?;
                let start = p + runs_off;
                if start >= record.len() {
                    return None;
                }
                return Some((decode_runs(&record[start..]), real_size));
            }
        }
        None
    }

    /// 将整份 $MFT（层面直连）并行读入连续 buffer，为后续 rayon 并行解析做准备。
    ///
    /// 单线程 I/O 是扫描的最大瓶颈（本机 D: 盘上 2.2GB MFT 数据一次读死要 6.6s），
    /// 所以把 MFT 数据的逻辑区段按 worker 数均分，每个 worker 打开卷的一个共享只读句柄，
    /// 只读自己负责的区段，读写互不冲突。
    ///
    /// CHUA_MFT_SERIAL=1 时回退到原来的顺序读，用于对比数据一致性。
    fn read_all_mft_records(
        volume: &str,
        file: &mut File,
        mft_runs: &[Run],
        cluster_size: u64,
        record_size: usize,
        total_records: usize,
    ) -> Result<Vec<u8>, String> {
        let total_bytes = total_records as usize * record_size;
        if total_bytes == 0 {
            return Err("MFT 记录数为 0".to_string());
        }
        if std::env::var("CHUA_MFT_SERIAL").is_ok() {
            return read_all_mft_records_serial(file, mft_runs, cluster_size, record_size, total_bytes);
        }

    /// 原来的顺序读路径（按 extent 依次读入，不涉及多句柄/多段合并），用于数据一致性对比。
    fn read_all_mft_records_serial(
        file: &mut File,
        mft_runs: &[Run],
        cluster_size: u64,
        _record_size: usize,
        total_bytes: usize,
    ) -> Result<Vec<u8>, String> {
        let mut data = Vec::with_capacity(total_bytes);
        let mut got = 0usize;
        for run in mft_runs {
            if got >= total_bytes {
                break;
            }
            let run_len = (run.len as u64 * cluster_size) as usize;
            if run_len == 0 {
                continue;
            }
            let vol_off = (run.lcn as u64 * cluster_size) as usize;
            let take = if run_len < total_bytes - got {
                run_len
            } else {
                total_bytes - got
            };
            file.seek(SeekFrom::Start(vol_off as u64))
                .map_err(|e| format!("MFT extent 定位失败: {}", e))?;
            let mut buf = vec![0u8; take];
            file.read_exact(&mut buf)
                .map_err(|e| format!("读取 MFT extent 失败: {}", e))?;
            data.extend_from_slice(&buf);
            got += take;
        }
        if got != total_bytes {
            return Err(format!(
                "MFT 记录读取不完整：已读 {} 预期 {}",
                got, total_bytes
            ));
        }
        Ok(data)
    }
        // 计算每个 extent 在逻辑数据中的起始/长度，最后一个超出 real_size 的截断
        let mut extents: Vec<(u64, usize, usize)> = Vec::new();
        let mut log_off = 0usize;
        for r in mft_runs {
            if r.len <= 0 {
                continue;
            }
            let len = (r.len as u64) * cluster_size;
            let rest = total_bytes - log_off;
            let take = if rest < (len as usize) { rest } else { len as usize };
            extents.push(((r.lcn as u64) * cluster_size, log_off, take));
            log_off += take;
            if log_off >= total_bytes {
                break;
            }
        }
        if log_off != total_bytes {
            return Err(format!(
                "MFT 记录读取不完整：已读 {} 预期 {}",
                log_off, total_bytes
            ));
        }
        // 每 worker 负责等长的逻辑段；直接向预分配的 buffer 写，避免两份 MFT 同时存活
        // （一份 2.2GB 已提交 + 八份 worker 份 copy + 合并用 2.2GB 会复现 4.4GB 第二次提交失败）
        let n_workers = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(8);
        let per_worker = (total_bytes + n_workers - 1) / n_workers;

        // 只提交一次 2.2GB：所有 worker 向同一段空间偏移读写
        let mut data = Vec::with_capacity(total_bytes);
        // with_capacity 只保留容量不缩小 len；把 len 提到 total_bytes（u8 无 Drop 析构，
        // 且 worker 把 [0, total_bytes) 每个字节都写满才返回，所以 set_len 安全）
        unsafe { data.set_len(total_bytes) };
        let extents_b = extents.clone();
        let mut workers = Vec::with_capacity(n_workers);
        for w in 0..n_workers {
            let lo = per_worker * w;
            let hi = std::cmp::min(lo + per_worker, total_bytes);
            let extents_b = extents_b.clone();
            let volume_b = volume.to_string();
            // raw pointer：worker 区间非重叠，不在后续阶段访问 data
            let p = SendPtr(std::ptr::NonNull::new(data.as_mut_ptr()).unwrap());
            workers.push(std::thread::spawn(move || -> Result<(), String> {
                if hi <= lo {
                    return Ok(());
                }
                let mut file = std::fs::OpenOptions::new()
                    .read(true)
                    .share_mode(0x7)
                    .open(&volume_b)
                    .map_err(|e| format!("打开卷 {} 失败: {}", volume_b, e))?;
                let buf = p.slice(lo, hi);
                for (lcn_off, log_off, len) in extents_b {
                    if log_off >= hi {
                        break;
                    }
                    let ov_lo = std::cmp::max(lo, log_off);
                    let ov_hi = std::cmp::min(hi, log_off + len);
                    if ov_hi > ov_lo {
                        let take = ov_hi - ov_lo;
                        file.seek(SeekFrom::Start(
                            lcn_off as u64 + ov_lo as u64 - log_off as u64,
                        ))
                            .map_err(|e| format!("MFT extent 定位失败: {}", e))?;
                        file.read_exact(&mut buf[ov_lo - lo..ov_lo - lo + take])
                            .map_err(|e| format!("读取 MFT extent 失败: {}", e))?;
                    }
                }
                Ok(())
            }));
        }

        // 合并 worker 结果
        for handle in workers {
            handle
                .join()
                .map_err(|_| "worker 执行 panic".to_string())?
                .map_err(|e| format!("worker 读 MFT 失败: {}", e))?;
        }
        Ok(data)
    }

    /// 将单条 MFT 记录解析为目录/文件信息（由 rayon 并行调用）。
    fn parse_one_record(
        data: &[u8],
        rec_num: usize,
        record_size: usize,
        name_match: &(dyn Fn(&str) -> bool + Send + Sync),
        sector_size: usize,
    ) -> ParChunk {
        let off = rec_num * record_size;
        if off + record_size > data.len() {
            return ParChunk {
                dirs: Vec::new(),
                files: Vec::new(),
            };
        }
        let rec = &data[off..off + record_size];
        let mut chunk = ParChunk {
            dirs: Vec::new(),
            files: Vec::new(),
        };
        if rec.len() < 48 || !rec.starts_with(b"FILE") {
            return chunk;
        }
        let mut rec = rec.to_vec();
        apply_fixup(&mut rec, sector_size);
        if rec.len() < 48 || !rec.starts_with(b"FILE") {
            return chunk;
        }
        if u16_at(&rec, 0x16).map_or(true, |v| v & 0x0001 == 0) {
            return chunk;
        }
        if u64_at(&rec, 0x20).map_or(true, |v| v != 0) {
            return chunk;
        }
        if let Some(info) = parse_record(&rec) {
            if info.is_dir {
                chunk.dirs.push((rec_num as u64, info));
            } else if name_match(&info.name) {
                chunk.files.push((rec_num as u64, info));
            }
        }
        chunk
    }

    /// 自底向上拼目录路径（每个目录只计算一次，归入 path_cache）。
    fn dir_full_path(
        map: &HashMap<u64, RecordInfo>,
        path_cache: &mut HashMap<u64, String>,
        rec_num: u64,
    ) -> Option<String> {
        if let Some(p) = path_cache.get(&rec_num) {
            return Some(p.clone());
        }
        let info = map.get(&rec_num)?;
        let mut parts: Vec<&str> = Vec::with_capacity(8);
        parts.push(&info.name);
        let mut cur = info.parent;
        let mut guard = 0;
        loop {
            if cur == 5 {
                break;
            }
            let parent = map.get(&cur)?;
            parts.push(&parent.name);
            cur = parent.parent;
            guard += 1;
            if guard > 256 || parts.len() > 256 {
                return None;
            }
        }
        parts.reverse();
        if parts.is_empty() {
            return None;
        }
        let path = parts.join("/");
        path_cache.insert(rec_num, path.clone());
        Some(path)
    }

    // ---- 入口 ----

    // 规范化 root，解析盘符与可选子路径前缀
    let normalized = root.replace('\\', "/");
    let normalized = normalized.trim_end_matches('/');
    let bytes = normalized.as_bytes();
    if bytes.len() < 2 || bytes[1] != b':' {
        return Err(format!("不是盘符路径: {}", root));
    }
    let drive = (bytes[0] as char).to_ascii_uppercase();
    let volume = format!("\\\\.\\{}:", drive);
    let mut prefix: Option<String> = if normalized.len() > 2 {
        Some(normalized.to_string())
    } else {
        None
    };
    // 把 root 规范化成真实长路径：调用方可能传入 8.3 短名（如 C:\Users\RUNNER~1\...），
    // 而 MFT 记录里是 Win32 长名，不规范化会导致前缀过滤全部失配。
    if prefix.is_some() {
        if let Ok(real) = std::fs::canonicalize(&normalized) {
            let text = real.to_string_lossy().replace('\\', "/");
            let text = text.strip_prefix("//?/").unwrap_or(&text).to_string();
            prefix = Some(text);
        }
    }

    // 打开卷并读取引导扇区
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0x7)
        .open(&volume)
        .map_err(|e| format!("打开卷 {} 失败: {}", volume, e))?;
    let mut boot = vec![0u8; 512];
    file.seek(SeekFrom::Start(0))
        .and_then(|_| file.read_exact(&mut boot))
        .map_err(|e| format!("读取引导扇区失败: {}", e))?;
    if &boot[3..11] != b"NTFS    " {
        return Err(format!("{} 不是 NTFS 卷", volume));
    }
    let bytes_per_sector = u16_at(&boot, 0x0B).ok_or("读取 BPS 失败")? as u64;
    let sectors_per_cluster = boot[0x0D] as u64;
    if bytes_per_sector == 0 || sectors_per_cluster == 0 {
        return Err("NTFS 引导扇区几何参数非法".to_string());
    }
    let cluster_size = bytes_per_sector * sectors_per_cluster;
    let mft_lcn = u64_at(&boot, 0x30).ok_or("读取 MFT LCN 失败")?;
    let rec_raw = boot[0x40] as i8;
    let record_size = if rec_raw > 0 {
        rec_raw as u64 * cluster_size
    } else {
        1u64 << ((-rec_raw) as u32)
    };
    if record_size == 0 || record_size > 1 << 20 {
        return Err("MFT 记录大小非法".to_string());
    }
    let sector_size = bytes_per_sector as usize;

    // 读 $MFT 记录 0，取 MFT 数据运行列表
    let mut rec0 = vec![0u8; record_size as usize];
    file.seek(SeekFrom::Start(mft_lcn * cluster_size))
        .and_then(|_| file.read_exact(&mut rec0))
        .map_err(|e| format!("读取 $MFT 记录 0 失败: {}", e))?;
    let (mft_runs, mft_real_size) =
        parse_mft_data(&rec0).ok_or("解析 $MFT $DATA 失败")?;
    if mft_runs.is_empty() {
        return Err("$MFT 运行列表为空".to_string());
    }
    // 记录号用 usize：rayon 对有索引的 usize 范围提供 with_max_len，支持分段并行
    let total_records = (mft_real_size / record_size) as usize;
    if total_records == 0 {
        return Err("MFT 记录数为 0".to_string());
    }

    // 优化的核心：整份 MFT 顺序读入，再 rayon 并行解析
    let mft_data = read_all_mft_records(
        &volume,
        &mut file,
        &mft_runs,
        cluster_size,
        record_size as usize,
        total_records,
    )?;

    // 逐记录并行解析：每个 ParChunk 内部只有本地数据，无锁。

    // 逐记录并行解析：每个 ParChunk 内部只有本地数据，无锁。
    let parsed = (0..total_records)
        .into_par_iter()
        .with_max_len(512)
        .map(|rec_num| {
            parse_one_record(&mft_data, rec_num, record_size as usize, name_match, sector_size)
        })
        .collect::<Vec<ParChunk>>();

    // 合并：目录记录入 map、命中的文件入 matched（rayon 保留顺序）
    let mut map: HashMap<u64, RecordInfo> = HashMap::new();
    let mut matched: Vec<(u64, RecordInfo)> = Vec::with_capacity(total_records);
    for c in parsed {
        map.extend(c.dirs);
        matched.extend(c.files);
    }
    matched.sort_by_key(|(rec_num, _)| *rec_num);

    // 只保留 max_results 条（与 walkdir 退路语义一致）
    if max_results > 0 && matched.len() > max_results as usize {
        matched.truncate(max_results as usize);
    }

    // 每个目录路径只计算一次，文件直接复用前缀（避免 559K 条记录各自向上回溯父链）
    let mut path_cache: HashMap<u64, String> = HashMap::with_capacity(map.len());
    let mut entries: Vec<SearchEntry> = Vec::new();
    for (_rec_num, info) in &matched {
        let path = match dir_full_path(&map, &mut path_cache, info.parent) {
            Some(path) => path,
            None => continue,
        };
        let path = format!("{}:/{}", drive, format!("{}/{}", path, info.name));
        let ext = match info.name.rfind('.') {
            Some(idx) if idx + 1 < info.name.len() => info.name[idx + 1..].to_ascii_lowercase(),
            _ => String::new(),
        };
        let prefix_lower = prefix.as_ref().map(|p| p.to_ascii_lowercase());
        if let Some(pref) = &prefix_lower {
            if !path.to_ascii_lowercase().starts_with(pref) {
                continue;
            }
        }
        entries.push(SearchEntry {
            path,
            size: info.size,
            modified: info.modified,
            ext,
        });
        if max_results > 0 && entries.len() >= max_results as usize {
            break;
        }
    }

    Ok(entries)
}
