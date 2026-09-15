//! 微信 4.x WCDB 原生读取库（自研，跨平台替代闭源 wcdb_api.dll）。
//!
//! <p>微信 4.x 的本地数据库基于 WCDB（SQLCipher 4）：AES-256-CBC + HMAC-SHA512，
//! 页大小 4096，PBKDF2-HMAC-SHA512 迭代 256000 次。本库使用 rusqlite 的
//! bundled-sqlcipher-vendored-openssl 特性静态链接 SQLCipher，以 64 位十六进制
//! 原始密钥直接解密读取，不依赖微信进程或任何闭源动态库。</p>
//!
//! <h3>导出的 C ABI（与 wcdb_api.dll 对齐）</h3>
//! <ul>
//!   <li>{@code int wechat_wcdb_open_account(const char* path, const char* key, int64* h)}</li>
//!   <li>{@code int wechat_wcdb_close_account(int64 h)}</li>
//!   <li>{@code int wechat_wcdb_get_sessions(int64 h, void** out)}</li>
//!   <li>{@code int wechat_wcdb_get_messages(int64 h, const char* username, int limit, int offset, void** out)}</li>
//!   <li>{@code int wechat_wcdb_get_message_count(int64 h, const char* username, int* out)}</li>
//!   <li>{@code int wechat_wcdb_get_display_names(int64 h, const char* json, void** out)}</li>
//!   <li>{@code void wechat_wcdb_free_string(void* p)}</li>
//!   <li>{@code const char* wechat_wcdb_last_error()}</li>
//! </ul>
//!
//! @author CH
//! @since 4.0.0.42

use rusqlite::{Connection, OpenFlags};
use serde_json::{Map, Value};
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 返回码：成功
const RC_OK: i32 = 0;
/// 返回码：参数错误
const RC_ARG: i32 = 1;
/// 返回码：打开/解密失败
const RC_OPEN: i32 = 2;
/// 返回码：查询失败
const RC_QUERY: i32 = 3;

/// 微信 4.x SQLCipher 页大小
const CIPHER_PAGE_SIZE: i64 = 4096;
/// 微信 4.x PBKDF2 迭代次数
const KDF_ITER: i64 = 256_000;
/// 默认分页条数
const DEFAULT_LIMIT: i64 = 500;
/// IN 查询分批大小
const IN_BATCH: usize = 450;

/// 会话表候选名（小写匹配）
const SESSION_TABLES: &[&str] = &["session", "sessions"];
/// 消息表候选名（小写匹配）
const MESSAGE_TABLES: &[&str] = &["message", "messages", "msg", "chatmsg"];
/// 联系人表候选名（小写匹配）
const CONTACT_TABLES: &[&str] = &["contact", "contacts", "nameinfo", "friend", "wccontact"];

/// 会话归属列候选（消息表中的会话对方 wxid）
const CONV_COLUMNS: &[&str] = &[
    "username", "talker", "conversation", "conv_username", "chat_username", "peer_username",
];
/// 消息时间排序列候选
const TIME_COLUMNS: &[&str] = &["create_time", "createtime", "time", "msg_time", "create_timestamp"];
/// 发送者列候选
const SENDER_COLUMNS: &[&str] = &[
    "sender", "sender_username", "from_username", "sender_wxid", "fromuser", "sendusername",
];
/// 消息类型列候选
const TYPE_COLUMNS: &[&str] = &["local_type", "type", "msg_type", "message_type"];
/// 文本内容列候选
const CONTENT_COLUMNS: &[&str] = &[
    "message_content", "content", "msg_content", "str_content", "content_text",
];
/// 压缩内容列候选
const COMPRESS_COLUMNS: &[&str] = &[
    "compress_content", "compressed_content", "bytes_extra", "compress_content_zstd",
];
/// 会话标识列候选
const SESSION_ID_COLUMNS: &[&str] = &["username", "wxid", "id", "username_v2"];
/// 会话名称列候选
const SESSION_NAME_COLUMNS: &[&str] = &[
    "display_name", "displayname", "display", "nickname", "remark", "name",
];
/// 联系人标识列候选
const CONTACT_ID_COLUMNS: &[&str] = &["username", "wxid", "id", "user_name"];
/// 联系人名称列候选（按优先级：备注名优先）
const CONTACT_NAME_COLUMNS: &[&str] = &[
    "remark", "remark_name", "displayname", "display_name", "nickname", "nick_name", "name",
];

