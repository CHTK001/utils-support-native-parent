package com.chua.datarecovery.support;

import java.nio.file.Files;
import java.nio.file.InvalidPathException;
import java.nio.file.Path;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.CompletableFuture;

/**
 * 数据恢复的设备扫描与文件恢复入口，通过 JNI 调用 {@code data_recovery_ffi}。
 *
 * <p>静态块用 {@link com.chua.common.support.utils.NativeUtils#loadFromClasspath(String)}
 * 加载原生库；{@code nativeScan} / {@code nativeScanAndRecover} / {@code nativeRecover}
 * 为私有原生方法。</p>
 *
 * <p><b>包名不可改</b>：原生库导出的符号是
 * {@code Java_com_chua_datarecovery_support_DataRecovery_*}，把本类的全限定名
 * 编译进了二进制。改包名会导致原生方法无法解析（UnsatisfiedLinkError），
 * 除非同步重建 4 平台 Rust 库。</p>
 *
 * <p>本文件原位于 {@code utils-support-datarecovery-starter}，为让 JNI 绑定与动态库
 * 同住而迁入本模块；因全限定名保持不变，调用方无需改动。</p>
 *
 * <h3>success 字段的口径（重要）</h3>
 * <p>原生 {@code ScanResultJson.success} 与 {@code message} 是<b>硬编码</b>的
 * （恒为 {@code true} 与 {@code "Scan completed"}），只有 JNI 取字符串失败时才是
 * false。原生 {@code scan_walkdir} 遇到不存在的根目录时只往 stderr 打印
 * "root does not exist" 并返回空统计，不上报错误。因此本类在调用原生之前先做目标
 * 校验：非设备路径且不存在时，直接返回 {@code success=false} 的失败结果，不调原生。
 * 设备路径（{@code \\.\} / {@code \\?\} / {@code /dev/}）无法用文件 API 判断，
 * 仍交由原生处理。</p>
 *
 * <h3>回调</h3>
 * <p>{@link RecoveryCallback} 由本类在 Java 侧触发：各操作开始时
 * {@code onProgress(阶段, 0)}、结束时 {@code onProgress(阶段, 100)}；扫描类操作对
 * 每条结果触发 {@code onFileFound}；失败时触发 {@code onError}（随后仍按原语义返回
 * 失败结果或抛出）。原生侧不接受回调参数，故此前 {@code callback()} 设置的回调
 * 永远不会被触发。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */

public class DataRecovery {

    static {
        com.chua.common.support.utils.NativeUtils.loadFromClasspath("data_recovery_ffi");
    }

    /**
     * Device路径
    */
    private final String devicePath;

    /**
     * 创建 数据recovery 实例
     * @param devicePath device路径
     */
    private DataRecovery(String devicePath) {
        this.devicePath = devicePath;
    }

    /**
     * 的
     *
     * @param devicePath device路径
     * @return 的的结果
     */
    public static DataRecovery of(String devicePath) {
        return new DataRecovery(normalizeDevicePath(devicePath));
    }

    /**
     * normalizedevice路径
     *
     * @param path 路径
     * @return normalizedevice路径的结果
     */
    private static String normalizeDevicePath(String path) {
        if (path == null || path.isEmpty()) {
            return path;
        }
        String lower = path.toLowerCase();
        if (lower.startsWith("\\\\.\\") || lower.startsWith("\\\\?\\") || lower.startsWith("/dev/")) {
            return path;
        }
        // 仅对裸盘符（如 "F:"、"F:\"）转换为原始设备路径 \\.\F:
        // 完整路径（如 "C:\Users\..."）保持原样，供 walkdir 等模式使用
        if (lower.matches("^[a-z]:\\\\?$")) {
            return "\\\\.\\" + path.substring(0, 1).toUpperCase() + ":";
        }
        return path;
    }

    /**
     * Callback
     *
     * @param callback callback
     * @return callback的结果
     */
    public DataRecovery callback(RecoveryCallback callback) {
        this.callback = callback;
        return this;
    }

    /**
     * Callback
    */
    private RecoveryCallback callback;

