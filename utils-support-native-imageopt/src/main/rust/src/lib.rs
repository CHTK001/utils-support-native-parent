//! 图片压缩原生门面 —— 把 libdeflate 与 libimagequant 收敛成扁平 C ABI，
//! 供 Java 侧通过 Panama FFM 直接下探调用。
//!
//! 两个上游库都由 crates.io 上的 `libdeflater` / `imagequant` crate 自带源码，
//! 经 `cc` 在构建机本地编译并静态链接进本 cdylib，
//! 因此目标机无需安装任何 C 库、也不需要 CMake。
//!
//! # 符号命名空间
//!
//! | 前缀 | 上游库 | 用途 |
//! |------|--------|------|
//! | `chua_df_*` | libdeflate | deflate / zlib 压缩 |
//! | `chua_iq_*` | libimagequant（pngquant 内核） | 调色板量化 |
//! | `chua_imageopt_*` | 本门面 | 版本与错误描述 |
//!
//! # 约定
//!
//! - 所有函数成功返回 0，失败返回负错误码；原因用 `chua_imageopt_last_error()` 取回（线程局部）。
//! - **不分配任何需要调用方释放的缓冲区**。压缩输出与量化输出都由调用方预分配、
//!   按 `*_cap` 上限传入。这样跨 FFM 边界不存在所有权歧义，也避免了
//!   「谁负责 free」在 Java 侧被漏掉导致的原生内存泄漏。
//! - 容量不足返回 `ERR_TOO_SMALL`，`*_len` 里写入实际所需字节数，调用方可据此扩容重试。

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_uint, c_uchar};
use std::panic::AssertUnwindSafe;
use std::ptr;
use std::slice;

use imagequant::{Attributes, RGBA};
use libdeflater::{CompressionLvl, Compressor};

/// libdeflate 支持的最高压缩级别。
const MAX_LEVEL: c_int = 12;

/// libimagequant 调色板最大颜色数。
const MAX_PALETTE: usize = 256;

/// 单个调色板颜色占用的字节数。
const PALETTE_ENTRY_BYTES: usize = 4;

/// 错误码：入参非法。
const ERR_ARG: c_int = -1;
/// 错误码：调用方提供的输出缓冲区太小。
const ERR_TOO_SMALL: c_int = -2;
/// 错误码：压缩失败。
const ERR_COMPRESS: c_int = -3;
/// 错误码：量化失败。
const ERR_QUANTIZE: c_int = -4;
/// 错误码：内部 panic（已被捕获）。
const ERR_PANIC: c_int = -5;

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

/// 记录一次失败原因，供 `chua_imageopt_last_error()` 取回。
fn set_last_error(context: &str, detail: &str) {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = CString::new(format!("{context}: {detail}")).unwrap_or_default();
    });
}

/// 本门面版本串（静态存储，调用方不需释放）。
#[no_mangle]
pub extern "C" fn chua_imageopt_version() -> *const c_char {
    c"chua_native_imageopt/0.1.0 (libdeflate 1.26.1, libimagequant 4.4.1)"
        .as_ptr()
        .cast::<c_char>()
}

/// 内嵌的 libdeflate 版本号（静态存储，调用方不需释放）。
#[no_mangle]
pub extern "C" fn chua_imageopt_deflate_version() -> *const c_char {
    c"1.26.1".as_ptr().cast::<c_char>()
}

/// 内嵌的 libimagequant 版本号（静态存储，调用方不需释放）。
#[no_mangle]
pub extern "C" fn chua_imageopt_quantize_version() -> *const c_char {
    c"4.4.1".as_ptr().cast::<c_char>()
}

/// 最近一次失败的描述（线程局部，指向内部存储，调用方不需释放）。
#[no_mangle]
pub extern "C" fn chua_imageopt_last_error() -> *const c_char {
    LAST_ERROR.with(|cell| cell.borrow().as_ptr())
}

/// 校验压缩级别是否落在 libdeflate 支持的区间内。
fn valid_level(level: c_int) -> bool {
    (0..=MAX_LEVEL).contains(&level)
}

