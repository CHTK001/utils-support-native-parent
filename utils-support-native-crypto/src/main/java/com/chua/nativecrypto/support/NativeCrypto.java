package com.chua.nativecrypto.support;

/**
 * 基于 Rust cdylib 并通过 JNI 加载的原生加密门面。
 *
 * <p>此类应在 JVM 启动时初始化一次。每个加密实例持有独立状态，
 * 可在单个线程中安全使用。</p>
 *
 * @author CH
 * @since 4.0.0.42
 */
public final class NativeCrypto {

    /**
     * 仅负责加载动态库，不提供任何业务接口。
     */
    static {
        NativeCryptoJniLoader.load();
    }

    private NativeCrypto() {
    }

    /**
     * 使用 AES-GCM 算法加密数据。
     *
     * @param key 加密密钥
     * @param nonce 随机数
     * @param plaintext 明文数据
     * @param tagLengthBits 认证标签长度（位）
     * @return 加密状态句柄
     */
    public static native long aesGcmEncrypt(byte[] key, byte[] nonce, byte[] plaintext, int tagLengthBits);

    /**
     * 更新 AES-GCM 加密状态。
     *
     * @param state 加密状态句柄
     * @param plaintext 明文数据
     * @param offset 偏移量
     * @param len 长度
     * @param out 输出缓冲区
     * @param outOffset 输出偏移量
     * @return 写入字节数
     */
    public static native int aesGcmEncryptUpdate(long state, byte[] plaintext, int offset, int len, byte[] out, int outOffset);

    /**
     * 完成 AES-GCM 加密。
     *
     * @param state 加密状态句柄
     * @param out 输出缓冲区
     * @param outOffset 输出偏移量
     * @return 写入字节数
     */
    public static native int aesGcmEncryptDoFinal(long state, byte[] out, int outOffset);

    /**
     * 释放 AES-GCM 加密状态。
     *
     * @param state 加密状态句柄
     */
    public static native void aesGcmFree(long state);

    /**
     * 使用 AES-GCM 算法解密数据。
     *
     * @param key 解密密钥
     * @param nonce 随机数
     * @param ciphertext 密文数据
     * @param tag 认证标签
     * @param tagLengthBits 认证标签长度（位）
     * @return 解密后的明文
     */
    public static native byte[] aesGcmDecrypt(byte[] key, byte[] nonce, byte[] ciphertext, byte[] tag, int tagLengthBits);

    /**
     * 使用 ChaCha20-Poly1305 算法加密数据。
     *
     * @param key 加密密钥
     * @param nonce 随机数
     * @param plaintext 明文数据
     * @return 加密后的密文
     */
    public static native byte[] chacha20Poly1305Encrypt(byte[] key, byte[] nonce, byte[] plaintext);

    /**
     * 更新 ChaCha20-Poly1305 加密状态。
     *
     * @param state 加密状态句柄
     * @param plaintext 明文数据
     * @param offset 偏移量
     * @param len 长度
     * @param out 输出缓冲区
     * @param outOffset 输出偏移量
     * @return 写入字节数
     */
    public static native int chacha20Poly1305EncryptUpdate(long state, byte[] plaintext, int offset, int len, byte[] out, int outOffset);

    /**
     * 完成 ChaCha20-Poly1305 加密。
     *
     * @param state 加密状态句柄
     * @param out 输出缓冲区
     * @param outOffset 输出偏移量
     * @return 写入字节数
     */
    public static native int chacha20Poly1305EncryptDoFinal(long state, byte[] out, int outOffset);

    /**
     * 释放 ChaCha20-Poly1305 加密状态。
     *
     * @param state 加密状态句柄
     */
    public static native void chacha20Poly1305Free(long state);

    /**
     * 使用 ChaCha20-Poly1305 算法解密数据。
     *
     * @param key 解密密钥
     * @param nonce 随机数
     * @param ciphertext 密文数据
     * @param tag 认证标签
     * @return 解密后的明文
     */
    public static native byte[] chacha20Poly1305Decrypt(byte[] key, byte[] nonce, byte[] ciphertext, byte[] tag);

    /**
     * 计算 HMAC-SHA256。
     *
     * @param key 密钥
     * @param message 消息
     * @return HMAC 摘要
     */
    public static native byte[] hmacSha256(byte[] key, byte[] message);
}