    public static interface RecoveryCallback {
        void onProgress(String stage, int percent);
        void onFileFound(FileEntry entry);
        void onRecovered(String path, long bytes);
        void onError(String error);
    }

    public static class FileEntry {
        /**
         * 名称
        */
        public String name;
        /**
         * 路径
        */
        public String path;
        /**
         * 尺寸bytes
        */
        public long sizeBytes;
        /**
         * Modified时间戳
        */
        public long modifiedTimestamp;
        /**
         * 删除标记时间戳
        */
        public long deletedTimestamp;
        /**
         * Recovery分数
        */
        public int recoveryScore;
        /**
         * Carved签名
        */
        public String carvedSignature;
    }

    public static class ScanResult {
        /**
         * 成功
        */
        public boolean success;
        /**
         * Filesscanned
        */
        public int filesScanned;
        /**
         * Filesfound
        */
        public int filesFound;
        /**
         * 消息
        */
        public String message;
        /**
         * Entries
        */
        public FileEntry[] entries;
    }

    public static class DeleteResult {
        /**
         * 成功
        */
        public boolean success;
        /**
         * Bytesoverwritten
        */
        public long bytesOverwritten;
        /**
         * Passescompleted
        */
        public int passesCompleted;
        /**
         * 消息
        */
        public String message;
    }

    public static class RecoverResult {
        /**
         * 是否成功。
         *
         * <p>原生 RecoverResultJson 返回该字段；此前本类没有对应属性，
         * 而 parse() 用的 ObjectMapper 未关闭 FAIL_ON_UNKNOWN_PROPERTIES，
         * 于是 recover 即使操作成功也会抛 "Parse JSON failed"。</p>
        */
        public boolean success;
        /**
         * 结果描述，来自原生 message 字段
        */
        public String message;
        /**
         * 成功数量
        */
        public int successCount;
        /**
         * 失败数量
        */
        public int failedCount;
        /**
         * 失败列表
        */
        public FailedItem[] failedList;
        /**
         * 总数byteswritten
        */
        public long totalBytesWritten;
    }

    public static class FailedItem {
        /**
         * 路径
        */
        public String path;
        /**
         * ReasonMLML
        */
        public String reason;
    }

    /**
     * 扫描
     *
     * @param scanMode 扫描mode
     * @return 扫描的结果
     */
    public ScanResult scan(int scanMode) {
        String reason = validateTarget();
        if (reason != null) {
            return failure(reason, STAGE_SCAN);
        }
        progress(STAGE_SCAN, 0);
        try {
            String json = nativeScan(devicePath, scanMode);
            ScanResult result = parse(json, ScanResult.class);
            if (result == null) {
                return failure("原生返回空结果", STAGE_SCAN);
            }
            if (callback != null && result.entries != null) {
                for (FileEntry entry : result.entries) {
                    callback.onFileFound(entry);
                }
            }
            progress(STAGE_SCAN, 100);
            return result;
        } catch (RuntimeException e) {
            notifyError(STAGE_SCAN + " 失败: " + e.getMessage());
            throw e;
        }
    }

    /**
     * 扫描和recover
     *
     * @param scanMode 扫描mode
     * @param outputDir 输出dir
     * @return 扫描和recover的结果
     */
    public ScanResult scanAndRecover(int scanMode, String outputDir) {
        String reason = validateTarget();
        if (reason == null) {
            reason = validateOutputDir(outputDir);
        }
        if (reason != null) {
            return failure(reason, STAGE_SCAN_RECOVER);
        }
        progress(STAGE_SCAN_RECOVER, 0);
        try {
            String json = nativeScanAndRecover(devicePath, scanMode, outputDir);
            ScanResult result = parse(json, ScanResult.class);
            if (result == null) {
                return failure("原生返回空结果", STAGE_SCAN_RECOVER);
            }
            if (callback != null && result.entries != null) {
                for (FileEntry entry : result.entries) {
                    callback.onFileFound(entry);
                    callback.onRecovered(entry.path, entry.sizeBytes);
                }
            }
            progress(STAGE_SCAN_RECOVER, 100);
            return result;
        } catch (RuntimeException e) {
            notifyError(STAGE_SCAN_RECOVER + " 失败: " + e.getMessage());
            throw e;
        }
    }

