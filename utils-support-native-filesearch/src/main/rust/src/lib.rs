use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_longlong};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
fn open_volume_device(path: &str) -> Option<std::fs::File> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    let result = OpenOptions::new()
        .read(true)
        .share_mode(0x3)
        .open(path);
    match &result {
        Ok(_) => eprintln!("[mft] open_volume_device SUCCESS for {}", path),
        Err(e) => eprintln!("[mft] open_volume_device FAILED for {}: {}", path, e),
    }
    result.ok()
}

pub type FileResultCallback = unsafe extern "C" fn(
    path: *const c_char,
    extension: *const c_char,
    size: c_longlong,
    allocated_size: c_longlong,
    last_modified: u64,
    usn_record_id: u64,
    parent_file_id: u64,
    path_len: c_int,
    is_directory: c_int,
    extension_len: c_int,
    attributes: u32,
);

static CANCELLED: AtomicBool = AtomicBool::new(false);

const NTFS_BLOCK_SIZE: usize = 512;

// ==================== Windows: NTFS MFT 顺序扫描 ====================

#[cfg(windows)]
mod platform {
    use super::*;
    use byteorder::{ByteOrder, LittleEndian};
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};
    use jwalk::WalkDir;
    use rayon::prelude::*;

    struct MftEntry {
        name: String,
        parent_frn: u64,
        is_dir: bool,
        data_size: i64,
        allocated_size: i64,
        nt_time: u64,
        attrs: u32,
    }

    pub fn search_files<F>(
        root: &str,
        name_pattern: Option<&str>,
        min_size: i64,
        max_size: i64,
        max_results: i32,
        mut callback: F,
    ) -> i32
    where
        F: FnMut(*const c_char, *const c_char, c_longlong, c_longlong, u64, u64, u64, c_int, c_int, c_int, u32),
    {
        let trimmed_root = root.trim_end_matches(|c| c == '/' || c == '\\');
        let has_volume = trimmed_root.len() >= 2
            && trimmed_root.as_bytes().get(0).map_or(false, |b| b.is_ascii_alphabetic())
            && trimmed_root.as_bytes().get(1) == Some(&b':');

        if !has_volume {
            return search_files_walkdir(root, name_pattern, min_size, max_size, max_results, callback);
        }

        eprintln!("[mft] has_volume_letter=true, trying MFT scan for root={}", root);
        let mft_result = try_mft_scan(root, trimmed_root, name_pattern, min_size, max_size, max_results, &mut callback);
        if mft_result > 0 {
            eprintln!("[mft] MFT scan returned {}", mft_result);
            return mft_result;
        }
        eprintln!("[mft] MFT scan returned {} (<=0), falling back to walkdir", mft_result);
        // MFT 解析可能跑通但拿到 0 条（flags 校验过严、parent_frn 缺失、$FILE_NAME 解析失败等），
        // 此时不能当成"成功"，必须降级到 walkdir 才能给出真实结果。
        search_files_walkdir(root, name_pattern, min_size, max_size, max_results, callback)
    }

    /// 尝试 MFT 顺序扫描（直接读卷设备，完全绕过 ntfs crate）
    fn try_mft_scan<F>(
        _root: &str,
        trimmed_root: &str,
        name_pattern: Option<&str>,
        min_size: i64,
        max_size: i64,
        max_results: i32,
        callback: &mut F,
    ) -> i32
    where
        F: FnMut(*const c_char, *const c_char, c_longlong, c_longlong, u64, u64, u64, c_int, c_int, c_int, u32),
    {
        let volume = &trimmed_root[..1];
        let device_path = format!(r"\\.\{}:", volume);
        let mut device = match open_volume_device(&device_path) {
            Some(f) => f,
            None => return -1,
        };

        let boot = match read_exact_bytes(&mut device, 512) {
            Some(b) => b,
            None => { eprintln!("[mft] FAIL: read boot block"); return -1; }
        };

        if &boot[3..11] != b"NTFS    " {
            eprintln!("[mft] FAIL: not NTFS (sig={:?})", &boot[3..11]); return -1;
        }

        let bytes_per_sector = LittleEndian::read_u16(&boot[0x0B..0x0D]) as u64;
        let sectors_per_cluster = boot[0x0D] as u64;
        let cluster_size = bytes_per_sector * sectors_per_cluster;
        let mft_lcn = LittleEndian::read_u64(&boot[0x30..0x38]);

        let clusters_per_mft = boot[0x40] as i8;
        let record_size = if clusters_per_mft >= 0 {
            (cluster_size * clusters_per_mft as u64) as usize
        } else {
            (1u64 << (-clusters_per_mft as u64)) as usize
        };

        eprintln!("[mft] bps={} spc={} cluster={} mft_lcn={} clusters_per_mft={} record_size={}",
            bytes_per_sector, sectors_per_cluster, cluster_size, mft_lcn, clusters_per_mft, record_size);

        if record_size < 512 || record_size > 65536 || cluster_size == 0 {
            eprintln!("[mft] FAIL: bad params"); return -1;
        }

        let mft_record_offset = mft_lcn * cluster_size;
        if device.seek(SeekFrom::Start(mft_record_offset)).is_err() {
            eprintln!("[mft] FAIL: seek to MFT record offset {}", mft_record_offset); return -1;
        }
        let first_record = match read_exact_bytes(&mut device, record_size) {
            Some(b) => b,
            None => { eprintln!("[mft] FAIL: read MFT record"); return -1; }
        };

        if &first_record[0..4] != b"FILE" {
            eprintln!("[mft] FAIL: first record not FILE (sig={:?})", &first_record[0..4]); return -1;
        }

        let data_runs = match parse_mft_data_runs(&first_record, record_size) {
            Some(r) => r,
            None => { eprintln!("[mft] FAIL: parse data runs"); return -1; }
        };

        let total_mft_size = match get_data_size(&first_record, record_size) {
            Some(s) => s,
            None => { eprintln!("[mft] FAIL: get data size"); return -1; }
        };

        let mut mft_data = match read_mft_via_runs(&mut device, &data_runs, cluster_size, total_mft_size) {
            Some(d) => d,
            None => { eprintln!("[mft] FAIL: read MFT via runs"); return -1; }
        };

        eprintln!("[mft] read {} bytes of MFT data", mft_data.len());

        if mft_data.len() < record_size {
            eprintln!("[mft] FAIL: mft_data shorter than one record ({} < {})", mft_data.len(), record_size);
            return -1;
        }

        // 计算盘符前缀（用于构建完整路径）
        let volume_prefix = format!("{}:/", volume);

        // 内存解析 MFT 记录，按前缀过滤并补上盘符
        let result = parse_mft_records(
            &mut mft_data,
            record_size,
            &volume_prefix,
            name_pattern,
            min_size,
            max_size,
            max_results,
            callback,
        );
        result
    }

    /// 从非驻留 $DATA 属性中提取 data_size
    fn get_data_size(record: &[u8], record_size: usize) -> Option<u64> {
        let first_attr_offset = LittleEndian::read_u16(&record[20..22]) as usize;
        let mut attr_offset = first_attr_offset;
        while attr_offset + 8 <= record_size {
            let attr_type = LittleEndian::read_u32(&record[attr_offset..attr_offset + 4]);
            if attr_type == 0xFFFFFFFF { break; }
            let attr_len = LittleEndian::read_u32(&record[attr_offset + 4..attr_offset + 8]) as usize;
            if attr_len == 0 || attr_len > record_size - attr_offset { break; }

            if attr_type == 0x80 && record[attr_offset + 8] != 0 {
                let data_size = LittleEndian::read_u64(&record[attr_offset + 0x30..attr_offset + 0x38]);
                return Some(data_size);
            }
            attr_offset += attr_len;
        }
        None
    }

    /// 解析 $DATA 非驻留属性的 data runs
    fn parse_mft_data_runs(record: &[u8], record_size: usize) -> Option<Vec<(u64, u64)>> {
        let first_attr_offset = LittleEndian::read_u16(&record[20..22]) as usize;
        let mut attr_offset = first_attr_offset;
        while attr_offset + 8 <= record_size {
            let attr_type = LittleEndian::read_u32(&record[attr_offset..attr_offset + 4]);
            if attr_type == 0xFFFFFFFF { break; }
            let attr_len = LittleEndian::read_u32(&record[attr_offset + 4..attr_offset + 8]) as usize;
            if attr_len == 0 || attr_len > record_size - attr_offset { break; }
            let non_resident = record[attr_offset + 8];

            if attr_type == 0x80 && non_resident != 0 {
                let run_array_rel = LittleEndian::read_u16(&record[attr_offset + 0x20..attr_offset + 0x22]) as usize;
                let run_array_start = attr_offset + run_array_rel;
                let data_end = attr_offset + attr_len;
                return Some(parse_data_runs_from(&record[run_array_start..data_end]));
            }
            attr_offset += attr_len;
        }
        None
    }

    /// 解析 data runs 字节数组
    fn parse_data_runs_from(data: &[u8]) -> Vec<(u64, u64)> {
        let mut runs = Vec::new();
        let mut pos = 0;
        let mut current_lcn: i64 = 0;
        while pos < data.len() {
            let header = data[pos];
            if header == 0 { break; }
            let count_len = (header >> 4) as usize;
            let offset_len = (header & 0x0F) as usize;
            pos += 1;
            if pos + count_len + offset_len > data.len() { break; }

            let mut count: u64 = 0;
            for i in 0..count_len {
                count |= (data[pos + i] as u64) << (i * 8);
            }

            let mut offset: i64 = 0;
            if offset_len > 0 {
                for i in 0..offset_len {
                    offset |= (data[pos + count_len + i] as i64) << (i * 8);
                }
                let sign_extend = 64 - offset_len * 8;
                offset = (offset << sign_extend) >> sign_extend;
                current_lcn += offset;
                runs.push((current_lcn as u64, count));
            }
            pos += count_len + offset_len;
        }
        runs
    }

    /// 通过 data runs 从卷设备读取 MFT 数据
    fn read_mft_via_runs(
        device: &mut File,
        runs: &[(u64, u64)],
        cluster_size: u64,
        total_size: u64,
    ) -> Option<Vec<u8>> {
        let mut data = Vec::with_capacity(total_size as usize);

        for &(lcn, count) in runs {
            if CANCELLED.load(Ordering::SeqCst) {
                return None;
            }
            let read_len = (count * cluster_size).min(total_size - data.len() as u64) as usize;
            if read_len == 0 { break; }

            let offset = lcn * cluster_size;
            if device.seek(SeekFrom::Start(offset)).is_err() { break; }

            let mut chunk = vec![0u8; read_len];
            if device.read_exact(&mut chunk).is_err() { break; }
            data.extend_from_slice(&chunk);

            if data.len() >= total_size as usize { break; }
        }
        if data.is_empty() { None } else { Some(data) }
    }

    /// 从内存中的 MFT 数据解析文件记录，过滤并回调
    /// Phase 1: rayon 并行解析所有记录
    /// Phase 2: BFS 拓扑排序计算完整路径
    fn parse_mft_records<F>(
        mft_data: &mut [u8],
        record_size: usize,
        root_volume: &str,
        name_pattern: Option<&str>,
        min_size: i64,
        max_size: i64,
        max_results: i32,
        callback: &mut F,
    ) -> i32
    where
        F: FnMut(*const c_char, *const c_char, c_longlong, c_longlong, u64, u64, u64, c_int, c_int, c_int, u32),
    {
        let records_in_buffer = mft_data.len() / record_size;
        let num_chunks = rayon::current_num_threads().max(1);
        let chunk_size = ((records_in_buffer + num_chunks - 1) / num_chunks).max(1);

        // Phase 1: 并行解析 MFT 记录（用 par_chunks_mut 安全切分 &mut）
        let chunk_results: Vec<HashMap<u64, MftEntry>> = mft_data
            .par_chunks_mut(record_size * chunk_size)
            .enumerate()
            .map(|(chunk_idx, chunk_data)| {
                let base_frn = chunk_idx * chunk_size;
                let records_in_chunk = chunk_data.len() / record_size;
                let mut local_entries = HashMap::new();

                for frn in 0..records_in_chunk {
                    let start = frn * record_size;
                    let abs_frn = base_frn + frn;
                    let record_buf = &mut chunk_data[start..start + record_size];

                    if &record_buf[0..4] != b"FILE" { continue; }
                    let flags = LittleEndian::read_u16(&record_buf[22..24]);
                    if flags & 0x0001 == 0 { continue; }
                    let base_frn = LittleEndian::read_u64(&record_buf[32..40]) & 0x0000_FFFF_FFFF_FFFF;
                    if base_frn != 0 { continue; }

                    fixup_record(record_buf);
                    let is_dir = (flags & 0x0002) != 0;
                    let first_attr_offset = LittleEndian::read_u16(&record_buf[20..22]) as usize;

                    let mut attr_offset = first_attr_offset;
                    let mut best_name: Option<(u64, String, i64, u64, u32)> = None;
                    let mut allocated_size: i64 = 0;

                    while attr_offset + 8 <= record_size {
                        let attr_type = LittleEndian::read_u32(&record_buf[attr_offset..attr_offset + 4]);
                        if attr_type == 0xFFFFFFFF { break; }
                        let attr_len = LittleEndian::read_u32(&record_buf[attr_offset + 4..attr_offset + 8]) as usize;
                        if attr_len == 0 || attr_len > record_size - attr_offset { break; }

                        if attr_type == 0x80 && record_buf[attr_offset + 8] != 0 {
                            allocated_size = LittleEndian::read_u64(&record_buf[attr_offset + 0x28..attr_offset + 0x30]) as i64;
                        }

                        if attr_type == 0x30 {
                            if record_buf[attr_offset + 8] != 0 {
                                attr_offset += attr_len;
                                continue;
                            }
                            let value_offset = LittleEndian::read_u16(&record_buf[attr_offset + 0x14..attr_offset + 0x16]) as usize;
                            let value_start = attr_offset + value_offset;
                            let parent_frn = LittleEndian::read_u64(&record_buf[value_start..value_start + 8]) & 0x0000_FFFF_FFFF_FFFF;
                            let mtime = LittleEndian::read_u64(&record_buf[value_start + 0x10..value_start + 0x18]);
                            let ds = LittleEndian::read_u64(&record_buf[value_start + 0x30..value_start + 0x38]) as i64;
                            let file_attrs = LittleEndian::read_u32(&record_buf[value_start + 0x38..value_start + 0x3C]);
                            let name_length_40 = record_buf[value_start + 0x40] as u8;
                            let ns_41 = record_buf[value_start + 0x41] as u8;
                            let name_length = name_length_40 as usize;
                            let namespace = ns_41;

                            let name_start = value_start + 0x42;
                            let name_end = name_start + name_length * 2;
                            if name_end > record_buf.len() || name_length == 0 {
                                attr_offset += attr_len;
                                continue;
                            }
                            let name = utf16le_to_string(&record_buf[name_start..name_end]);

                            let should_replace = match &best_name {
                                Some(_) => namespace == 1,
                                None => true,
                            };
                            if should_replace {
                                best_name = Some((parent_frn, name, ds, mtime, file_attrs));
                            }
                        }
                        attr_offset += attr_len;
                    }

                    if let Some((parent_frn, name, ds, nt_time, attrs)) = best_name {
                        local_entries.insert(
                            abs_frn as u64,
                            MftEntry {
                                name, parent_frn, is_dir,
                                data_size: ds, allocated_size,
                                nt_time, attrs,
                            },
                        );
                    }
                }
                local_entries
            })
            .collect();

        // 合并所有 chunk
        let mut entries: HashMap<u64, MftEntry> = HashMap::with_capacity(records_in_buffer);
        for chunk_map in chunk_results {
            entries.extend(chunk_map);
        }

        if entries.is_empty() {
            eprintln!("[mft] empty_entries records_in_buffer={}", records_in_buffer);
            return 0;
        }

        eprintln!("[mft] parsed {} entries ({} threads)", entries.len(), num_chunks);

        // Phase 2: BFS 拓扑路径计算
        let mut children: HashMap<u64, Vec<u64>> = HashMap::new();
        for (&frn, entry) in &entries {
            if frn >= 5 {
                children.entry(entry.parent_frn).or_default().push(frn);
            }
        }

        let mut path_cache: HashMap<u64, String> = HashMap::new();
        path_cache.insert(5, String::new());

        let mut queue: Vec<u64> = Vec::new();
        let mut visited: std::collections::HashSet<u64> = std::collections::HashSet::new();
        visited.insert(5);
        if let Some(roots) = children.get(&5) {
            for &r in roots {
                if visited.insert(r) { queue.push(r); }
            }
        }

        let mut front = 0usize;
        while front < queue.len() {
            let frn = queue[front];
            front += 1;
            let entry = match entries.get(&frn) { Some(e) => e, None => continue };
            let parent_path = path_cache.get(&entry.parent_frn).cloned().unwrap_or_default();
            let full_path = if parent_path.is_empty() { entry.name.clone() }
                           else { format!("{}\\{}", parent_path, entry.name) };
            path_cache.insert(frn, full_path);
            if let Some(kids) = children.get(&frn) {
                for &k in kids {
                    if visited.insert(k) { queue.push(k); }
                }
            }
        }

        eprintln!("[mft] BFS computed {} paths", path_cache.len());

        // Phase 3: 收集结果
        let mut results: Vec<(String, String, i64, i64, u64, u32, i32, u64)> = Vec::new();

        for (&frn, entry) in &entries {
            if frn < 5 { continue; }
            if CANCELLED.load(Ordering::SeqCst) { break; }
            if !entry.is_dir {
                if min_size >= 0 && entry.data_size < min_size { continue; }
                if max_size >= 0 && entry.data_size > max_size { continue; }
            }
            let full_path = match path_cache.get(&frn) { Some(p) => p.clone(), None => continue };
            if full_path.is_empty() { continue; }

            if let Some(pat) = name_pattern {
                if entry.is_dir { continue; }
                let file_name = Path::new(&full_path).file_name().and_then(|n| n.to_str()).unwrap_or(&full_path);
                if !super::glob_match(file_name, pat) { continue; }
            }

            let ext = if entry.is_dir { String::new() }
                      else { Path::new(&full_path).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase() };
            let unix_ms = nt_to_unix_ms(entry.nt_time);
            let forward_path = full_path.replace('\\', "/");
            let display_path = format!("{}{}", root_volume, forward_path);
            let entry_alloc = if entry.is_dir { 0 } else { entry.allocated_size };

            results.push((display_path, ext,
                if entry.is_dir { 0 } else { entry.data_size },
                entry_alloc, unix_ms, entry.attrs,
                if entry.is_dir { 1 } else { 0 },
                entry.parent_frn));
        }

        let mut count = 0i32;
        for (path_str, ext, size, alloc, modified, attrs, is_dir, parent) in results {
            if max_results > 0 && count >= max_results { break; }
            let path_cstr = match CString::new(path_str.as_str()) { Ok(c) => c, Err(_) => continue };
            let ext_cstr = match CString::new(ext.as_str()) { Ok(c) => c, Err(_) => continue };
            callback(path_cstr.as_ptr(), ext_cstr.as_ptr(), size, alloc, modified, 0, parent,
                path_str.len() as c_int, is_dir, ext.len() as c_int, attrs);
            count += 1;
        }
        count
    }

    /// 从 reader 读取精确字节数
    fn read_exact_bytes(reader: &mut impl Read, n: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; n];
        reader.read_exact(&mut buf).ok()?;
        Some(buf)
    }

    /// 对子目录路径使用 jwalk（多线程）回退
    fn search_files_walkdir<F>(
        root: &str,
        name_pattern: Option<&str>,
        min_size: i64,
        max_size: i64,
        max_results: i32,
        mut callback: F,
    ) -> i32
    where
        F: FnMut(*const c_char, *const c_char, c_longlong, c_longlong, u64, u64, u64, c_int, c_int, c_int, u32),
    {
        let mut count = 0i32;
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if CANCELLED.load(Ordering::SeqCst) {
                break;
            }
            if max_results > 0 && count >= max_results {
                break;
            }

            let path = entry.path();
            if path.components().count() <= 1 {
                continue;
            }

            if entry.file_type().is_dir() { continue; }

            if let Some(pat) = name_pattern {
                let name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n,
                    None => continue,
                };
                if !super::glob_match(name, pat) { continue; }
            }

            let size = entry.metadata().map(|m| m.len() as c_longlong).unwrap_or(0);

            if min_size >= 0 && size < min_size { continue; }
            if max_size >= 0 && size > max_size { continue; }

            let path_str = path.to_string_lossy().to_string().replace('\\', "/");
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            let last_modified = super::get_last_modified(&path);
            let attrs = super::get_attributes(&path);

            let path_cstr = match CString::new(path_str.as_str()) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let ext_cstr = match CString::new(ext.as_str()) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let alloc = size;
            callback(
                path_cstr.as_ptr(),
                ext_cstr.as_ptr(),
                size,
                alloc,
                last_modified,
                0,
                0,
                path_str.len() as c_int,
                0,
                ext.len() as c_int,
                attrs as u32,
            );
            count += 1;
        }
        count
    }

    fn fixup_record(buf: &mut [u8]) {
        let usn_offset = LittleEndian::read_u16(&buf[4..6]) as usize;
        let usn_count = LittleEndian::read_u16(&buf[6..8]);
        let array_count = (usn_count.saturating_sub(1)) as usize;

        let array_start = usn_offset + 2;
        let mut sector_pos = NTFS_BLOCK_SIZE - 2;

        for i in 0..array_count {
            let entry_start = array_start + i * 2;
            if entry_start + 2 > buf.len() || sector_pos + 2 > buf.len() {
                break;
            }
            buf[sector_pos] = buf[entry_start];
            buf[sector_pos + 1] = buf[entry_start + 1];
            sector_pos += NTFS_BLOCK_SIZE;
        }
    }

    fn utf16le_to_string(bytes: &[u8]) -> String {
        let u16_vec: Vec<u16> = bytes
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u16_vec)
    }

    fn nt_to_unix_ms(nt_time: u64) -> u64 {
        const EPOCH_DIFF: u64 = 116444736000000000;
        if nt_time >= EPOCH_DIFF {
            (nt_time - EPOCH_DIFF) / 10000
        } else {
            0
        }
    }
}