thread_local! {
    /// 最近一次错误信息（线程局部，与 wcdb_api 的 GetLastErrorMsg 用法对齐）
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// 记录最近一次错误
fn set_error(message: impl Into<String>) {
    let text = message.into();
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() =
            Some(CString::new(text).unwrap_or_else(|_| CString::new("error").unwrap()));
    });
}

/// 单个消息分片库（message_0.db ... message_n.db，已 ATTACH 到主连接）
struct MsgDb {
    /// ATTACH 别名
    alias: String,
    /// 消息表名
    table: String,
    /// 表全部列
    columns: Vec<String>,
    /// 会话归属列
    conv_col: String,
    /// 时间排序列
    time_col: Option<String>,
}

/// 显示名解析表
struct NameTable {
    /// ATTACH 别名，None 表示主库
    alias: Option<String>,
    /// 表名
    table: String,
    /// 标识列
    id_col: String,
    /// 名称列
    name_col: String,
}

/// 打开的微信账号库上下文
struct Account {
    /// 主连接（session.db，contact/message 分片均以只读方式 ATTACH）
    conn: Mutex<Connection>,
    /// 会话表名
    session_table: Option<String>,
    /// 消息分片库元数据
    msg_dbs: Vec<MsgDb>,
    /// 显示名解析表（按优先级排序）
    name_tables: Vec<NameTable>,
}

/// 打开微信账号会话库。
///
/// # 参数
/// * `path` - session.db 绝对路径（UTF-8）
/// * `key` - 64 位十六进制原始密钥
/// * `out_handle` - 出参，账号库句柄
///
/// # 返回
/// 0 成功，非 0 失败（可用 wechat_wcdb_last_error 取错误信息）
#[no_mangle]
pub extern "C" fn wechat_wcdb_open_account(
    path: *const c_char,
    key: *const c_char,
    out_handle: *mut i64,
) -> i32 {
    if path.is_null() || key.is_null() || out_handle.is_null() {
        set_error("空参数");
        return RC_ARG;
    }
    let path_str = unsafe { CStr::from_ptr(path) }.to_string_lossy();
    let key_str = unsafe { CStr::from_ptr(key) }.to_string_lossy();
    if !is_valid_key(key_str.as_ref()) {
        set_error("密钥格式非法，要求 64 位十六进制字符");
        return RC_ARG;
    }

    match open_account_inner(path_str.as_ref(), key_str.as_ref()) {
        Ok(account) => {
            unsafe { *out_handle = Box::into_raw(Box::new(account)) as i64 };
            RC_OK
        }
        Err(e) => {
            set_error(e);
            RC_OPEN
        }
    }
}

/// 关闭账号库句柄。
#[no_mangle]
pub extern "C" fn wechat_wcdb_close_account(handle: i64) -> i32 {
    if handle == 0 {
        return RC_ARG;
    }
    unsafe {
        drop(Box::from_raw(handle as *mut Account));
    }
    RC_OK
}

/// 获取全部会话列表 JSON（数组结构，每行一个会话对象，列原样透传）。
#[no_mangle]
pub extern "C" fn wechat_wcdb_get_sessions(handle: i64, out: *mut *mut c_char) -> i32 {
    let account = match account_ref(handle) {
        Some(a) => a,
        None => {
            set_error("句柄无效");
            return RC_ARG;
        }
    };
    let table = match &account.session_table {
        Some(t) => t.clone(),
        None => {
            write_out(out, "[]");
            return RC_OK;
        }
    };
    let conn = account.conn.lock().unwrap();
    let sql = format!("SELECT * FROM {}", quote_ident("", &table));
    let rows = match query_rows(&conn, &sql, &[]) {
        Ok(rows) => rows,
        Err(e) => {
            set_error(e);
            return RC_QUERY;
        }
    };
    let mut array = Vec::with_capacity(rows.len());
    for row in rows {
        let normalized = normalize_row(
            row,
            SESSION_ID_COLUMNS,
            "username",
            SESSION_NAME_COLUMNS,
            "display_name",
        );
        array.push(Value::Object(normalized));
    }
    write_out(out, &Value::Array(array).to_string());
    RC_OK
}