    /**
     * Recover
     *
     * @param filePaths 文件路径
     * @param outputDir 输出dir
     * @param preserveStructure preserve结构
     * @return recover的结果
     */
    public RecoverResult recover(String[] filePaths, String outputDir, boolean preserveStructure) {
        String reason = validateTarget();
        if (reason == null) {
            reason = validateOutputDir(outputDir);
        }
        if (reason == null && (filePaths == null || filePaths.length == 0)) {
            reason = "待恢复文件列表为空";
        }
        if (reason != null) {
            RecoverResult failed = new RecoverResult();
            failed.successCount = 0;
            failed.failedCount = filePaths == null ? 0 : filePaths.length;
            failed.totalBytesWritten = 0;
            failed.failedList = new FailedItem[0];
            notifyError(reason);
            return failed;
        }
        progress(STAGE_RECOVER, 0);
        try {
            String json = nativeRecover(devicePath, filePaths, outputDir, preserveStructure);
            RecoverResult result = parse(json, RecoverResult.class);
            if (result == null) {
                notifyError("原生返回空结果");
                return null;
            }
            if (callback != null && result.failedList != null) {
                for (FailedItem item : result.failedList) {
                    callback.onError(item.path + ": " + item.reason);
                }
            }
            progress(STAGE_RECOVER, 100);
            return result;
        } catch (RuntimeException e) {
            notifyError(STAGE_RECOVER + " 失败: " + e.getMessage());
            throw e;
        }
    }

    /**
     * Permanent删除
     *
     * @param filePath 文件路径
     * @param method 方法
     * @return permanent删除的结果
     */
    public DeleteResult permanentDelete(String filePath, String method) {
        String reason = validateTarget();
        if (reason == null && (filePath == null || filePath.isEmpty())) {
            reason = "待删除文件路径为空";
        }
        if (reason != null) {
            DeleteResult refused = new DeleteResult();
            refused.success = false;
            refused.bytesOverwritten = 0;
            refused.passesCompleted = 0;
            refused.message = reason;
            notifyError(reason);
            return refused;
        }
        progress(STAGE_DELETE, 0);
        try {
            String json = nativeDelete(devicePath, filePath, method);
            DeleteResult result = parse(json, DeleteResult.class);
            if (result == null) {
                notifyError("原生返回空结果");
                return null;
            }
            if (result.success) {
                // 删除没有对应的回调语义，仅以进度与结果表达；不误用 onRecovered
                progress(STAGE_DELETE, 100);
            } else {
                notifyError(result.message);
            }
            return result;
        } catch (RuntimeException e) {
            notifyError(STAGE_DELETE + " 失败: " + e.getMessage());
            throw e;
        }
    }

    /**
     * 扫描异步
     *
     * @param scanMode 扫描mode
     * @return 扫描异步的结果
     */
    public CompletableFuture<ScanResult> scanAsync(int scanMode) {
        return CompletableFuture.supplyAsync(() -> scan(scanMode));
    }

    /**
     * recover异步
     *
     * @param filePaths 文件路径
     * @param outputDir 输出dir
     * @param preserveStructure preserve结构
     * @return recover异步的结果
     */
    public CompletableFuture<RecoverResult> recoverAsync(String[] filePaths, String outputDir, boolean preserveStructure) {
        return CompletableFuture.supplyAsync(() -> recover(filePaths, outputDir, preserveStructure));
    }

    /**
     * 删除异步
     *
     * @param filePath 文件路径
     * @param method 方法
     * @return 删除异步的结果
     */
    public CompletableFuture<DeleteResult> deleteAsync(String filePath, String method) {
        return CompletableFuture.supplyAsync(() -> permanentDelete(filePath, method));
    }

    // ==================== 阶段名 ====================

    /**
     * 阶段名：扫描
     */
    private static final String STAGE_SCAN = "scan";

    /**
     * 阶段名：扫描并恢复
     */
    private static final String STAGE_SCAN_RECOVER = "scanAndRecover";

    /**
     * 阶段名：恢复
     */
    private static final String STAGE_RECOVER = "recover";

    /**
     * 阶段名：永久删除
     */
    private static final String STAGE_DELETE = "permanentDelete";