// ==================== Non-Windows: walkdir ====================

#[cfg(not(windows))]
mod platform {
    use super::*;
    use jwalk::WalkDir;

    pub fn search_files<F>(
        root: &str,
        name_pattern: Option<&str>,
        min_size: i64,
        max_size: i64,
        max_results: i32,
        mut callback: F,
    ) -> i32
    where
        F: FnMut(*const c_char, *const c_char, c_longlong, c_longlong, u64, u64, u64, c_int, c_int, c_int, u32),
    {
        let mut count = 0i32;

        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if CANCELLED.load(Ordering::SeqCst) {
                break;
            }
            if max_results > 0 && count >= max_results {
                break;
            }

            let path = entry.path().to_path_buf();
            if path.components().count() <= 1 {
                continue;
            }

            if entry.file_type().is_dir() { continue; }

            if let Some(pat) = name_pattern {
                let name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n,
                    None => continue,
                };
                if !glob_match(name, pat) {
                    continue;
                }
            }

            let size = entry.metadata().map(|m| m.len() as c_longlong).unwrap_or(0);
            let alloc = size;

            if min_size >= 0 && size < min_size { continue; }
            if max_size >= 0 && size > max_size { continue; }

            let path_str = path.to_string_lossy().to_string();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            let last_modified = get_last_modified(&path);
            let attrs = get_attributes(&path);

