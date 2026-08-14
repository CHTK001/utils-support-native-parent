# utils-support-native-filesearch

跨平台快速文件搜索 Rust native 库（WizTree 能力）

---

## 快速开始

### 1. 添加依赖

```xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-filesearch</artifactId>
    <version>${project.version}</version>
</dependency>
```

---

## 构建

```bash
cd src/main/rust
./build.sh auto auto release
```

---

## 原生库导出函数

| 函数 | 说明 |
|------|------|
| `fast_search_by_name(root, pattern, max, callback)` | 按名称通配符搜索 |
| `fast_search_by_size(root, min, max, max_results, callback)` | 按大小范围搜索 |
| `fast_get_tree(root, max_depth, callback)` | 获取目录树 |
| `fast_get_version()` | 获取版本号 |
| `fast_search_cancel()` | 取消搜索 |