/// 分页获取指定会话的消息 JSON（跨 message_*.db 分片 UNION ALL，按时间升序）。
#[no_mangle]
pub extern "C" fn wechat_wcdb_get_messages(
    handle: i64,
    username: *const c_char,
    limit: i32,
    offset: i32,
    out: *mut *mut c_char,
) -> i32 {
    let account = match account_ref(handle) {
        Some(a) => a,
        None => {
            set_error("句柄无效");
            return RC_ARG;
        }
    };
    if username.is_null() {
        set_error("username 为空");
        return RC_ARG;
    }
    let talker = unsafe { CStr::from_ptr(username) }.to_string_lossy().to_string();
    let limit = if limit <= 0 { DEFAULT_LIMIT } else { limit as i64 };
    let offset = if offset < 0 { 0 } else { offset as i64 };

    let result = if account.msg_dbs.is_empty() {
        let conn = account.conn.lock().unwrap();
        query_messages_single(&conn, None, &talker, limit, offset)
    } else {
        let conn = account.conn.lock().unwrap();
        query_messages_sharded(&conn, &account.msg_dbs, &talker, limit, offset)
    };

    match result {
        Ok(rows) => {
            write_out(out, &Value::Array(rows).to_string());
            RC_OK
        }
        Err(e) => {
            set_error(e);
            RC_QUERY
        }
    }
}

/// 获取指定会话的消息总数（跨全部分片求和）。
#[no_mangle]
pub extern "C" fn wechat_wcdb_get_message_count(
    handle: i64,
    username: *const c_char,
    out_count: *mut i32,
) -> i32 {
    let account = match account_ref(handle) {
        Some(a) => a,
        None => {
            set_error("句柄无效");
            return RC_ARG;
        }
    };
    if username.is_null() || out_count.is_null() {
        set_error("空参数");
        return RC_ARG;
    }
    let talker = unsafe { CStr::from_ptr(username) }.to_string_lossy().to_string();

    let mut total: i64 = 0;
    let conn = account.conn.lock().unwrap();
    if account.msg_dbs.is_empty() {
        if let Some(table) = table_exists(&conn, None, MESSAGE_TABLES) {
            if let Ok(cols) = columns_of(&conn, None, &table) {
                if let Some(conv) = first_existing(&cols, CONV_COLUMNS) {
                    let sql = format!(
                        "SELECT count(*) FROM {} WHERE {} = ?1",
                        quote_ident("", &table),
                        quote_ident("", &conv)
                    );
                    total = conn
                        .query_row(&sql, rusqlite::params![&talker], |r| r.get::<_, i64>(0))
                        .unwrap_or(0);
                }
            }
        }
    } else {
        for db in &account.msg_dbs {
            let sql = format!(
                "SELECT count(*) FROM {}.{} WHERE {} = ?1",
                db.alias,
                quote_ident("", &db.table),
                quote_ident("", &db.conv_col)
            );
            total += conn
                .query_row(&sql, rusqlite::params![&talker], |r| r.get::<_, i64>(0))
                .unwrap_or(0);
        }
    }
    unsafe { *out_count = total.min(i32::MAX as i64) as i32 };
    RC_OK
}

/// 批量解析发送者显示名。
///
/// 入参为 JSON 字符串数组（也容忍对象数组，取 username/wxid/id 候选字段），
/// 返回 {标识: 显示名} 的 JSON 对象。
#[no_mangle]
pub extern "C" fn wechat_wcdb_get_display_names(
    handle: i64,
    json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    let account = match account_ref(handle) {
        Some(a) => a,
        None => {
            set_error("句柄无效");
            return RC_ARG;
        }
    };
    if json.is_null() {
        write_out(out, "{}");
        return RC_OK;
    }
    let input = unsafe { CStr::from_ptr(json) }.to_string_lossy();
    let wanted = parse_name_request(input.as_ref());

    let mut resolved: Map<String, Value> = Map::new();
    let conn = account.conn.lock().unwrap();
    for name_table in &account.name_tables {
        let pending: Vec<String> = wanted
            .iter()
            .filter(|id| !resolved.contains_key(*id))
            .cloned()
            .collect();
        if pending.is_empty() {
            break;
        }
        let table_ref = name_table
            .alias
            .as_ref()
            .map(|a| format!("{}.{}", a, quote_ident("", &name_table.table)))
            .unwrap_or_else(|| quote_ident("", &name_table.table));
        for chunk in pending.chunks(IN_BATCH) {
            let placeholders = vec!["?"; chunk.len()].join(",");
            let sql = format!(
                "SELECT {}, {} FROM {} WHERE {} IN ({}) AND {} IS NOT NULL AND {} <> ''",
                quote_ident("", &name_table.id_col),
                quote_ident("", &name_table.name_col),
                table_ref,
                quote_ident("", &name_table.id_col),
                placeholders,
                quote_ident("", &name_table.name_col),
                quote_ident("", &name_table.name_col)
            );
            let params: Vec<&dyn rusqlite::ToSql> =
                chunk.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
            if let Ok(mut stmt) = conn.prepare(&sql) {
                let mapper = |row: &rusqlite::Row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                };
                if let Ok(iter) = stmt.query_map(params.as_slice(), mapper) {
                    for pair in iter.flatten() {
                        resolved.entry(pair.0).or_insert(Value::String(pair.1));
                    }
                }
            }
        }
    }
    write_out(out, &Value::Object(resolved).to_string());
    RC_OK
}

