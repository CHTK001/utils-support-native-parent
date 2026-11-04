#!/usr/bin/env python3
"""Dump raw NTFS MFT record attributes for a given record number.

Usage: dump_mft.py <record number>   (e.g. 0, 16, 1000)
      (the FILE_NAME value data is emitted as hex at the two known offsets)
"""
import sys


def u8(b, o):
    return b[o]


def u16(b, o):
    return int.from_bytes(b[o:o + 2], "little")


def u32(b, o):
    return int.from_bytes(b[o:o + 4], "little")


def u64(b, o):
    return int.from_bytes(b[o:o + 8], "little")


def decode_name(b, v, nlen):
    raw = b[v + 2:v + 2 + nlen * 2]
    try:
        return raw.decode("utf-16le").rstrip("\x00")
    except Exception:
        return repr(raw)


def walk_instance(b, v):
    """One FILE_NAME instance: NameLen at v+0x40, NameType at v+0x41, name at v+0x42."""
    nlen = u8(b, v + 0x40)
    ns = u8(b, v + 0x41)
    name = decode_name(b, v + 2, nlen) if nlen else None
    return nlen, ns, name


def main():
    rec_num = int(sys.argv[1])
    vol = "\\\\.\\C:"
    f = open(vol, "rb")
    boot = f.read(512)
    bps = u16(boot, 0x0B)
    spc = boot[0x0D]
    cluster = bps * spc
    mft_lcn = u64(boot, 0x30)
    rec_raw = boot[0x40]
    rec_size = rec_raw * cluster if rec_raw > 0 else 1 << (-rec_raw)
    print(f"sector={bps} cluster={cluster} MFT_lcn={mft_lcn} rec_size={rec_size}")

    off = mft_lcn * cluster + rec_num * rec_size
    f.seek(off)
    rec = f.read(rec_size)
    print(f"record {rec_num}: magic={rec[:4]}, flags0x16={hex(u16(rec, 0x16))}, "
          f"attrsOff={u16(rec, 0x14)}, u64_0x20={u64(rec, 0x20)}")

    p = u16(rec, 0x14)
    while p + 8 <= len(rec):
        atype = u32(rec, p)
        alen = u32(rec, p + 4)
        if atype == 0xFFFF_FFFF or alen < 16 or p + alen > len(rec):
            break
        non_res = rec[p + 8] != 0
        print(f"--- attr p={p} type=0x{atype:04X} len={alen} resident={non_res} "
              f"namelen={u8(rec, p + 9)} nameoff={u16(rec, p + 10)}")
        if atype == 0x30 and not non_res:
            v = p + 16
            nlen, ns, name = walk_instance(rec, v)
            print(f"   value at v=p+16: NameLen={nlen} NameType(ns)={ns} Name='{name}'")
            print(f"   value hex v+0x00..v+0x50: {rec[v:v + 0x50].hex()}")
            print(f"   parent=u64(v+0x00)={u64(rec, v)}")
        p += alen
    f.close()


main()
