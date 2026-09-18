package com.chua.nativewechat.support;

import lombok.extern.slf4j.Slf4j;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.nio.charset.StandardCharsets;

/**
 * 从运行中的微信进程内存提取数据库加密密钥（纯 Java FFM 实现）。
 *
 * @author CH
 * @since 4.0.0.42
 */
@Slf4j
public final class WechatKeyExtractor {

    private static final Arena GLOBAL = Arena.global();
    private static final Linker LINKER = Linker.nativeLinker();
    private static final SymbolLookup KERNEL32 = SymbolLookup.libraryLookup("kernel32.dll", GLOBAL);

    private static final int PROCESS_QUERY_INFORMATION = 0x0400;
    private static final int PROCESS_VM_READ = 0x0010;
    private static final int TH32CS_SNAPPROCESS = 0x00000002;
    private static final int TH32CS_SNAPMODULE = 0x00000008;
    private static final int TH32CS_SNAPMODULE32 = 0x00000010;

    private static final MethodHandle MH_CreateToolhelp32Snapshot;
    private static final MethodHandle MH_Process32FirstW;
    private static final MethodHandle MH_Process32NextW;
    private static final MethodHandle MH_OpenProcess;
    private static final MethodHandle MH_CloseHandle;
    private static final MethodHandle MH_Module32FirstW;
    private static final MethodHandle MH_Module32NextW;
    private static final MethodHandle MH_ReadProcessMemory;
    private static final MethodHandle MH_GetProcessId;

