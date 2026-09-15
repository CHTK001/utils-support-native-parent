import sqlcipher3
import os

db_path = "target/test-sqlcipher/session.db"
os.makedirs(os.path.dirname(db_path), exist_ok=True)
if os.path.exists(db_path):
    os.remove(db_path)

# 微信 4.x 参数（在连接后通过 PRAGMA key 设置）
# 用 0x 前缀表示 raw 二进制密钥，跳过 PBKDF2 派生，直接作为 32B 密钥（与 Rust 侧 96 位 raw key + salt 行为一致）
raw_key_hex = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6"
conn = sqlcipher3.connect(db_path)
cur = conn.cursor()
cur.execute("PRAGMA cipher_page_size = 4096")
cur.execute("PRAGMA kdf_iter = 256000")
cur.execute("PRAGMA cipher_hmac_algorithm = HMAC_SHA512")
cur.execute("PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512")
cur.execute("PRAGMA key = '0x%s'" % raw_key_hex)

cur.execute("""CREATE TABLE session(
    username TEXT PRIMARY KEY,
    display_name TEXT,
    last_msg_time INTEGER
)""")
cur.execute("INSERT INTO session VALUES ('wxid_test_001','测试会话甲',1700000000000)")
cur.execute("INSERT INTO session VALUES ('wxid_test_002','测试会话乙',1700000001000)")
conn.commit()

# 校验可读
cur.execute("SELECT * FROM session")
rows = cur.fetchall()
print("已写入 %d 行:" % len(rows))
for r in rows:
    print(" ", r)

conn.close()

# 输出文件头 salt（前 16 字节）
with open(db_path, "rb") as f:
    salt = f.read(16)
print("salt(hex):", salt.hex())
print("raw key(hex):", raw_key_hex)
print("full key(hex):", raw_key_hex + salt.hex())
print("DB path:", os.path.abspath(db_path))