            let path_cstr = match CString::new(path_str.as_str()) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let ext_cstr = match CString::new(ext.as_str()) {
                Ok(c) => c,
                Err(_) => continue,
            };

            callback(
                path_cstr.as_ptr(),
                ext_cstr.as_ptr(),
                size,
                alloc,
                last_modified,
                0,
                0,
                path_str.len() as c_int,
                0,
                ext.len() as c_int,
                attrs as u32,
            );
            count += 1;
        }

        count
    }
}

// ==================== C ABI 导出函数 ====================

#[no_mangle]
pub unsafe extern "C" fn fast_scan(
    root_dir: *const c_char,
    max_results: c_int,
    callback: Option<FileResultCallback>,
) -> c_int {
    if root_dir.is_null() || callback.is_none() {
        return -1;
    }
    let root = match std::ffi::CStr::from_ptr(root_dir).to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };
    let cb = callback.unwrap();
    CANCELLED.store(false, Ordering::SeqCst);

    platform::search_files(
        root,
        None,
        -1,
        -1,
        max_results,
        |path, ext, size, alloc, mtime, usn, parent, plen, is_dir, elen, attr| {
            cb(path, ext, size, alloc, mtime, usn, parent, plen, is_dir, elen, attr);
        },
    )
}

#[no_mangle]
pub unsafe extern "C" fn fast_search_by_name(
    root_dir: *const c_char,
    pattern: *const c_char,
    max_results: c_int,
    callback: Option<FileResultCallback>,
) -> c_int {
    if root_dir.is_null() || pattern.is_null() || callback.is_none() {
        return -1;
    }
    let root = match std::ffi::CStr::from_ptr(root_dir).to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };
    let pat = match std::ffi::CStr::from_ptr(pattern).to_str() {
        Ok(s) => s,
        Err(_) => return -3,
    };
    let cb = callback.unwrap();
    CANCELLED.store(false, Ordering::SeqCst);

    platform::search_files(
        root,
        Some(pat),
        -1,
        -1,
        max_results,
        |path, ext, size, alloc, mtime, usn, parent, plen, is_dir, elen, attr| {
            cb(path, ext, size, alloc, mtime, usn, parent, plen, is_dir, elen, attr);
        },
    )
}

