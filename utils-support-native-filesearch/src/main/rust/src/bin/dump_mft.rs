//! 调试探针：转储单个 MFT 记录中的属性头与 FILE_NAME 属性的内容。
//!
//! 用法: mftdump.exe <记录号>

use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes([
        b[o], b[o + 1], b[o + 2], b[o + 3], b[o + 4], b[o + 5], b[o + 6], b[o + 7],
    ])
}

fn u8_at(b: &[u8], o: usize) -> u8 {
    b[o]
}

fn decode_name(b: &[u8], v: usize, nlen: usize) -> String {
    if nlen == 0 {
        return String::new();
    }
    let raw = &b[v + 2..v + 2 + nlen * 2];
    let units: Vec<u16> = (0..raw.len())
        .step_by(2)
        .map(|i| u16::from_le_bytes([raw[i], raw[i + 1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rec_num = args.get(1).map(|s| s.parse::<usize>().unwrap()).unwrap_or(0);

    let raw = std::fs::File::open("\\\\.\\C:").unwrap();
    let mut f = std::io::BufReader::new(raw);

    let mut boot = vec![0u8; 512];
    f.read_exact(&mut boot).unwrap();
    let bps = u16_at(&boot, 0x0B) as u64;
    let spc = boot[0x0D] as u64;
    let cluster = bps * spc;
    let mft_lcn = u64_at(&boot, 0x30);
    let rec_raw = boot[0x40] as i8;
    let rec_size = if rec_raw > 0 {
        rec_raw as u64 * cluster
    } else {
        1u64 << ((-rec_raw) as u32)
    };
    println!("sector_size={} cluster={} MFT_lcn={} record_size={}", bps, cluster, mft_lcn, rec_size);

    let offset = mft_lcn * cluster + rec_num as u64 * rec_size;
    f.rewind().unwrap();
    let mut rec = vec![0u8; rec_size as usize];
    f.seek(SeekFrom::Start(offset)).unwrap();
    f.read_exact(&mut rec).unwrap();

    println!("record {} magic={} flags0x16=0x{:04X} attrs0x14={:d} u64_0x20=" {}{}{}",
        rec_num,
        String::from_utf8_lossy(&rec[0..4]),
        u16_at(&rec, 0x16),
        u16_at(&rec, 0x14),
        u64_at(&rec, 0x20)
    );

    let mut p = u16_at(&rec, 0x14) as usize;
    while p + 8 <= rec.len() {
        let atype = u32_at(&rec, p);
        let alen = u32_at(&rec, p + 4);
        if atype == 0xffff_ffff || alen < 16 || p + alen as usize > rec.len() {
            break;
        }
        let non_res = rec[p + 8] != 0;
        println!("--- attr p={} type=0x{:04X} len={} resident={} name_len={} name_off={} ",
            p, atype, alen, non_res, u8_at(&rec, p + 9), u16_at(&rec, p + 10));
        if atype == 0x30 && !non_res {
            let v = p + 16;
            let nlen = u8_at(&rec, v + 0x40) as usize;
            let ns = u8_at(&rec, v + 0x41) as usize;
            println!("   FILE_NAME value at v=p+16: NameLen={} NameType(ns)={} Name='{ui}' parent={hi:?}",
                nlen, ns, ui = decode_name(&rec, v, nlen), hi = u64_at(&rec, v));
            println!("   value hex v+0x00..v+0x50: {}", rec[v..v + 0x50].iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
            println!("   code's divination: clen=u32(p+16)={} coff=u16(p+20)={:x}", u32_at(&rec, p + 16), u16_at(&rec, p + 20));
            println!("   sanity: p+coff+clen={} > record len {} ?", p + u16_at(&rec, p + 20) as usize + u32_at(&rec, p + 16) as usize, rec.len());
        }
        p += alen as usize;
    }
}