/// 在目标级别附近展开候选档位。
///
/// 顺序刻意是「目标档优先、再向下、最后向上」：无论 `budget` 多小，
/// 都至少会尝试调用方点名的那一档。
///
/// 已经在最高档时**不向下展开**——libdeflate 的压缩率对级别单调，
/// 更低的档必然更大，试了也是白试，只会把 CPU 烧在注定更差的结果上。
fn candidate_levels(level: c_int, budget: usize) -> Vec<c_int> {
    let mut candidates = vec![level];
    if level >= MAX_LEVEL {
        return candidates;
    }
    for candidate in [level - 1, level + 1] {
        if valid_level(candidate) && !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates.truncate(budget.max(1));
    candidates
}

/// libdeflate 单次 zlib 压缩，返回压缩后字节数。
///
/// `level` 取值 0~12（0 最快、12 最小），`dst` 由调用方预分配。
///
/// # Safety
/// `src` 必须指向至少 `src_len` 个可读字节，`dst` 必须指向至少 `dst_cap` 个可写字节。
#[no_mangle]
pub unsafe extern "C" fn chua_df_zlib_compress(
    src: *const c_uchar,
    src_len: usize,
    level: c_int,
    dst: *mut c_uchar,
    dst_cap: usize,
    dst_len: *mut usize,
) -> c_int {
    if src.is_null() || dst.is_null() || dst_len.is_null() {
        set_last_error("chua_df_zlib_compress", "null argument");
        return ERR_ARG;
    }
    if !valid_level(level) {
        set_last_error("chua_df_zlib_compress", "level must be within 0..=12");
        return ERR_ARG;
    }
    *dst_len = 0;

    let mut compressor = match CompressionLvl::new(level) {
        Ok(lvl) => Compressor::new(lvl),
        Err(_) => {
            set_last_error("chua_df_zlib_compress", "invalid compression level");
            return ERR_ARG;
        }
    };
    let bound = compressor.zlib_compress_bound(src_len);
    if dst_cap < bound {
        *dst_len = bound;
        set_last_error("chua_df_zlib_compress", "output buffer too small");
        return ERR_TOO_SMALL;
    }

    let input = slice::from_raw_parts(src, src_len);
    let output = slice::from_raw_parts_mut(dst, dst_cap);
    match compressor.zlib_compress(input, output) {
        Ok(written) => {
            *dst_len = written;
            0
        }
        Err(_) => {
            set_last_error("chua_df_zlib_compress", "libdeflate_zlib_compress failed");
            ERR_COMPRESS
        }
    }
}

/// libdeflate 多档试压取最小值，返回压缩后字节数。
///
/// 这是 Java 侧 `DeflateEngine.DeflateProfile#iterations` 的原生实现：
/// 在目标级别附近取若干档逐一压缩，返回最短结果——zopfli 的思路（穷举参数组合取最小），
/// 但用 libdeflate 的高速内核做试压，把耗时压到可接受范围。
///
/// 每个候选都先压进独立的暂存缓冲再比较，避免后一次试压把先前的最优结果覆盖掉。
/// `attempts` 为 1 时退化为单次压缩，实际试压档数被钳制在 1..=3；
/// 目标级别已是最高档时实际只试 1 档（见 `candidate_levels`）。
///
/// # Safety
/// `src` 必须指向至少 `src_len` 个可读字节，`dst` 必须指向至少 `dst_cap` 个可写字节。
#[no_mangle]
pub unsafe extern "C" fn chua_df_zlib_compress_best(
    src: *const c_uchar,
    src_len: usize,
    level: c_int,
    attempts: c_int,
    dst: *mut c_uchar,
    dst_cap: usize,
    dst_len: *mut usize,
) -> c_int {
    if src.is_null() || dst.is_null() || dst_len.is_null() {
        set_last_error("chua_df_zlib_compress_best", "null argument");
        return ERR_ARG;
    }
    if !valid_level(level) {
        set_last_error("chua_df_zlib_compress_best", "level must be within 0..=12");
        return ERR_ARG;
    }
    *dst_len = 0;

    let input = slice::from_raw_parts(src, src_len);
    let levels = candidate_levels(level, attempts.clamp(1, 3) as usize);

    // 上界只取决于输入长度、与级别无关，所以用第一个候选档的压缩器算一次即可。
    let bound = match CompressionLvl::new(level).ok().map(Compressor::new) {
        Some(mut compressor) => compressor.zlib_compress_bound(src_len),
        None => {
            set_last_error("chua_df_zlib_compress_best", "invalid compression level");
            return ERR_ARG;
        }
    };
    if dst_cap < bound {
        *dst_len = bound;
        set_last_error("chua_df_zlib_compress_best", "output buffer too small");
        return ERR_TOO_SMALL;
    }

    let mut best: Option<Vec<u8>> = None;
    for candidate in levels {
        let mut compressor = match CompressionLvl::new(candidate) {
            Ok(lvl) => Compressor::new(lvl),
            Err(_) => continue,
        };
        let mut scratch = vec![0u8; bound];
        let output = scratch.as_mut_slice();
        if let Ok(written) = compressor.zlib_compress(input, output) {
            scratch.truncate(written);
            match &best {
                Some(current) if current.len() <= written => {}
                _ => best = Some(scratch),
            }
        }
    }

    match best {
        Some(result) => {
            ptr::copy_nonoverlapping(result.as_ptr(), dst, result.len());
            *dst_len = result.len();
            0
        }
        None => {
            set_last_error("chua_df_zlib_compress_best", "all candidate levels failed");
            ERR_COMPRESS
        }
    }
}

/// libimagequant 调色板量化。
///
/// - `rgba`：输入像素，每像素 4 字节 RGBA 顺序，共 `pixel_count` 个像素
/// - `out_palette`：输出调色板，每色 4 字节 RGBA 顺序，调用方至少预留 `256 * 4` 字节
/// - `out_indices`：输出索引，每像素 1 字节，共 `pixel_count` 个
/// - `max_colors`：目标色数，2~256
/// - `dither`：抖动强度 0（关闭）~100（最强）
/// - `speed`：libimagequant 速度档 1（最慢/最好）~10（最快）
///
/// 成功时 `*out_color_count` 写入实际调色板颜色数。
///
/// # Safety
/// 所有指针都必须指向各自足够大的可写/可读区域，且 `pixel_count == width * height`。
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn chua_iq_quantize(
    rgba: *const c_uchar,
    pixel_count: usize,
    width: c_uint,
    height: c_uint,
    max_colors: c_int,
    dither: c_int,
    speed: c_int,
    out_palette: *mut c_uchar,
    out_palette_cap: usize,
    out_indices: *mut c_uchar,
    out_color_count: *mut c_int,
) -> c_int {
    if rgba.is_null() || out_palette.is_null() || out_indices.is_null() || out_color_count.is_null() {
        set_last_error("chua_iq_quantize", "null argument");
        return ERR_ARG;
    }
    if width == 0 || height == 0 {
        set_last_error("chua_iq_quantize", "width and height must be positive");
        return ERR_ARG;
    }
    let expected = width as usize * height as usize;
    if pixel_count != expected {
        set_last_error("chua_iq_quantize", "pixel_count must equal width * height");
        return ERR_ARG;
    }
    if !(2..=256).contains(&max_colors) {
        set_last_error("chua_iq_quantize", "max_colors must be within 2..=256");
        return ERR_ARG;
    }
    if !(0..=100).contains(&dither) {
        set_last_error("chua_iq_quantize", "dither must be within 0..=100");
        return ERR_ARG;
    }
    if !(1..=10).contains(&speed) {
        set_last_error("chua_iq_quantize", "speed must be within 1..=10");
        return ERR_ARG;
    }
    let palette_bytes = MAX_PALETTE * PALETTE_ENTRY_BYTES;
    if out_palette_cap < palette_bytes {
        set_last_error("chua_iq_quantize", "palette buffer too small");
        return ERR_TOO_SMALL;
    }

    *out_color_count = 0;
    let bytes = slice::from_raw_parts(rgba, pixel_count * PALETTE_ENTRY_BYTES);
    let pixels: Vec<RGBA> = bytes
        .chunks_exact(PALETTE_ENTRY_BYTES)
        .map(|c| RGBA::new(c[0], c[1], c[2], c[3]))
        .collect();

    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let mut attr = Attributes::new();
        attr.set_max_colors(max_colors as u32)?;
        attr.set_speed(speed)?;
        let mut image = attr.new_image_borrowed(&pixels, width as usize, height as usize, 0.0)?;
        let mut quantized = attr.quantize(&mut image)?;
        quantized.set_dithering_level(dither as f32 / 100.0)?;
        quantized.remapped(&mut image)
    }));

    let (palette, indices) = match result {
        Ok(Ok(pair)) => pair,
        Ok(Err(e)) => {
            set_last_error("chua_iq_quantize", &format!("{e:?}"));
            return ERR_QUANTIZE;
        }
        Err(_) => {
            set_last_error("chua_iq_quantize", "panic inside libimagequant");
            return ERR_PANIC;
        }
    };

    if indices.len() != expected {
        set_last_error("chua_iq_quantize", "remap produced unexpected index count");
        return ERR_QUANTIZE;
    }

    let palette_out = slice::from_raw_parts_mut(out_palette, palette_bytes);
    for (i, color) in palette.iter().take(MAX_PALETTE).enumerate() {
        palette_out[i * PALETTE_ENTRY_BYTES] = color.r;
        palette_out[i * PALETTE_ENTRY_BYTES + 1] = color.g;
        palette_out[i * PALETTE_ENTRY_BYTES + 2] = color.b;
        palette_out[i * PALETTE_ENTRY_BYTES + 3] = color.a;
    }
    ptr::copy_nonoverlapping(indices.as_ptr(), out_indices, expected);
    *out_color_count = palette.len().min(MAX_PALETTE) as c_int;
    0
}
