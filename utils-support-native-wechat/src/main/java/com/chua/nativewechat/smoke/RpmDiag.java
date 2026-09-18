package com.chua.nativewechat.smoke;

import java.lang.foreign.*;
import java.lang.invoke.MethodHandle;
import java.util.*;

/**
 * ReadProcessMemory 最小诊断。
 *
 * @author CH
 * @since 4.0.0.42
 */
public class RpmDiag {

    /**
     * 程序入口，运行示例自检。
     *
     * @param args 参数，不允许为 null
     * @throws Throwable 当执行过程不满足前置条件时
     */
    public static void main(String[] args) throws Throwable {
        int targetPid = Integer.parseInt(args[0]);
        boolean selfTest = Boolean.parseBoolean(args.length > 1 ? args[1] : "false");
        int pid = selfTest ? (int) ProcessHandle.current().pid() : targetPid;

        Arena arena = Arena.ofShared();
        try {
            SymbolLookup k32 = SymbolLookup.libraryLookup("kernel32.dll", arena);

            MethodHandle openProcessMH = downcall(k32.find("OpenProcess").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_LONG,
                            ValueLayout.JAVA_INT, ValueLayout.JAVA_BOOLEAN, ValueLayout.JAVA_INT));
            MethodHandle readMemMH = downcall(k32.find("ReadProcessMemory").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_BOOLEAN,
                            ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG,
                            ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG));
            MethodHandle vqeMH = downcall(k32.find("VirtualQueryEx").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_LONG,
                            ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG));
            MethodHandle closeMH = downcall(k32.find("CloseHandle").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_BOOLEAN, ValueLayout.JAVA_LONG));
            MethodHandle gleMH = downcall(k32.find("GetLastError").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_INT));

            System.out.println("=== RpmDiag: PID=" + pid + " (self=" + selfTest + ") ===");

            long h = (long) openProcessMH.invoke(0x1FFFFF, false, pid);
            if (h == 0) {
                int gle = (int) gleMH.invoke();
                System.out.println("[1] OpenProcess 失败, GLE=" + gle);
                return;
            }
            System.out.println("[1] OpenProcess OK, handle=0x" + Long.toHexString(h));

            MemorySegment mbi = arena.allocate(64, 8);
            List<long[]> regions = enumRegions(h, mbi, vqeMH);
            System.out.println("[2] 找到 " + regions.size() + " 个候选可读区域");

            if (regions.isEmpty()) {
                regions = enumRegionsFrom(h, mbi, vqeMH, 0x100000000L);
                System.out.println("[2b] 扩大搜索后找到 " + regions.size() + " 个区域");
            }
            if (regions.isEmpty()) {
                System.out.println("无法找到任何已提交区域");
                closeMH.invoke(h);
                return;
            }

            MemorySegment buf = arena.allocate(65536, 8);
            MemorySegment brPtr = arena.allocate(8, 8);
            int okCount = 0;
            int failCount = 0;
            for (long[] r : regions) {
                Result res = tryRead(h, r, buf, brPtr, readMemMH, gleMH);
                if (res.ok) {
                    okCount++;
                    printResult(r, res);
                    // 额外读 64K 测试大块
                    if (r[1] > 65536) {
                        Result big = tryRead64K(h, r, buf, brPtr, readMemMH, gleMH);
                        if (big.ok) {
                            System.out.println("  [RPM-64K-OK] 0x" + Long.toHexString(r[0])
                                    + " bytesRead=" + big.bytesRead);
                        } else {
                            System.out.println("  [RPM-64K-FAIL] 0x" + Long.toHexString(r[0])
                                    + " GLE=" + big.gle);
                        }
                    }
                } else {
                    failCount++;
                    System.out.println("  [RPM-FAIL] 0x" + Long.toHexString(r[0])
                            + " GLE=" + res.gle + gleName(res.gle));
                }
            }
            System.out.println("[3] RPM 结果: OK=" + okCount + " FAIL=" + failCount);

            if (selfTest) {
                int[] testArr = new int[]{0x12345678, 0xDEADBEEF};
                MemorySegment arrSeg = MemorySegment.ofArray(testArr);
                MemorySegment selfBuf = arena.allocate(8, 8);
                MemorySegment selfPtr = arena.allocate(8, 8);
                brPtr = selfPtr;
                buf = selfBuf;
                Result sr = tryRead(h, new long[]{arrSeg.address(), 8L}, selfBuf, selfPtr, readMemMH, gleMH);
                if (sr.ok) {
                    int v1 = selfBuf.get(ValueLayout.JAVA_INT, 0);
                    int v2 = selfBuf.get(ValueLayout.JAVA_INT, 4);
                    System.out.println("[selftest] 读回值: 0x" + Integer.toHexString(v1)
                            + " 0x" + Integer.toHexString(v2)
                            + " (期望 0x12345678 0xdeadbeef)");
                } else {
                    System.out.println("[selftest] 读自身也失败, GLE=" + sr.gle);
                }
            }

            closeMH.invoke(h);
        } finally {
            arena.close();
        }
    }

    private static class Result {
        boolean ok;
        int gle;
        long bytesRead;

        Result(boolean ok, int gle, long bytesRead) {
            this.ok = ok;
            this.gle = gle;
            this.bytesRead = bytesRead;
        }
    }

    /**
     * enumRegions。
     *
     * @param h 方法入参 h
     * @param mbi 方法入参 mbi
     * @param vqeMH 方法入参 vqeMH
     * @return 结果列表，无数据时为空列表
     */
    private static List<long[]> enumRegions(long h, MemorySegment mbi, MethodHandle vqeMH) {
        List<long[]> regions = new ArrayList<>();
        long addr = 0x10000;
        int count = 0;
        while (addr < 0x7FFFFFFFFFFFL && count < 5) {
            long ret = vqeInvoke(h, addr, mbi, vqeMH);
            if (ret == 0) {
                break;
            }
            long baseAddr = mbi.get(ValueLayout.JAVA_LONG, 0);
            long regionSize = mbi.get(ValueLayout.JAVA_LONG, 24);
            int state = mbi.get(ValueLayout.JAVA_INT, 32);
            int protect = mbi.get(ValueLayout.JAVA_INT, 36);
            System.out.println("  [VQE] base=0x" + Long.toHexString(baseAddr)
                    + " size=0x" + Long.toHexString(regionSize)
                    + " state=" + state + " protect=0x" + Integer.toHexString(protect));
            if (state == 4096 && regionSize >= 4096) {
                regions.add(new long[]{baseAddr, regionSize});
            }
            addr = baseAddr + regionSize;
            if (regionSize == 0 || addr <= 0x10000) {
                break;
            }
            count++;
        }
        return regions;
    }

    /**
     * enumRegions来自。
     *
     * @param h 方法入参 h
     * @param mbi 方法入参 mbi
     * @param vqeMH 方法入参 vqeMH
     * @param startAddr 启动Addr，不允许为 null
     * @return 结果列表，无数据时为空列表
     */
    private static List<long[]> enumRegionsFrom(long h, MemorySegment mbi, MethodHandle vqeMH, long startAddr) {
        List<long[]> regions = new ArrayList<>();
        long addr = startAddr;
        int count = 0;
        while (addr < 0x7FFFFFFFFFFFL && count < 10) {
            long ret = vqeInvoke(h, addr, mbi, vqeMH);
            if (ret == 0) {
                break;
            }
            long baseAddr = mbi.get(ValueLayout.JAVA_LONG, 0);
            long regionSize = mbi.get(ValueLayout.JAVA_LONG, 24);
            int state = mbi.get(ValueLayout.JAVA_INT, 32);
            int protect = mbi.get(ValueLayout.JAVA_INT, 36);
            if (state == 4096 && regionSize >= 4096) {
                regions.add(new long[]{baseAddr, regionSize});
                System.out.println("  [VQE-extra] base=0x" + Long.toHexString(baseAddr)
                        + " size=0x" + Long.toHexString(regionSize)
                        + " state=" + state + " protect=0x" + Integer.toHexString(protect));
            }
            addr = baseAddr + regionSize;
            if (regionSize == 0 || addr <= startAddr) {
                break;
            }
            count++;
        }
        return regions;
    }

    /**
     * vqe调用。
     *
     * @param h 方法入参 h
     * @param addr 方法入参 addr
     * @param mbi 方法入参 mbi
     * @param vqeMH 方法入参 vqeMH
     * @return 结果数值
     */
    private static long vqeInvoke(long h, long addr, MemorySegment mbi, MethodHandle vqeMH) {
        try {
            return (long) vqeMH.invoke(h, addr, mbi.address(), 64L);
        } catch (Throwable t) {
            System.out.println("  [VQE-exc] " + t);
            return 0;
        }
    }

    private static Result tryRead(long h, long[] r, MemorySegment buf, MemorySegment brPtr,
                                  MethodHandle readMemMH, MethodHandle gleMH) {
        try {
            boolean ok = (boolean) readMemMH.invoke(
                    h, r[0], buf.address(), 4096L, brPtr.address(), 0L);
            if (ok) {
                long bytesRead = brPtr.get(ValueLayout.JAVA_LONG, 0);
                return new Result(true, 0, bytesRead);
            }
            int gle = (int) gleMH.invoke();
            return new Result(false, gle, 0);
        } catch (Throwable t) {
            System.out.println("  [RPM-exc] " + t.getClass().getSimpleName() + ": " + t.getMessage());
            return new Result(false, -1, 0);
        }
    }

    private static Result tryRead64K(long h, long[] r, MemorySegment buf, MemorySegment brPtr,
                                     MethodHandle readMemMH, MethodHandle gleMH) {
        try {
            boolean ok = (boolean) readMemMH.invoke(
                    h, r[0], buf.address(), 65536L, brPtr.address(), 0L);
            if (ok) {
                long bytesRead = brPtr.get(ValueLayout.JAVA_LONG, 0);
                return new Result(true, 0, bytesRead);
            }
            int gle = (int) gleMH.invoke();
            return new Result(false, gle, 0);
        } catch (Throwable t) {
            return new Result(false, -1, 0);
        }
    }

    /**
     * print结果。
     *
     * @param r 方法入参 r
     * @param res 方法入参 res
     */
    private static void printResult(long[] r, Result res) {
        StringBuilder sb = new StringBuilder("  [RPM-OK] 0x" + Long.toHexString(r[0])
                + " bytesRead=" + res.bytesRead + " first8=");
        // 无法直接获取 buf 内容（已传出去了），简化输出
        System.out.println(sb);
    }

    /**
     * gle名称。
     *
     * @param gle 方法入参 gle
     * @return 结果字符串
     */
    private static String gleName(int gle) {
        return switch (gle) {
            case 5 -> " (ACCESS_DENIED)";
            case 87 -> " (INVALID_PARAMETER)";
            case 299 -> " (ERROR_PARTIAL_COPY)";
            case -1 -> " (FFM异常)";
            default -> "";
        };
    }

    /**
     * downcall。
     *
     * @param sym 方法入参 sym
     * @param fd 方法入参 fd
     * @return 方法处理 对象
     * @throws Throwable 当执行过程不满足前置条件时
     */
    private static MethodHandle downcall(MemorySegment sym, FunctionDescriptor fd) throws Throwable {
        return Linker.nativeLinker().downcallHandle(sym, fd);
    }
}