    static {
        try {
            MH_CreateToolhelp32Snapshot = LINKER.downcallHandle(
                    KERNEL32.find("CreateToolhelp32Snapshot").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.JAVA_INT, ValueLayout.JAVA_INT));
            MH_Process32FirstW = LINKER.downcallHandle(
                    KERNEL32.find("Process32FirstW").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            MH_Process32NextW = LINKER.downcallHandle(
                    KERNEL32.find("Process32NextW").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            MH_OpenProcess = LINKER.downcallHandle(
                    KERNEL32.find("OpenProcess").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.JAVA_INT, ValueLayout.JAVA_INT, ValueLayout.JAVA_INT));
            MH_CloseHandle = LINKER.downcallHandle(
                    KERNEL32.find("CloseHandle").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS));
            MH_Module32FirstW = LINKER.downcallHandle(
                    KERNEL32.find("Module32FirstW").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            MH_Module32NextW = LINKER.downcallHandle(
                    KERNEL32.find("Module32NextW").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
            MH_ReadProcessMemory = LINKER.downcallHandle(
                    KERNEL32.find("ReadProcessMemory").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                            ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));
            MH_GetProcessId = LINKER.downcallHandle(
                    KERNEL32.find("GetProcessId").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS));
        } catch (Exception e) {
            throw new RuntimeException("Failed to link Windows API", e);
        }
    }

    /**
     * extract键。
     *
     * @return 结果字符串
     */
    public static String extractKey() {
        log.info("开始从微信进程提取数据库密钥...");

        try (Arena arena = Arena.ofConfined()) {
            System.err.println("[DEBUG] 开始查找微信进程...");
            int pid = findWeChatProcess(arena);
            System.err.println("[DEBUG] findWeChatProcess 返回: " + pid);
            if (pid == -1) {
                throw new IllegalStateException("未找到微信进程，请先启动并登录微信");
            }
            log.info("找到微信进程 PID: {}", pid);

            MemorySegment procHandle = openProcess(arena, pid);
            if (procHandle == null || procHandle.address() == 0) {
                throw new IllegalStateException("无法打开微信进程 (PID: " + pid + ")，请以管理员权限运行");
            }
            log.info("成功打开微信进程");

            try {
                long[] moduleInfo = findModule(arena, procHandle, pid);
                if (moduleInfo == null) {
                    throw new IllegalStateException("未找到微信核心模块 (Weixin.dll/WeChatWin.dll)");
                }
                log.info("找到核心模块: 基地址=0x{}, 大小={}MB",
                        Long.toHexString(moduleInfo[0]), moduleInfo[1] / 1024 / 1024);

                String key = scanForKey(arena, procHandle, moduleInfo[0], (int) moduleInfo[1]);
                if (key != null) {
                    log.info("成功提取数据库密钥: {}...", key.substring(0, 8));
                    return key;
                }

                throw new IllegalStateException("在微信进程内存中未找到数据库密钥");
            } finally {
                closeHandle(arena, procHandle);
            }
        }
    }

    /**
     * 打开处理。
     *
     * @param arena 方法入参 arena
     * @param pid 方法入参 pid
     * @return Memory分段 对象
     */
    private static MemorySegment openProcess(Arena arena, int pid) {
        Object result = invoke(MH_OpenProcess, PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if (result instanceof MemorySegment ms) {
            return ms;
        }
        return null;
    }

    /**
     * 查找WeChat处理。
     *
     * @param arena 方法入参 arena
     * @return 结果数值
     */
    private static int findWeChatProcess(Arena arena) {
        String[] targets = {"Weixin.exe", "WeChat.exe"};
        System.err.println("[DEBUG] findWeChatProcess: 创建快照...");
        Object snapResult = invoke(MH_CreateToolhelp32Snapshot, TH32CS_SNAPPROCESS, 0);
        System.err.println("[DEBUG] snapResult 类型: " + (snapResult == null ? "null" : snapResult.getClass().getName()));
        MemorySegment snapshot;
        if (snapResult instanceof MemorySegment ms) {
            snapshot = ms;
        } else {
            System.err.println("[DEBUG] CreateToolhelp32Snapshot 返回: " + snapResult);
            return -1;
        }
        if (snapshot == null || snapshot.address() == 0) {
            System.err.println("[DEBUG] CreateToolhelp32Snapshot 返回空句柄");
            return -1;
        }
        System.err.println("[DEBUG] 快照句柄: 0x" + Long.toHexString(snapshot.address()));

        try {
            // PROCESSENTRY32W: dwSize(4) + cntUsage(4) + th32ProcessID(4) + th32ModuleID(4) +
            //   cntThreads(4) + th32ParentProcessID(4) + pcPriClassBase(4) + dwFlags(4) + szExeFile(520)
            int entrySize = 4 * 8 + 260 * 2; // = 552
            MemorySegment entry = arena.allocate(entrySize, 4);
            entry.set(ValueLayout.JAVA_INT, 0, entrySize);
            System.err.println("[DEBUG] entrySize=" + entrySize + ", entry.address()=0x" + Long.toHexString(entry.address()));
            System.err.println("[DEBUG] entry bytes at 0-7: " + entry.get(ValueLayout.JAVA_LONG, 0));

            // 尝试获取 last error
            Object firstResult = invoke(MH_Process32FirstW, snapshot, entry);
            System.err.println("[DEBUG] Process32FirstW 返回类型: " + (firstResult == null ? "null" : firstResult.getClass().getName()) + " 值: " + firstResult);
            int rc = (firstResult instanceof Integer intVal) ? intVal : (firstResult instanceof MemorySegment ms2 ? (int) ms2.address() : 0);
            System.err.println("[DEBUG] Process32FirstW rc=" + rc);
            if (rc != 0) {
                int count = 0;
                do {
                    String exeName = readWStringAtOffset(entry, 32, 260);
                    count++;
                    if (count <= 5 || exeName.toLowerCase().contains("weixin") || exeName.toLowerCase().contains("wechat")) {
                        System.err.println("[DEBUG] 进程: " + exeName);
                    }
                    for (String target : targets) {
                        if (target.equalsIgnoreCase(exeName)) {
                            int pid = entry.get(ValueLayout.JAVA_INT, 8);
                            System.err.println("[DEBUG] 找到目标进程: " + exeName + " (PID: " + pid + ")");
                            return pid;
                        }
                    }
                } while (toInt(MH_Process32NextW, snapshot, entry) != 0);
                System.err.println("[DEBUG] 遍历了 " + count + " 个进程");
            } else {
                System.err.println("[DEBUG] Process32FirstW 失败");
            }
        } finally {
            closeHandle(arena, snapshot);
        }
        return -1;
    }

    /**
     * 查找Module。
     *
     * @param arena 方法入参 arena
     * @param procHandle proc处理，不允许为 null
     * @param pid 方法入参 pid
     * @return 结果值
     */
    private static long[] findModule(Arena arena, MemorySegment procHandle, int pid) {
        String[] targets = {"Weixin.dll", "WeChatWin.dll"};
        Object snapResult = invoke(MH_CreateToolhelp32Snapshot,
                TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
        MemorySegment snapshot;
        if (snapResult instanceof MemorySegment ms) {
            snapshot = ms;
        } else {
            return null;
        }
        if (snapshot == null || snapshot.address() == 0) {
            return null;
        }

        try {
            // MODULEENTRY32W: dwSize(4) + th32ModuleID(4) + th32ProcessID(4) + GlblcntUsage(4) +
            //   ProccntUsage(4) + padding(4) + modBaseAddr(8) + modBaseSize(4) + padding(4) +
            //   hModule(8) + szModule(520) + szExePath(1042)
            int entrySize = 4 * 5 + 4 + 8 + 4 + 4 + 8 + 260 * 2 + (260 * 2 + 1) * 2;
            MemorySegment entry = arena.allocate(entrySize, 4);
            entry.set(ValueLayout.JAVA_INT, 0, entrySize);

            if (toInt(MH_Module32FirstW, snapshot, entry) != 0) {
                do {
                    String modName = readWStringAtOffset(entry, 44, 260);
                    for (String target : targets) {
                        if (target.equalsIgnoreCase(modName)) {
                            long baseAddr = entry.get(ValueLayout.ADDRESS, 24).address();
                            int size = entry.get(ValueLayout.JAVA_INT, 32);
                            return new long[]{baseAddr, size};
                        }
                    }
                } while (toInt(MH_Module32NextW, snapshot, entry) != 0);
            }
        } finally {
            closeHandle(arena, snapshot);
        }
        return null;
    }

    /**
     * scanFor键。
     *
     * @param arena 方法入参 arena
     * @param procHandle proc处理，不允许为 null
     * @param baseAddr 方法入参 baseAddr
     * @param size 大小，不允许为 null
     * @return 结果字符串
     */
    private static String scanForKey(Arena arena, MemorySegment procHandle, long baseAddr, int size) {
        final long CHUNK = 4L * 1024 * 1024;
        long offset = 0;

        while (offset < size) {
            long readSize = Math.min(CHUNK + 64, size - offset);
            MemorySegment buffer = arena.allocate(readSize);
            MemorySegment bytesRead = arena.allocate(ValueLayout.JAVA_LONG);

            int result = toInt(MH_ReadProcessMemory, procHandle, baseAddr + offset, buffer, readSize, bytesRead);
            if (result != 0) {
                long actual = bytesRead.get(ValueLayout.JAVA_LONG, 0);
                if (actual > 0) {
                    byte[] bytes = new byte[(int) actual];
                    buffer.asByteBuffer().get(bytes);
                    String key = searchKey(bytes);
                    if (key != null) {
                        return key;
                    }
                }
            }
            offset += CHUNK;
        }
        return null;
    }

    /**
     * 搜索键。
     *
     * @param bytes 字节数组，不允许为 null
     * @return 结果字符串
     */
    private static String searchKey(byte[] bytes) {
        // 方法1：搜索 SetDBKey 后面的 hex 密钥
        byte[] setDbKey = "SetDBKey".getBytes(StandardCharsets.US_ASCII);
        int idx = indexOf(bytes, setDbKey);
        if (idx >= 0) {
            String key = extractHexKeyAt(bytes, idx + setDbKey.length);
            if (key != null) {
                return key;
            }
        }

        // 方法2：搜索 SetKey 后面的 hex 密钥
        byte[] setKey = "SetKey".getBytes(StandardCharsets.US_ASCII);
        idx = indexOf(bytes, setKey);
        if (idx >= 0) {
            String key = extractHexKeyAt(bytes, idx + setKey.length);
            if (key != null) {
                return key;
            }
        }

        // 方法3：暴力搜索 64 字符 hex 字符串
        for (int i = 0; i <= bytes.length - 64; i++) {
            if (isHexAt(bytes, i, 64)) {
                String candidate = new String(bytes, i, 64, StandardCharsets.US_ASCII).toLowerCase();
                if (!candidate.chars().allMatch(c -> c == '0') && !candidate.chars().allMatch(c -> c == 'f')) {
                    return candidate;
                }
            }
        }
        return null;
    }

    /**
     * extractHex键At。
     *
     * @param bytes 字节数组，不允许为 null
     * @param start 启动，不允许为 null
     * @return 结果字符串
     */
    private static String extractHexKeyAt(byte[] bytes, int start) {
        // 跳过非 hex 字符找到 hex 起始位置
        for (int i = start; i <= Math.min(start + 128, bytes.length - 64); i++) {
            if (isHexAt(bytes, i, 64)) {
                String candidate = new String(bytes, i, 64, StandardCharsets.US_ASCII).toLowerCase();
                if (!candidate.chars().allMatch(c -> c == '0') && !candidate.chars().allMatch(c -> c == 'f')) {
                    return candidate;
                }
            }
        }
        return null;
    }

    /**
     * 索引Of。
     *
     * @param haystack 方法入参 haystack
     * @param needle 方法入参 needle
     * @return 结果数值
     */
    private static int indexOf(byte[] haystack, byte[] needle) {
        outer:
        for (int i = 0; i <= haystack.length - needle.length; i++) {
            for (int j = 0; j < needle.length; j++) {
                if (haystack[i + j] != needle[j]) {
                    continue outer;
                }
            }
            return i;
        }
        return -1;
    }

    /**
     * 是否HexAt。
     *
     * @param bytes 字节数组，不允许为 null
     * @param offset 偏移量，不允许为 null
     * @param len 方法入参 len
     * @return 是否成功（true 表示成功）
     */
    private static boolean isHexAt(byte[] bytes, int offset, int len) {
        for (int i = offset; i < offset + len; i++) {
            byte b = bytes[i];
            if (!((b >= '0' && b <= '9') || (b >= 'a' && b <= 'f') || (b >= 'A' && b <= 'F'))) {
                return false;
            }
        }
        return true;
    }

    /**
     * 读取W字符串At偏移量。
     *
     * @param struct 方法入参 struct
     * @param charOffset char偏移量，不允许为 null
     * @param maxChars 最大值Chars，不允许为 null
     * @return 结果字符串
     */
    private static String readWStringAtOffset(MemorySegment struct, int charOffset, int maxChars) {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < maxChars; i++) {
            char c = struct.get(ValueLayout.JAVA_CHAR, charOffset + i * 2);
            if (c == 0) {
                break;
            }
            sb.append(c);
        }
        return sb.toString();
    }

    /**
     * 转为Int。
     *
     * @param mh 方法入参 mh
     * @param args 参数，不允许为 null
     * @return 结果数值
     */
    private static int toInt(MethodHandle mh, Object... args) {
        try {
            return (int) mh.invokeWithArguments(args);
        } catch (Throwable e) {
            return 0;
        }
    }

    /**
     * 调用。
     *
     * @param mh 方法入参 mh
     * @param args 参数，不允许为 null
     * @return 对象 对象
     */
    private static Object invoke(MethodHandle mh, Object... args) {
        try {
            return mh.invokeWithArguments(args);
        } catch (Throwable e) {
            return null;
        }
    }

    /**
     * 关闭处理。
     *
     * @param arena 方法入参 arena
     * @param handle 处理，不允许为 null
     */
    private static void closeHandle(Arena arena, MemorySegment handle) {
        try {
            MH_CloseHandle.invokeWithArguments(handle);
        } catch (Throwable ignored) {
            // 关闭失败可忽略
        }
    }

    /**
     * 程序入口，运行示例自检。
     *
     * @param args 参数，不允许为 null
     */
    public static void main(String[] args) {
        try {
            String key = extractKey();
            System.out.println("密钥: " + key);
        } catch (Exception e) {
            System.err.println("错误: " + e.getMessage());
        }
    }
}