#[no_mangle]
pub unsafe extern "C" fn fast_search_by_size(
    root_dir: *const c_char,
    min_size: c_longlong,
    max_size: c_longlong,
    max_results: c_int,
    callback: Option<FileResultCallback>,
) -> c_int {
    if root_dir.is_null() || callback.is_none() {
        return -1;
    }
    let root = match std::ffi::CStr::from_ptr(root_dir).to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };
    let cb = callback.unwrap();
    CANCELLED.store(false, Ordering::SeqCst);

    platform::search_files(
        root,
        None,
        min_size,
        max_size,
        max_results,
        |path, ext, size, alloc, mtime, usn, parent, plen, is_dir, elen, attr| {
            cb(path, ext, size, alloc, mtime, usn, parent, plen, is_dir, elen, attr);
        },
    )
}

#[no_mangle]
pub unsafe extern "C" fn fast_get_tree(
    root_dir: *const c_char,
    max_depth: c_int,
    max_results: c_int,
    callback: Option<FileResultCallback>,
) -> c_int {
    if root_dir.is_null() || callback.is_none() {
        return -1;
    }
    let root = match std::ffi::CStr::from_ptr(root_dir).to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };
    let cb = callback.unwrap();
    let _max_depth = max_depth;
    CANCELLED.store(false, Ordering::SeqCst);

    platform::search_files(
        root,
        None,
        -1,
        -1,
        max_results,
        |path, ext, size, alloc, mtime, usn, parent, plen, is_dir, elen, attr| {
            cb(path, ext, size, alloc, mtime, usn, parent, plen, is_dir, elen, attr);
        },
    )
}

