import sqlcipher3
import os

db_path = "target/test-sqlcipher/session.db"
raw_key_hex = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6"

# 读文件头 salt
with open(db_path, "rb") as f:
    salt = f.read(16).hex()
print("salt:", salt)
full_key = raw_key_hex + salt
print("full key:", full_key)

# 尝试 1：96 位 raw key（0x 前缀，跳过 KDF）
print("\n--- 尝试 1: 96 位 0x 前缀（raw key + salt）---")
try:
    conn = sqlcipher3.connect(db_path)
    cur = conn.cursor()
    cur.execute("PRAGMA key = '0x%s'" % full_key)
    cur.execute("SELECT * FROM session")
    rows = cur.fetchall()
    print("成功! 行数:", len(rows))
    for r in rows:
        print(" ", r)
    conn.close()
except Exception as e:
    print("失败:", e)

# 尝试 2：32 位 raw key（0x 前缀，sqlcipher 自动从文件头读 salt）
print("\n--- 尝试 2: 32 位 0x 前缀（仅 raw key，salt 自动）---")
try:
    conn = sqlcipher3.connect(db_path)
    cur = conn.cursor()
    cur.execute("PRAGMA key = '0x%s'" % raw_key_hex)
    cur.execute("SELECT * FROM session")
    rows = cur.fetchall()
    print("成功! 行数:", len(rows))
    for r in rows:
        print(" ", r)
    conn.close()
except Exception as e:
    print("失败:", e)

# 尝试 3：32 位明文密码（走 PBKDF2 派生）
print("\n--- 尝试 3: 32 位明文密码（PBKDF2 派生）---")
try:
    conn = sqlcipher3.connect(db_path)
    cur = conn.cursor()
    cur.execute("PRAGMA key = '%s'" % raw_key_hex)
    cur.execute("SELECT * FROM session")
    rows = cur.fetchall()
    print("成功! 行数:", len(rows))
    for r in rows:
        print(" ", r)
    conn.close()
except Exception as e:
    print("失败:", e)
