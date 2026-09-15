//! SQLCipher 解密管道自测：验证 32B raw key 二进制模式能否读回真实数据。
//!
//! 与 Python sqlcipher3 成功的密钥方式完全一致：
//!   - `PRAGMA key = '0x<64hex>'`（二进制模式，跳过 PBKDF2，32B raw key）
//!   - salt 由 SQLCipher 自动从 DB 文件头读取，无需手动拼接
//!
//! 注意：SQLite PRAGMA 语句不支持参数绑定，必须用字符串字面量。

use rusqlite::{params, Connection, OpenFlags};

fn main() {
    let db_path = r"d:\ch\project\utils-support-native-parent\utils-support-native-wechat\target\test-sqlcipher\session.db";
    let key = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";
    let data = std::fs::read(db_path).expect("读取 DB 文件失败");
    let salt = hex_encode(&data[..16]);
    println!("[0] salt: {}", salt);

    // 检测 SQLite 库版本（应为 SQLCipher）
    let probe = Connection::open_in_memory().expect("open in-memory failed");
    let version: String = probe
        .query_row("SELECT sqlite_version()", params![], |r| r.get(0))
        .expect("version query failed");
    println!("[0] SQLite version: {}", version);

    // 检测 SQLCipher 特征：查询 PRAGMA cipher_version
    match probe.query_row("PRAGMA cipher_version", params![], |r| r.get::<_, String>(0)) {
        Ok(cv) => println!("[0] SQLCipher cipher_version: {}", cv),
        Err(e) => println!("[0] PRAGMA cipher_version 不可用: {}", e),
    }

    // 测试 A: 32B raw key 二进制模式（字符串字面量，先 params 后 key）
    println!("\n=== 测试 A: 32B raw key 二进制模式（字符串字面量，先 params 后 key） ===");
    test_binary_mode(db_path, key, "先params后key", false);

    // 测试 B: 32B raw key 二进制模式（字符串字面量，先 key 后 params）
    println!("\n=== 测试 B: 32B raw key 二进制模式（字符串字面量，先 key 后 params） ===");
    test_binary_mode(db_path, key, "先key后params", true);

    // 测试 C: 96位 raw key + salt 拼接（旧 SQLCipher 3.x 二进制模式兜底）
    println!("\n=== 测试 C: 96位 raw key + salt（字符串字面量，旧模式兜底） ===");
    test_binary_mode_full(db_path, key, &salt);
}

/// 二进制模式测试：`PRAGMA key = '0x<64hex>'`（字符串字面量，32B raw key）
fn test_binary_mode(db_path: &str, key: &str, label: &str, key_first: bool) {
    let uri = format!("file:{}?mode=ro", db_path.replace('\\', "/"));
    let conn = match Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    ) {
        Ok(c) => c,
        Err(e) => {
            println!("[FAIL] {} open: {}", label, e);
            return;
        }
    };
    conn.busy_timeout(std::time::Duration::from_secs(5)).ok();

    let key_literal = format!("'0x{}'", key);

    if key_first {
        // 先 key 后 params
        if let Err(e) = conn.execute_batch(&format!("PRAGMA key = {}", key_literal)) {
            println!("[FAIL] {} execute key: {}", label, e);
            return;
        }
        let batch = format!(
            "PRAGMA cipher_page_size = 4096;\
             PRAGMA kdf_iter = 256000;\
             PRAGMA cipher_hmac_algorithm = HMAC_SHA512;\
             PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;"
        );
        if let Err(e) = conn.execute_batch(&batch) {
            println!("[FAIL] {} execute params: {}", label, e);
            return;
        }
    } else {
        // 先 params 后 key
        let batch = format!(
            "PRAGMA cipher_page_size = 4096;\
             PRAGMA kdf_iter = 256000;\
             PRAGMA cipher_hmac_algorithm = HMAC_SHA512;\
             PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;"
        );
        if let Err(e) = conn.execute_batch(&batch) {
            println!("[FAIL] {} execute params: {}", label, e);
            return;
        }
        if let Err(e) = conn.execute_batch(&format!("PRAGMA key = {}", key_literal)) {
            println!("[FAIL] {} execute key: {}", label, e);
            return;
        }
    }

    // 验证数据
    match conn.query_row("SELECT count(*) FROM sqlite_master", params![], |r| r.get::<_, i64>(0))
    {
        Ok(count) => {
            println!("[OK] {} sqlite_master count: {}", label, count);
            match conn
                .prepare("SELECT username, display_name, last_msg_time FROM session")
                .unwrap()
                .query_map(params![], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .collect::<Result<Vec<(String, String, i64)>, _>>()
            {
                Ok(rows) => {
                    println!("[OK] {} 行数: {}", label, rows.len());
                    for row in &rows {
                        println!("  {:?}", row);
                    }
                }
                Err(e) => println!("[FAIL] {} read: {}", label, e),
            }
        }
        Err(e) => println!("[FAIL] {} read: {}", label, e),
    }
}

/// 96位 raw key + salt 拼接（旧 SQLCipher 3.x 二进制模式兜底）
fn test_binary_mode_full(db_path: &str, key: &str, salt: &str) {
    let uri = format!("file:{}?mode=ro", db_path.replace('\\', "/"));
    let conn = match Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    ) {
        Ok(c) => c,
        Err(e) => {
            println!("[FAIL] 96位拼接 open: {}", e);
            return;
        }
    };
    conn.busy_timeout(std::time::Duration::from_secs(5)).ok();

    let full_key = format!("{}{}", key, salt);
    let batch = format!(
        "PRAGMA cipher_page_size = 4096;\
         PRAGMA kdf_iter = 256000;\
         PRAGMA cipher_hmac_algorithm = HMAC_SHA512;\
         PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;"
    );
    if let Err(e) = conn.execute_batch(&batch) {
        println!("[FAIL] 96位拼接 execute params: {}", e);
        return;
    }
    let key_literal = format!("'0x{}'", full_key);
    if let Err(e) = conn.execute_batch(&format!("PRAGMA key = {}", key_literal)) {
        println!("[FAIL] 96位拼接 execute key: {}", e);
        return;
    }
    match conn.query_row("SELECT count(*) FROM sqlite_master", params![], |r| {
        r.get::<_, i64>(0)
    }) {
        Ok(count) => {
            println!("[OK] 96位拼接 sqlite_master count: {}", count);
            match conn
                .prepare("SELECT username, display_name, last_msg_time FROM session")
                .unwrap()
                .query_map(params![], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .collect::<Result<Vec<(String, String, i64)>, _>>()
            {
                Ok(rows) => {
                    println!("[OK] 96位拼接 行数: {}", rows.len());
                    for row in &rows {
                        println!("  {:?}", row);
                    }
                }
                Err(e) => println!("[FAIL] 96位拼接 read: {}", e),
            }
        }
        Err(e) => println!("[FAIL] 96位拼接 read: {}", e),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}