/// 释放本库通过出参返回的字符串。
#[no_mangle]
pub extern "C" fn wechat_wcdb_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            drop(CString::from_raw(ptr));
        }
    }
}

/// 返回最近一次错误信息（线程局部，无需释放）。
#[no_mangle]
pub extern "C" fn wechat_wcdb_last_error() -> *const c_char {
    LAST_ERROR.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|c| c.as_ptr())
            .unwrap_or(std::ptr::null())
    })
}

/// 打开并解密账号库，发现 session/contact/message 分片结构。
fn open_account_inner(path: &str, key: &str) -> Result<Account, String> {
    let db_file = PathBuf::from(path);
    if !db_file.is_file() {
        return Err(format!("数据库文件不存在: {}", path));
    }
    let conn = open_sqlcipher(&file_uri(&db_file), key, &db_file)
        .map_err(|e| format!("打开 session.db 失败（密钥错误或非微信 4.x 库）: {}", e))?;

    // 目录结构：<storage>/session/session.db、<storage>/message/message_*.db、<storage>/contact/contact.db
    let session_dir = db_file.parent().unwrap_or_else(|| Path::new("."));
    let storage_dir = session_dir.parent().unwrap_or(session_dir);
    let message_dir = storage_dir.join("message");
    let contact_db = storage_dir.join("contact").join("contact.db");

    // 挂载消息分片（只读 ATTACH，失败的分片跳过）
    let mut shard_paths: Vec<PathBuf> = Vec::new();
    collect_message_shards(&message_dir, &mut shard_paths);
    if shard_paths.is_empty() {
        collect_message_shards(session_dir, &mut shard_paths);
    }
    shard_paths.sort();

    let mut msg_dbs: Vec<MsgDb> = Vec::new();
    for (index, shard) in shard_paths.iter().enumerate() {
        let alias = format!("m{}", index);
        if attach_sqlcipher(&conn, &alias, shard, key).is_err() {
            continue;
        }
        if let Some((table, columns, conv_col, time_col)) = discover_msg_db(&conn, &alias) {
            msg_dbs.push(MsgDb {
                alias,
                table,
                columns,
                conv_col,
                time_col,
            });
        }
    }

    // 挂载联系人库用于显示名解析
    let mut name_tables: Vec<NameTable> = Vec::new();
    if contact_db.is_file() && attach_sqlcipher(&conn, "c0", &contact_db, key).is_ok() {
        if let Some((table, cols)) = find_name_source(&conn, Some("c0"), CONTACT_TABLES) {
            if let Some(id_col) = first_existing(&cols, CONTACT_ID_COLUMNS) {
                for name_col in existing_in_order(&cols, CONTACT_NAME_COLUMNS) {
                    name_tables.push(NameTable {
                        alias: Some("c0".to_string()),
                        table: table.clone(),
                        id_col: id_col.clone(),
                        name_col,
                    });
                }
            }
        }
    }

    // 会话表：精确候选名优先，兜底扫描“同时含标识列与名称列”的表
    let session_table = table_exists(&conn, None, SESSION_TABLES).or_else(|| {
        list_tables(&conn, None).ok().and_then(|tables| {
            tables.into_iter().find(|t| {
                columns_of(&conn, None, t)
                    .map(|cols| {
                        cols.iter()
                            .any(|c| SESSION_ID_COLUMNS.contains(&c.to_lowercase().as_str()))
                            && cols
                                .iter()
                                .any(|c| SESSION_NAME_COLUMNS.contains(&c.to_lowercase().as_str()))
                    })
                    .unwrap_or(false)
            })
        })
    });

    // Session 表本身也作为名称兜底来源（低优先级）
    if let Some(table) = &session_table {
        if let Ok(cols) = columns_of(&conn, None, table) {
            if let Some(id_col) = first_existing(&cols, SESSION_ID_COLUMNS) {
                for name_col in existing_in_order(&cols, SESSION_NAME_COLUMNS) {
                    name_tables.push(NameTable {
                        alias: None,
                        table: table.clone(),
                        id_col: id_col.clone(),
                        name_col,
                    });
                }
            }
        }
    }

    Ok(Account {
        conn: Mutex::new(conn),
        session_table,
        msg_dbs,
        name_tables,
    })
}

