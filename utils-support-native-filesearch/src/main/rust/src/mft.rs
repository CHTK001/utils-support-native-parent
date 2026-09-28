//! Windows NTFS MFT 直读搜索。
//!
//! <p>直接打开卷设备 {@code \\.\X:}，读取 NTFS 引导扇区与 {@code $MFT}，
//! 逐条解析 MFT 记录（$STANDARD_INFORMATION / $FILE_NAME / 未命名 $DATA），
//! 重建完整路径并返回文件大小、修改时间。仅 Windows 且通常需要管理员权限。</p>
//!
//! <p>解析失败时由调用方回退到 {@code walkdir}，因此本模块只保证“尽力而为”：
//! 任何异常都返回 {@link Err}，绝不 panic 到 FFI 边界。</p>

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

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

/// 非 Windows 平台：MFT 直读不可用。
#[cfg(not(windows))]
pub fn collect(
    _root: &str,
    _max_results: i32,
    _name_match: &dyn Fn(&str) -> bool,
) -> Result<Vec<SearchEntry>, String> {
    Err("NTFS MFT 直读仅支持 Windows".to_string())
}

/// Windows 平台：执行 MFT 直读搜索。
///
/// @param root        搜索根目录（盘符，如 `C:\` 或 `C:\Users`）
/// @param max_results 最大返回数，<= 0 表示不限
/// @param name_match  文件名匹配谓词（由调用方传入 glob 逻辑）
/// @return 命中条目；解析失败返回 `Err` 供调用方回退 walkdir
#[cfg(windows)]
pub fn collect(
    root: &str,
    max_results: i32,
    name_match: &dyn Fn(&str) -> bool,
) -> Result<Vec<SearchEntry>, String> {
    use std::collections::HashMap;
    use std::os::windows::fs::OpenOptionsExt;

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

    /// $MFT 顺序预取读取器。
    ///
    /// 原实现对每条记录单独 `seek` + 读 1024 字节，141 万条记录即 141 万次系统调用。
    /// 而 $MFT 在卷上是按 extent 顺序连续排列的，顺序读的实际吞吐远高于随机定位，
    /// 因此改成沿 extent 顺序一次预取一整块，主循环从内存切片取记录。
    ///
    /// 语义与原逐条 `seek` + 1024 字节读的实现完全一致，只是把「随机定位单条」
    /// 换成「顺序批量」。块大小取 2 MiB：既能摊薄系统调用开销，
    /// 又不会为小卷浪费内存。
    struct StreamPrefetcher<'a> {
        /// 底层卷句柄
        file: &'a mut File,
        /// $MFT 的 extent 列表
        runs: &'a [Run],
        /// 簇大小（字节）
        cluster_size: u64,
        /// 单条记录大小（字节）
        record_size: usize,
        /// 预取块大小（字节）
        block_size: usize,
        /// 预取块覆盖的数据流起始偏移
        block_off: u64,
        /// 预取块内已填充的字节数
        block_len: usize,
        /// 预取块缓冲
        block: Vec<u8>,
        /// I/O 错误：读失败时记录下来，交给调用方回退 walkdir
        io_err: Option<String>,
    }

    impl<'a> StreamPrefetcher<'a> {
        /// 创建一个预取器。
        ///
        /// @param file         卷句柄
        /// @param runs         $MFT 的 extent 列表
        /// @param cluster_size 簇大小（字节）
        /// @param record_size  单条记录大小（字节）
        fn new(
            file: &'a mut File,
            runs: &'a [Run],
            cluster_size: u64,
            record_size: usize,
        ) -> Self {
            // 块至少要装下 1 条记录，且向上取整到 4 KiB 以贴合页大小
            let mut block_size = (2usize << 20).max(record_size);
            block_size = block_size.next_multiple_of(4096);
            StreamPrefetcher {
                file,
                runs,
                cluster_size,
                record_size,
                block_size,
                block_off: 0,
                block_len: 0,
                // 缓冲只分配一次：fill 只覆写实际读到的区间，不重新清零，
                // 避免每条记录都 memset 整块
                block: vec![0u8; block_size],
                io_err: None,
            }
        }

        /// 把一条记录拷贝进调用方提供的缓冲。
        ///
        /// 复用同一个缓冲，避免 142 万条记录各分配一次 1KB。
        /// 记录内容在本次调用后即失效，解析结果由 `parse_record` 拷成自有数据。
        ///
        /// @param rec_num 记录序号
        /// @param buf     复用缓冲（内部按记录大小调整长度）
        /// @return 成功读到完整记录返回 `true`
        fn record_into(&mut self, rec_num: u64, buf: &mut Vec<u8>) -> bool {
            let off = match rec_num.checked_mul(self.record_size as u64) {
                Some(v) => v,
                None => return false,
            };
            if !(off >= self.block_off && off + self.record_size as u64 <= self.block_off + self.block_len as u64)
            {
                if self.fill(off).is_none() {
                    return false;
                }
            }
            let start = (off - self.block_off) as usize;
            if start + self.record_size > self.block_len {
                return false;
            }
            buf.clear();
            buf.extend_from_slice(&self.block[start..start + self.record_size]);
            true
        }

        /// 预取覆盖 `off` 起始的一个块。
        ///
        /// 一次尽量读满整个块（`block_size`），而不是只读调用方要的那几条 ——
        /// 调用方总是顺序遍历记录，下一次命中本块即可摊薄系统调用开销。
        /// 跨 extent 边界时把剩余部分接到下一个 extent，读不满则按实际长度收尾。
        ///
        /// 不重新分配也不清零缓冲：只有 `[0, out)` 区间被读入的新数据覆盖，
        /// 越界部分由 `block_len` 拦住，外部读不到上一块的残留内容。
        ///
        /// 读失败不是「流到尾」，必须记进 `io_err` 让调用方回退 walkdir，
        /// 否则会静默少返回一批记录。
        ///
        /// @param off 数据流偏移（字节）
        /// @return 填充到任意长度返回 `Some`；I/O 失败或完全无数据返回 `None`
        fn fill(&mut self, off: u64) -> Option<bool> {
            let mut out = 0usize;
            let want = self.block_size;
            let mut cur = off;
            while out < want {
                // 流已到尾时保留已读到的部分并收尾：不能把整块丢掉，
                // 否则最后一个不满的块里、已读入的记录会被静默跳过
                let (vol_off, avail) = match self.locate(cur) {
                    Some(v) => v,
                    None => break,
                };
                if avail == 0 {
                    break;
                }
                // avail 是 u64（extent 内剩余字节数），切片索引需要 usize
                let avail = avail.min((want - out) as u64) as usize;
                if avail == 0 {
                    break;
                }
                if let Err(e) = self.file.seek(SeekFrom::Start(vol_off)) {
                    self.io_err = Some(format!("seek {} 失败: {}", vol_off, e));
                    return None;
                }
                let mut got = 0usize;
                while got < avail {
                    match self.file.read(&mut self.block[out + got..out + avail]) {
                        Ok(0) => break,
                        Ok(n) => got += n,
                        Err(e) => {
                            self.io_err =
                                Some(format!("读取卷偏移 {} 失败: {}", vol_off, e));
                            break;
                        }
                    }
                }
                if got == 0 {
                    break;
                }
                out += got;
                cur += got as u64;
            }
            self.block_off = off;
            self.block_len = out;
            Some(out > 0)
        }

        /// 把数据流偏移映射为卷内物理偏移与该处可用字节数。
        ///
        /// @param off 数据流偏移（字节）
        /// @return `(卷内偏移, 该 extent 剩余字节数)`；越界返回 `None`
        fn locate(&self, mut off: u64) -> Option<(u64, u64)> {
            for run in self.runs {
                let run_bytes = run.len * self.cluster_size;
                if off < run_bytes {
                    return Some(((run.lcn as u64) * self.cluster_size + off, run_bytes - off));
                }
                off -= run_bytes;
            }
            None
        }
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
    let total_records = mft_real_size / record_size;
    if total_records == 0 {
        return Err("MFT 记录数为 0".to_string());
    }

    // 遍历 MFT，收集记录元数据
    let dbg = std::env::var("CHUA_MFT_DEBUG").is_ok();
    let mut n_read: u64 = 0;
    let mut n_file: u64 = 0;
    let mut n_inuse: u64 = 0;
    let mut n_base0: u64 = 0;
    let mut n_named: u64 = 0;
    let mut map: HashMap<u64, RecordInfo> = HashMap::with_capacity((total_records as usize).min(1 << 20));
    // 命中文件名模式的文件条目。文件是路径的叶子、不是路径组件，
    // 因此无需进 map（map 只服务 build_path 的父链回溯）；
    // 不命中模式的文件连 String 都不用留。
    let mut matched: Vec<(u64, RecordInfo)> = Vec::new();
    // 复用记录缓冲：142 万条记录逐条新建 1KB Vec 会产生等量堆分配
    let mut rec = vec![0u8; record_size as usize];
    let mut pf = StreamPrefetcher::new(&mut file, &mft_runs, cluster_size, record_size as usize);
    for rec_num in 0..total_records {
        // 系统文件（0..16）不必输出，但根目录 5 需保留用于拼路径
        // 记录读取改为顺序预取：预取块未命中才补读，不再逐条 seek
        if !pf.record_into(rec_num, &mut rec) {
            // 预取块未覆盖（理论上不会发生，块大小恒 ≥ 1 条记录）或流已到尾：
            // 保持与原实现一致的容错语义——短记录跳过，不中断整轮扫描
            continue;
        }
        if rec.len() < record_size as usize {
            continue;
        }
        n_read += 1;
        apply_fixup(&mut rec, sector_size);
        if rec.len() < 4 || &rec[0..4] != b"FILE" {
            continue;
        }
        n_file += 1;
        if u16_at(&rec, 0x16).map_or(true, |v| v & 0x0001 == 0) {
            continue;
        }
        n_inuse += 1;
        if u64_at(&rec, 0x20).map_or(true, |v| v != 0) {
            continue;
        }
        n_base0 += 1;
        if let Some(info) = parse_record(&rec) {
            if info.is_dir {
                map.insert(rec_num, info);
            } else if name_match(&info.name) {
                matched.push((rec_num, info));
            }
            n_named += 1;
        }
    }
    // 预取过程中的 I/O 失败不能当作「流到尾」：原实现遇错即返回 Err 由调用方
    // 回退 walkdir，静默继续会少返回一批记录
    if let Some(e) = pf.io_err.take() {
        return Err(format!("读取 MFT 失败: {}", e));
    }
    if dbg {
        eprintln!(
            "[mft] bps={} spc={} cluster={} mft_lcn={} rec_size={} total={} real={} runs={}",
            bytes_per_sector,
            sectors_per_cluster,
            cluster_size,
            mft_lcn,
            record_size,
            total_records,
            mft_real_size,
            mft_runs.len()
        );
        eprintln!(
            "[mft] read={} file_sig={} inuse={} base0={} named={} dir={} matched={}",
            n_read, n_file, n_inuse, n_base0, n_named, map.len(), matched.len()
        );
    }

    /// 自底向上拼路径。
    ///
    /// 起点条目自身不入 `map`（`map` 只存目录），故由调用方直接给出起始条目，
    /// 回溯从它的父记录开始；根记录 5 只用作终止标记，不计入路径。
    ///
    /// @param map   目录记录表（父链回溯用）
    /// @param drive 盘符
    /// @param info  起始条目
    /// @return 完整路径；父链断裂或超深返回 `None`
    fn build_path<'a>(
        map: &'a HashMap<u64, RecordInfo>,
        drive: char,
        info: &'a RecordInfo,
    ) -> Option<String> {
        let mut parts: Vec<&'a str> = Vec::with_capacity(8);
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
        Some(format!("{}:/{}", drive, parts.join("/")))
    }

    let mut entries: Vec<SearchEntry> = Vec::new();
    // 前缀只与路径形态有关，循环外小写化一次即可
    let prefix_lower = prefix.as_ref().map(|p| p.to_ascii_lowercase());
    // 只遍历命中集：map 现在只有目录，文件名模式已在遍历阶段判定过
    for (rec_num, info) in &matched {
        if *rec_num < 16 {
            continue;
        }
        let path = match build_path(&map, drive, info) {
            Some(p) => p,
            None => continue,
        };
        if let Some(pref) = &prefix_lower {
            if !path.to_ascii_lowercase().starts_with(pref) {
                continue;
            }
        }
        let ext = match info.name.rfind('.') {
            Some(idx) if idx + 1 < info.name.len() => info.name[idx + 1..].to_ascii_lowercase(),
            _ => String::new(),
        };
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
