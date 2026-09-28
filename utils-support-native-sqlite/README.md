# utils-support-native-sqlite

SQLite update_hook 原生动态库。
        封装 sqlite3_update_hook() 回调机制，以环形缓冲区 + JSON 事件格式暴露给 Java FFI 侧消费。

---

## 快速开始

### 1. 添加依赖

```xml
<dependency>
    <groupId>com.chua</groupId>
    <artifactId>utils-support-native-sqlite</artifactId>
    <version>${project.version}</version>
</dependency>
```

---

## 配置说明

本模块为零配置模块，引入依赖后即可使用。

---

## 依赖关系

```
utils-support-native-sqlite
└── (无内部依赖)
```