/// 发现分片库中的消息表与会话归属列。
fn discover_msg_db(
    conn: &Connection,
    alias: &str,
) -> Option<(String, Vec<String>, String, Option<String>)> {
    let table = table_exists(conn, Some(alias), MESSAGE_TABLES)?;
    let columns = columns_of(conn, Some(alias), &table).ok()?;
    let conv_col = first_existing(&columns, CONV_COLUMNS)?;
    let time_col = first_existing(&columns, TIME_COLUMNS);
    Some((table, columns, conv_col, time_col))
}

/// 跨分片查询消息：列取全部分片交集，UNION ALL 后按时间排序分页。
fn query_messages_sharded(
    conn: &Connection,
    dbs: &[MsgDb],
    talker: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<Value>, String> {
    // 公共列交集（以首个分片的列序为准），保证 UNION ALL 列结构一致
    let common_cols: Vec<String> = dbs[0]
        .columns
        .iter()
        .filter(|c| {
            dbs.iter()
                .all(|db| db.columns.iter().any(|x| x.eq_ignore_ascii_case(c)))
        })
        .cloned()
        .collect();
    if common_cols.is_empty() {
        return Err("消息分片表结构无公共列".to_string());
    }
    let select_cols = common_cols
        .iter()
        .map(|c| quote_ident("", c))
        .collect::<Vec<_>>()
        .join(", ");

    let mut unions: Vec<String> = Vec::new();
    for db in dbs {
        unions.push(format!(
            "SELECT {} FROM {}.{} WHERE {} = ?",
            select_cols,
            db.alias,
            quote_ident("", &db.table),
            quote_ident("", &db.conv_col)
        ));
    }
    let order_col = dbs[0]
        .time_col
        .as_ref()
        .filter(|c| common_cols.iter().any(|x| x.eq_ignore_ascii_case(c)))
        .map(|c| quote_ident("", c));

    let sql = match &order_col {
        Some(col) => format!(
            "SELECT * FROM ({}) ORDER BY {} ASC LIMIT ? OFFSET ?",
            unions.join(" UNION ALL "),
            col
        ),
        None => format!(
            "SELECT * FROM ({}) LIMIT ? OFFSET ?",
            unions.join(" UNION ALL ")
        ),
    };

    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(dbs.len() + 2);
    for _ in 0..dbs.len() {
        params.push(&talker);
    }
    params.push(&limit);
    params.push(&offset);

    let rows = query_rows(conn, &sql, params.as_slice())?;
    Ok(rows.into_iter().map(normalize_message).collect())
}

/// 单库消息查询（主库兜底，无分片目录时使用）。
fn query_messages_single(
    conn: &Connection,
    alias: Option<&str>,
    talker: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<Value>, String> {
    let table = match table_exists(conn, alias, MESSAGE_TABLES) {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };
    let cols = columns_of(conn, alias, &table)?;
    let conv_col = match first_existing(&cols, CONV_COLUMNS) {
        Some(c) => c,
        None => return Ok(Vec::new()),
    };
    let table_ref = alias
        .map(|a| format!("{}.{}", a, quote_ident("", &table)))
        .unwrap_or_else(|| quote_ident("", &table));
    let order = first_existing(&cols, TIME_COLUMNS);
    let sql = match order {
        Some(col) => format!(
            "SELECT * FROM {} WHERE {} = ?1 ORDER BY {} ASC LIMIT ?2 OFFSET ?3",
            table_ref,
            quote_ident("", &conv_col),
            quote_ident("", &col)
        ),
        None => format!(
            "SELECT * FROM {} WHERE {} = ?1 LIMIT ?2 OFFSET ?3",
            table_ref,
            quote_ident("", &conv_col)
        ),
    };
    let rows = query_rows(conn, &sql, &[&talker, &limit, &offset])?;
    Ok(rows.into_iter().map(normalize_message).collect())
}

/// 行 JSON 规范化：注入 Java 侧约定的 canonical 字段名。
fn normalize_row(
    mut row: Map<String, Value>,
    id_keys: &[&str],
    id_target: &str,
    name_keys: &[&str],
    name_target: &str,
) -> Map<String, Value> {
    if !row.contains_key(id_target) {
        if let Some(v) = take_first(&mut row, id_keys) {
            row.insert(id_target.to_string(), v);
        }
    }
    if !row.contains_key(name_target) {
        if let Some(v) = take_first(&mut row, name_keys) {
            row.insert(name_target.to_string(), v);
        }
    }
    row
}

/// 消息行规范化：补齐 sender_username/local_type/create_time/message_content/compress_content。
fn normalize_message(row: Map<String, Value>) -> Value {
    let mut row = row;
    if !row.contains_key("sender_username") {
        if let Some(v) = take_first(&mut row, SENDER_COLUMNS) {
            row.insert("sender_username".to_string(), v);
        }
    }
    if !row.contains_key("local_type") {
        if let Some(v) = take_first(&mut row, TYPE_COLUMNS) {
            row.insert("local_type".to_string(), v);
        }
    }
    if !row.contains_key("create_time") {
        if let Some(v) = take_first(&mut row, TIME_COLUMNS) {
            row.insert("create_time".to_string(), v);
        }
    }
    if !row.contains_key("message_content") {
        if let Some(v) = take_first(&mut row, CONTENT_COLUMNS) {
            row.insert("message_content".to_string(), v);
        }
    }
    if !row.contains_key("compress_content") {
        if let Some(v) = take_first(&mut row, COMPRESS_COLUMNS) {
            row.insert("compress_content".to_string(), v);
        }
    }
    Value::Object(row)
}

/// 从行中取第一个存在且非空的候选字段值（先精确后忽略大小写）。
fn take_first(row: &mut Map<String, Value>, keys: &[&str]) -> Option<Value> {
    for key in keys {
        if let Some(value) = row.get(*key) {
            if !value.is_null() {
                return Some(value.clone());
            }
        }
    }
    let lower: Vec<(String, Value)> = row
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v.clone()))
        .collect();
    for key in keys {
        if let Some(value) = lower.iter().find(|(k, _)| k == *key).map(|(_, v)| v.clone()) {
            if !value.is_null() {
                return Some(value);
            }
        }
    }
    None
}