    // ==================== 目标校验与回调 ====================

    /**
     * 校验扫描目标。
     *
     * <p>原生对不存在的根目录也返回 {@code success=true}（该字段硬编码），故在此前置
     * 拦截。设备路径（{@code \\.\} / {@code \\?\} / {@code /dev/}）无法用文件 API
     * 判断存在性，返回 null 交由原生处理。</p>
     *
     * @return 不可用原因；可用或无法判定时返回 null
     */
    private String validateTarget() {
        if (devicePath == null || devicePath.isEmpty()) {
            return "devicePath 为空";
        }
        String lower = devicePath.toLowerCase(Locale.ROOT);
        if (lower.startsWith("\\\\.\\") || lower.startsWith("\\\\?\\") || lower.startsWith("/dev/")) {
            return null;
        }
        try {
            return Files.exists(Path.of(devicePath)) ? null : "目标路径不存在: " + devicePath;
        } catch (InvalidPathException e) {
            return "目标路径非法: " + devicePath;
        }
    }

    /**
     * 校验输出目录参数。
     *
     * @param outputDir 输出目录
     * @return 不可用原因；可用时返回 null
     */
    private String validateOutputDir(String outputDir) {
        return outputDir == null || outputDir.isEmpty() ? "输出目录为空" : null;
    }

    /**
     * 构造扫描失败结果并触发错误回调。
     *
     * @param reason 失败原因
     * @param stage  阶段名
     * @return 失败结果
     */
    private ScanResult failure(String reason, String stage) {
        ScanResult result = new ScanResult();
        result.success = false;
        result.filesScanned = 0;
        result.filesFound = 0;
        result.entries = new FileEntry[0];
        result.message = reason;
        notifyError(reason);
        progress(stage, 100);
        return result;
    }

    /**
     * 触发进度回调。
     *
     * @param stage   阶段名
     * @param percent 百分比
     */
    private void progress(String stage, int percent) {
        if (callback != null) {
            callback.onProgress(stage, percent);
        }
    }

    /**
     * 触发错误回调。
     *
     * @param error 错误描述
     */
    private void notifyError(String error) {
        if (callback != null) {
            callback.onError(error);
        }
    }

    /**
     * Native扫描
     *
     * @param devicePath device路径
     * @param scanMode 扫描mode
     * @return NAT扫描的结果
     */
    private native String nativeScan(String devicePath, int scanMode);
    /**
     * NAT扫描和recover
     *
     * @param devicePath device路径
     * @param scanMode 扫描mode
     * @param outputDir 输出dir
     * @return NAT扫描和recover的结果
     */
    private native String nativeScanAndRecover(String devicePath, int scanMode, String outputDir);
    /**
     * natrecover
     *
     * @param devicePath device路径
     * @param filePaths 文件路径
     * @param outputDir 输出dir
     * @param preserveStructure preserve结构
     * @return NATrecover的结果
     */
    private native String nativeRecover(String devicePath, String[] filePaths, String outputDir, boolean preserveStructure);
    /**
     * Native删除
     *
     * @param devicePath device路径
     * @param filePath 文件路径
     * @param method 方法
     * @return NAT删除的结果
     */
    private native String nativeDelete(String devicePath, String filePath, String method);

    /**
     * 解析
     *
     * @param json json
     * @param clazz clazz
     * @return 解析的结果
     * @author CH
     * @since 4.0.0
     */
    private static <T> T parse(String json, Class<T> clazz) {
        try {
            com.fasterxml.jackson.databind.ObjectMapper mapper = new com.fasterxml.jackson.databind.ObjectMapper();
            mapper.setPropertyNamingStrategy(com.fasterxml.jackson.databind.PropertyNamingStrategies.SNAKE_CASE);
            return mapper.readValue(json, clazz);
        } catch (Exception e) {
            throw new RecoveryException("Parse JSON failed: " + json, e);
        }
    }

    public static class RecoveryException extends RuntimeException {
        /**
         * 创建 recovery异常 实例
         * @param message 消息
         * @param cause Throwable
         * @param cause cause
         */
        public RecoveryException(String message, Throwable cause) { super(message, cause); }
    }
}