#[no_mangle]
pub unsafe extern "C" fn fast_search_by_path(
    root_dir: *const c_char,
    path_pattern: *const c_char,
    max_results: c_int,
    callback: Option<FileResultCallback>,
) -> c_int {
    if root_dir.is_null() || path_pattern.is_null() || callback.is_none() {
        return -1;
    }
    let root = match std::ffi::CStr::from_ptr(root_dir).to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };
    let _pat = match std::ffi::CStr::from_ptr(path_pattern).to_str() {
        Ok(s) => s,
        Err(_) => return -3,
    };
    let cb = callback.unwrap();
    CANCELLED.store(false, Ordering::SeqCst);

    platform::search_files(
        root,
        None,
        -1,
        -1,
        max_results,
        |path, ext, size, alloc, mtime, usn, parent, plen, is_dir, elen, attr| {
            cb(path, ext, size, alloc, mtime, usn, parent, plen, is_dir, elen, attr);
        },
    )
}

#[no_mangle]
pub unsafe extern "C" fn fast_get_version() -> *const c_char {
    b"1.0.0\0".as_ptr() as *const c_char
}

#[no_mangle]
pub unsafe extern "C" fn fast_search_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

// ==================== 内部辅助函数 ====================

fn glob_match(name: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let name_chars: Vec<char> = name.chars().collect();
    let pat_chars: Vec<char> = pattern.chars().collect();
    glob_match_impl(&name_chars, &pat_chars, 0, 0)
}

fn glob_match_impl(name: &[char], pat: &[char], ni: usize, pi: usize) -> bool {
    if pi >= pat.len() {
        return ni >= name.len();
    }
    if ni < name.len() {
        if pat[pi] == '*' {
            if glob_match_impl(name, pat, ni, pi + 1) {
                return true;
            }
            if glob_match_impl(name, pat, ni + 1, pi) {
                return true;
            }
            return false;
        } else if pat[pi] == '?' || pat[pi] == name[ni] {
            return glob_match_impl(name, pat, ni + 1, pi + 1);
        }
    }
    false
}

fn get_attributes(path: &Path) -> u32 {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if let Ok(metadata) = path.metadata() {
            return metadata.file_attributes();
        }
        0
    }
    #[cfg(not(windows))]
    {
        if let Ok(metadata) = path.metadata() {
            return metadata.permissions().mode() as u32;
        }
        0
    }
}

fn get_last_modified(path: &Path) -> u64 {
    match path.metadata() {
        Ok(m) => match m.modified() {
            Ok(t) => t
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            Err(_) => 0,
        },
        Err(_) => 0,
    }
}