/// 解析显示名请求 JSON：字符串数组或对象数组。
fn parse_name_request(input: &str) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let parsed: Value = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(_) => return result,
    };
    if let Value::Array(items) = parsed {
        for item in items {
            match item {
                Value::String(s) => {
                    if !s.is_empty() {
                        result.push(s);
                    }
                }
                Value::Object(map) => {
                    for key in SENDER_COLUMNS.iter().chain(SESSION_ID_COLUMNS.iter()) {
                        if let Some(Value::String(s)) = map.get(*key) {
                            if !s.is_empty() {
                                result.push(s.clone());
                                break;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    result
}

/// 打开 SQLCipher 加密库（只读 URI），并执行微信 4.x 参数序列。
///
/// 微信 4.x 的密钥是 32 字节 raw key（64 位 hex）。SQLCipher 4.x 的二进制模式
/// 通过 `PRAGMA key = '0x<raw_key_hex>'` 触发（跳过 PBKDF2，直接用 32B 密钥），
/// salt 由 SQLCipher 自动从文件头读取并在 KDF 阶段处理，无需手动拼接。
///
/// 使用参数绑定避免 SQL 字符串字面量对 `x'...'` 的误解析。
fn open_sqlcipher(uri: &str, key: &str, db_path: &Path) -> Result<Connection, String> {
    let raw_key = normalize_key(key, db_path);
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;

    // 先设置 cipher 参数，再设 key（SQLCipher 4.x 中 PRAGMA key 必须在 cipher 参数后生效）
    conn.execute_batch(
        format!(
            "PRAGMA cipher_page_size = {};\
             PRAGMA kdf_iter = {};\
             PRAGMA cipher_hmac_algorithm = HMAC_SHA512;\
             PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;",
            CIPHER_PAGE_SIZE, KDF_ITER
        )
        .as_str(),
    )
    .map_err(|e| e.to_string())?;

    // 二进制模式：PRAGMA key = '0x<raw_key_hex>'（字符串字面量，SQLite PRAGMA 不支持参数绑定）
    // SQLCipher 4.x 检测到 0x 前缀即进入 binary mode（32B raw key）
    let key_literal = format!("'0x{}'", raw_key);
    conn.execute_batch(&format!("PRAGMA key = {}", key_literal))
        .map_err(|e| e.to_string())?;

    // 首次读访问触发密钥派生与 HMAC 校验，验证密钥正确性
    conn.query_row(
        "SELECT count(*) FROM sqlite_master",
        rusqlite::params![],
        |row| row.get::<_, i64>(0),
    )
    .map_err(|e| e.to_string())?;
    Ok(conn)
}

/// 在主连接上 ATTACH 一个加密库（只读 URI + 微信 4.x 密码参数）。
///
/// 密钥处理与 `open_sqlcipher` 一致：32 字节 raw key 走 SQLCipher 4.x
/// 二进制模式 `PRAGMA key = '0x<raw_key_hex>'`。
fn attach_sqlcipher(conn: &Connection, alias: &str, db_path: &Path, key: &str) -> Result<(), String> {
    let raw_key = normalize_key(key, db_path);
    let uri = file_uri(db_path).replace('\'', "''");
    conn.execute_batch(
        format!(
            "ATTACH DATABASE '{}' AS {};",
            uri, alias
        )
        .as_str(),
    )
    .map_err(|e| e.to_string())?;

    // 对 alias 设置 cipher 参数
    conn.execute_batch(
        format!(
            "PRAGMA {}.cipher_page_size = {};\
             PRAGMA {}.kdf_iter = {};\
             PRAGMA {}.cipher_hmac_algorithm = HMAC_SHA512;\
             PRAGMA {}.cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;",
            alias, CIPHER_PAGE_SIZE,
            alias, KDF_ITER,
            alias,
            alias
        )
        .as_str(),
    )
    .map_err(|e| e.to_string())?;

    // 二进制模式 key：PRAGMA alias.key = '0x<raw_key_hex>'（字符串字面量）
    let key_literal = format!("'0x{}'", raw_key);
    conn.execute_batch(&format!("PRAGMA {}.key = {}", alias, key_literal))
        .map_err(|e| e.to_string())?;

    conn.query_row(
        format!("SELECT count(*) FROM {}.sqlite_master", alias).as_str(),
        rusqlite::params![],
        |row| row.get::<_, i64>(0),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 在指定库中按候选名（小写）查找表。
fn table_exists(conn: &Connection, alias: Option<&str>, candidates: &[&str]) -> Option<String> {
    let tables = list_tables(conn, alias).ok()?;
    for candidate in candidates {
        if let Some(found) = tables.iter().find(|t| t.to_lowercase() == *candidate) {
            return Some(found.clone());
        }
    }
    None
}

/// 列出库内全部表与视图（排除 sqlite 内部表）。
fn list_tables(conn: &Connection, alias: Option<&str>) -> Result<Vec<String>, String> {
    let sql = match alias {
        Some(a) => format!(
            "SELECT name FROM {}.sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%'",
            a
        ),
        None => "SELECT name FROM sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%'"
            .to_string(),
    };
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

/// 取表的全部列名（PRAGMA table_info）。
fn columns_of(conn: &Connection, alias: Option<&str>, table: &str) -> Result<Vec<String>, String> {
    let sql = match alias {
        Some(a) => format!("PRAGMA {}.table_info({})", a, quote_ident("", table)),
        None => format!("PRAGMA table_info({})", quote_ident("", table)),
    };
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

/// 在列集合中按候选顺序找第一个存在的列。
fn first_existing(columns: &[String], candidates: &[&str]) -> Option<String> {
    for candidate in candidates {
        if let Some(found) = columns.iter().find(|c| c.eq_ignore_ascii_case(candidate)) {
            return Some(found.clone());
        }
    }
    None
}

/// 按候选优先级返回列集合中实际存在的列。
fn existing_in_order(columns: &[String], candidates: &[&str]) -> Vec<String> {
    let mut result = Vec::new();
    for candidate in candidates {
        if let Some(found) = columns.iter().find(|c| c.eq_ignore_ascii_case(candidate)) {
            result.push(found.clone());
        }
    }
    result
}

/// 查找联系人/名称来源表。
fn find_name_source(
    conn: &Connection,
    alias: Option<&str>,
    candidates: &[&str],
) -> Option<(String, Vec<String>)> {
    let table = table_exists(conn, alias, candidates)?;
    let columns = columns_of(conn, alias, &table).ok()?;
    Some((table, columns))
}

/// 收集 message 目录下的消息分片库（message_0.db ...，排除 wal/shm/journal）。
fn collect_message_shards(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let lower = name.to_lowercase();
        if !lower.ends_with(".db") || lower.contains('-') || !lower.starts_with("message") {
            continue;
        }
        out.push(path);
    }
}

/// 执行查询并把每行转为通用 JSON 对象（BLOB 输出为小写十六进制文本）。
fn query_rows(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Map<String, Value>>, String> {
    use rusqlite::types::ValueRef;

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let columns_count = columns.len();
    let mut result: Vec<Map<String, Value>> = Vec::new();

    let mapper = |row: &rusqlite::Row| -> rusqlite::Result<Map<String, Value>> {
        let mut map = Map::with_capacity(columns_count);
        for (index, name) in columns.iter().enumerate() {
            let value = match row.get_ref(index)? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(i) => Value::Number(i.into()),
                ValueRef::Real(f) => {
                    serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number)
                }
                ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
                ValueRef::Blob(b) => Value::String(hex_encode(b)),
            };
            map.insert(name.clone(), value);
        }
        Ok(map)
    };

    let iter = stmt.query_map(params, mapper).map_err(|e| e.to_string())?;
    for row in iter {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

/// 字节数组转小写十六进制字符串。
fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// 校验十六进制密钥：必须为 64 位 hex（32 字节 raw key）。
/// SQLCipher 4.x 二进制模式通过 `PRAGMA key = '0x<raw_key_hex>'` 触发，
/// salt 由库自动从 DB 文件头读取并在 KDF 阶段处理，无需手动拼接。
fn is_valid_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 归一化密钥为 64 位小写 hex（32 字节 raw key）。
/// 若传入 96 位 hex（raw key + salt）则取其前 64 位；
/// 若传入带 `0x` 前缀的字符串则去掉前缀。
fn normalize_key(key: &str, _db_path: &Path) -> String {
    let k = key.trim().to_lowercase();
    let k = k.strip_prefix("0x").unwrap_or(k.as_str()).to_string();
    if k.len() >= 64 {
        k[..64].to_string()
    } else {
        k
    }
}

/// SQL 标识符引用（转义双引号）；alias 为空时不加前缀。
fn quote_ident(alias: &str, ident: &str) -> String {
    let escaped = ident.replace('"', "\"\"");
    if alias.is_empty() {
        format!("\"{}\"", escaped)
    } else {
        format!("{}.\"{}\"", alias, escaped)
    }
}

/// 本地路径转 file: URI（强制只读模式，Windows 盘符与 \\?\ 前缀均已处理）。
fn file_uri(path: &Path) -> String {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut utf = abs.to_string_lossy().replace('\\', "/");
    // 去掉 Windows 规范化产生的 verbatim 前缀 \\?\
    if let Some(stripped) = utf.strip_prefix("//?/") {
        utf = stripped.to_string();
    }
    if !utf.starts_with('/') {
        // Windows 盘符路径 C:/... -> /C:/...
        utf = format!("/{}", utf);
    }
    let mut encoded = String::with_capacity(utf.len() + 16);
    for ch in utf.chars() {
        match ch {
            '%' => encoded.push_str("%25"),
            '#' => encoded.push_str("%23"),
            '?' => encoded.push_str("%3F"),
            ' ' => encoded.push_str("%20"),
            _ => encoded.push(ch),
        }
    }
    format!("file://{}?mode=ro", encoded)
}

/// 从句柄取账号引用。
fn account_ref(handle: i64) -> Option<&'static Account> {
    if handle == 0 {
        return None;
    }
    unsafe { (handle as *const Account).as_ref() }
}

/// 写出参 JSON 字符串（调用方须用 wechat_wcdb_free_string 释放）。
fn write_out(out: *mut *mut c_char, json: &str) {
    let cstring = CString::new(json).unwrap_or_else(|_| CString::new("[]").unwrap());
    unsafe { *out = cstring.into_raw() };
}
