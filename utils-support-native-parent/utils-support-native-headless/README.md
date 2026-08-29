# utils-support-native-headless

预编译 native 二进制库。

## 平台支持

| 平台 | 状态 | 备注 |
|------|------|------|
| Windows x86_64 | ✅ 已预编译 | headless.dll |
| Linux x86_64 | ❌ 源码不可追溯 | 需外部提供源码 |

## 被谁使用

Java 项目通过 \System.loadLibrary()\ 加载。
