# AGENTS.md — utils-support-native-parent

> **模块边界与依赖铁律：[`../规则.md`](../规则.md)（`D:\ch\project\规则.md`），动手前必须先读并遵守。违反 R1/R2 = S0 严重 bug。**
> 全局规则：`C:\Users\Administrator\.config\opencode\AGENTS.md`；源码编辑红线：`C:\Users\Administrator\.agents\GLOBAL-SOURCE-EDITING-RULES.md`。

## 本仓库定位

预训练 native 二进制库（Rust cdylib），提供 Java 侧字符串/编解码等底层能力。**不含业务规则。**

## 三条铁律（摘要，全文以 `规则.md` 为准）

1. **R1 禁止业务穿插**：本仓库不得实现任何业务 starter 的规则；上层也不得在 Java 侧重写同一份原生能力。
2. **R2 禁止穿透持久层**：跨模块取数只走契约。
3. **R4 缺能力就下沉**：平台相关的底层实现下沉到这里，并保持 Java 门面唯一。

## 交付闸门

`mvn -o -q compile -DskipTests` EXIT=0；编码 `BOM=0 / U+FFFD=0`；产物坐标与平台标签不得随意更改（上层按坐标+classifier 加载）。

## 原生产物与 CI（`native-*.yml`）

- 四平台产物一律以 CI 构建为准：工作流在对应 runner 上 build → 架构/导出符号校验 → 运行时冒烟 → 回填分支 `build/<模块>-native-artifacts`（`git push --force`，**不改 main**）；main 上的 `src/main/resources/native/` 由人工从该分支 checkout 后提交。
- **不要用字节/hash 比对判断「产物是否有更新」**：lld 用真实时钟写链接时间戳，同源码重编必然字节不同。实测两次 CI 的 Windows DLL 同尺寸、仅 24 字节不同 = COFF `TimeDateStamp`（+ 数据段内嵌 3 处副本）与 CodeView `RSDS` 的 PDB GUID，功能等价。判定「有没有真变化」看源提交与架构/导出符号断言，不看 hash。
- 冒烟步骤由矩阵 `smoke:` 控制：arm64 runner 无法 dlopen x86_64 dylib，`darwin-x86_64` 腿跳过冒烟属设计内，不是漏测。

## 提交

只提交自己的文件；产物变更必须标注需要同步发布的上层模块。
