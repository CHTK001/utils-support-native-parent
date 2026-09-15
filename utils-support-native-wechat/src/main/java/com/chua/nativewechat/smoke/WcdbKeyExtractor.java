package com.chua.nativewechat.smoke;

import com.chua.nativewechat.support.WechatWcdbBridge;

import java.io.RandomAccessFile;
import java.lang.foreign.*;
import java.lang.invoke.MethodHandle;
import java.util.*;

/**
 * 微信 WCDB 密钥提取器。
 *
 * @author CH
 * @since 4.0.0.42
 */
public class WcdbKeyExtractor {

    private static final String WX_ROOT = "C:\\Users\\Administrator\\Documents\\WXWork";
    private static final String SESSION_DB_REL = "1688857360896761\\Backup\\1789184160\\Data\\session.db";
    private static final int PAGE = 4096;
    private static final int CTX_SIZE = 128;

    public static void main(String[] args) throws Throwable {
        int pid = Integer.parseInt(args[0]);
        String dbPath = args.length > 1 ? args[1] : WX_ROOT + "\\" + SESSION_DB_REL;

        byte[] saltBytes = new byte[16];
        try (RandomAccessFile raf = new RandomAccessFile(dbPath, "r")) {
            raf.readFully(saltBytes);
        } catch (Exception e) {
            System.out.println("无法读取 session.db: " + e.getMessage());
            return;
        }
        String saltHex = toHex(saltBytes);
        System.out.println("=== 微信 WCDB 密钥提取器 (4KB 分页模式) ===");
        System.out.println("session.db: " + dbPath);
        System.out.println("文件头 salt (16B): " + saltHex);
        System.out.println();

        Arena arena = Arena.ofShared();
        MethodHandle closeHandleMH = null;
        long hProcess = 0;
        try {
            SymbolLookup k32 = SymbolLookup.libraryLookup("kernel32.dll", arena);

            MethodHandle openProcessMH = downcall(k32.find("OpenProcess").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_LONG,
                            ValueLayout.JAVA_INT, ValueLayout.JAVA_BOOLEAN, ValueLayout.JAVA_INT));
            MethodHandle readMemMH = downcall(k32.find("ReadProcessMemory").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_BOOLEAN,
                            ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG,
                            ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG));
            closeHandleMH = downcall(k32.find("CloseHandle").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_BOOLEAN, ValueLayout.JAVA_LONG));
            MethodHandle vqeMH = downcall(k32.find("VirtualQueryEx").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_LONG,
                            ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG));
            MethodHandle gleMH = downcall(k32.find("GetLastError").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_INT));

            // 打开进程（PROCESS_ALL_ACCESS）
            hProcess = (long) openProcessMH.invoke(0x1FFFFF, false, pid);
            if (hProcess == 0) {
                int gle = (int) gleMH.invoke();
                System.out.println("OpenProcess 失败, GLE=" + gle);
                return;
            }
            System.out.println("OpenProcess OK, handle=0x" + Long.toHexString(hProcess));

            // 枚举 committed 区域，收集 4KB 分页地址
            System.out.println("正在枚举内存区域 (4KB 分页)...");
            MemorySegment mbi = arena.allocate(64, 8);
            List<Long> pages = new ArrayList<>();
            long addr = 0x10000;
            int scanned = 0;
            int committed = 0;

            while (addr < 0x7FFFFFFFFFFFL && scanned < 50000) {
                long ret;
                try {
                    ret = (long) vqeMH.invoke(hProcess, addr, mbi.address(), 64L);
                } catch (Throwable t) {
                    break;
                }
                if (ret == 0) break;
                long baseAddr = mbi.get(ValueLayout.JAVA_LONG, 0);
                long regionSize = mbi.get(ValueLayout.JAVA_LONG, 24);
                int state = mbi.get(ValueLayout.JAVA_INT, 32);
                if (scanned < 5) {
                    System.out.println("[VQE] base=0x" + Long.toHexString(baseAddr)
                            + " size=0x" + Long.toHexString(regionSize)
                            + " state=" + state);
                }
                if (state == 4096) {
                    committed++;
                    // 将 committed 区域按 4KB 对齐分页
                    long alignedStart = (baseAddr + PAGE - 1) & ~(PAGE - 1L);
                    long alignedEnd = baseAddr + regionSize;
                    if (alignedStart < baseAddr) alignedStart = baseAddr;
                    for (long p = alignedStart; p + PAGE <= alignedEnd; p += PAGE) {
                        pages.add(p);
                    }
                }
                addr = baseAddr + regionSize;
                if (regionSize == 0 || addr <= 0x10000) break;
                scanned++;
                if (scanned % 1000 == 0) {
                    System.out.println("  已枚举 " + scanned + " 区域，committed=" + committed
                            + "，pages=" + pages.size());
                }
            }
            System.out.println("共枚举 " + scanned + " 区域，committed=" + committed
                    + "，生成 " + pages.size() + " 个 4KB 分页");

            // 逐 4KB 读取，搜索 salt
            byte[] buf = new byte[PAGE];
            MemorySegment bufSeg = MemorySegment.ofArray(buf);
            MemorySegment bytesReadPtr = arena.allocate(8, 8);
            List<Long> saltHits = new ArrayList<>();
            int totalPages = 0;
            int readOk = 0;
            int readFail = 0;

            for (long pageAddr : pages) {
                boolean ok;
                try {
                    ok = (boolean) readMemMH.invoke(
                            hProcess, pageAddr, bufSeg.address(), (long) PAGE,
                            bytesReadPtr.address(), 0L);
                } catch (Throwable t) {
                    if (totalPages < 5) {
                        System.out.println("[RPM-exc] 0x" + Long.toHexString(pageAddr)
                                + " " + t.getClass().getSimpleName());
                    }
                    totalPages++;
                    continue;
                }
                if (!ok) {
                    readFail++;
                    if (totalPages < 5) {
                        int gle = 0;
                        try { gle = (int) gleMH.invoke(); } catch (Throwable ignored) {}
                        System.out.println("[RPM-fail] 0x" + Long.toHexString(pageAddr)
                                + " n=" + PAGE + " gle=" + gle);
                    }
                    totalPages++;
                    continue;
                }
                readOk++;
                totalPages++;
                // 搜索 salt（16B 完整匹配）
                for (int i = 0; i + saltBytes.length <= PAGE; i++) {
                    if (matches(buf, i, saltBytes)) {
                        saltHits.add(pageAddr + i);
                    }
                }
                if (totalPages % 2000 == 0) {
                    System.out.println("已扫描 " + totalPages + " 页 (OK=" + readOk
                            + " FAIL=" + readFail + ")，命中 salt=" + saltHits.size());
                }
            }
            System.out.println("扫描了 " + totalPages + " 页 (OK=" + readOk
                    + " FAIL=" + readFail + ")，发现 " + saltHits.size() + " 处 salt");

            if (saltHits.isEmpty()) {
                System.out.println(">>> 未在 WXWork 内存中找到 salt，密钥可能未驻留内存 <<<");
                return;
            }

            // 收集候选密钥：读取 salt 前后各 64 字节上下文
            Map<String, Integer> candidateFreq = new LinkedHashMap<>();
            for (Long hit : saltHits) {
                long ctxStart = hit - 32;
                if (ctxStart < 0) continue;
                byte[] ctx = new byte[CTX_SIZE];
                MemorySegment ctxSeg = MemorySegment.ofArray(ctx);
                boolean ok;
                try {
                    ok = (boolean) readMemMH.invoke(
                            hProcess, ctxStart, ctxSeg.address(), CTX_SIZE,
                            bytesReadPtr.address(), 0L);
                } catch (Throwable t) {
                    continue;
                }
                if (!ok) continue;

                // salt 位于 ctx 偏移 32 处
                byte[] before = Arrays.copyOfRange(ctx, 0, 32);
                String beforeHex = toHex(before);
                if (isValidKeyCandidate(beforeHex)) {
                    candidateFreq.merge(beforeHex, 1, Integer::sum);
                }
                byte[] after = Arrays.copyOfRange(ctx, 48, 80);
                if (after.length == 32) {
                    String afterHex = toHex(after);
                    if (isValidKeyCandidate(afterHex)) {
                        candidateFreq.merge(afterHex, 1, Integer::sum);
                    }
                }
            }

            if (candidateFreq.isEmpty()) {
                System.out.println("未找到有效密钥候选。");
                return;
            }

            List<Map.Entry<String, Integer>> sorted = new ArrayList<>(candidateFreq.entrySet());
            sorted.sort((a, b) -> b.getValue() - a.getValue());

            System.out.println("共 " + sorted.size() + " 个唯一候选，按频率排序:");
            int top = Math.min(20, sorted.size());
            for (int i = 0; i < top; i++) {
                Map.Entry<String, Integer> e = sorted.get(i);
                System.out.println("  " + (i + 1) + ". raw=" + e.getKey() + "  freq=" + e.getValue());
            }

            System.out.println("\n开始验证候选...");
            boolean success = false;
            for (Map.Entry<String, Integer> e : sorted) {
                String raw = e.getKey();
                System.out.print("  尝试 raw=" + raw + " ... ");
                try {
                    WechatWcdbBridge bridge = WechatWcdbBridge.load();
                    long handle = bridge.openAccount(dbPath, raw);
                    String sessionsJson = bridge.getSessions(handle);
                    System.out.println("成功! 会话 JSON 长度=" + (sessionsJson != null ? sessionsJson.length() : 0));
                    if (sessionsJson != null && sessionsJson.length() > 10) {
                        System.out.println("前 300 字符: " + sessionsJson.substring(0, Math.min(300, sessionsJson.length())));
                    }
                    bridge.closeAccount(handle);
                    bridge.close();
                    success = true;
                    break;
                } catch (Exception ex) {
                    System.out.println("失败: " + ex.getMessage());
                }
            }
            if (success) {
                System.out.println("\n>>> 密钥提取成功! <<<");
            } else {
                System.out.println("\n>>> 所有候选均失败。需要 Hook/DLL 注入方案。<<<");
            }
        } catch (Exception e) {
            System.out.println("错误: " + e.getMessage());
            e.printStackTrace();
        } finally {
            if (hProcess != 0 && closeHandleMH != null) {
                try {
                    closeHandleMH.invoke(hProcess);
                } catch (Throwable ignored) {}
            }
            arena.close();
        }
    }

    private static MethodHandle downcall(MemorySegment sym, FunctionDescriptor fd) throws Throwable {
        return Linker.nativeLinker().downcallHandle(sym, fd);
    }

    private static boolean isValidKeyCandidate(String hex) {
        if (hex.length() != 64) return false;
        long v = 0;
        try {
            for (int i = 0; i < 8; i++) {
                long chunk = Long.parseLong(hex.substring(i * 8, i * 8 + 8), 16);
                v |= chunk;
            }
        } catch (NumberFormatException e) {
            return false;
        }
        return v != 0;
    }

    private static boolean matches(byte[] data, int pos, byte[] needle) {
        for (int j = 0; j < needle.length; j++) {
            if ((data[pos + j] & 0xFF) != (needle[j] & 0xFF)) return false;
        }
        return true;
    }

    private static String toHex(byte[] bytes) {
        StringBuilder sb = new StringBuilder(bytes.length * 2);
        for (byte b : bytes) sb.append(String.format("%02x", b));
        return sb.toString();
    }
}