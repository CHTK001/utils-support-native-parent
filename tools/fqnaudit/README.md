# fqnaudit — 内联全限定名（AGENTS.md 3.2）审计工具

用 javac 自己的语法树找内联全限定引用，而不是用文本正则。

## 为什么不能用正则

同一个 `utils-support-deeplearning-onnx-starter` 模块，本轮先后用三种正则统计过，
给出 **3 / 444 / 233** 三个互相矛盾的数字，**三次都是错的，而且错因各不相同**：

| 统计方式 | 报出 | 错因 |
|---|---|---|
| 按 `import` 扫，找未被 import 的 `com.chua.*` 引用 | 3 | 内联全限定名根本没有 import，检测器看不见 |
| 正则 `com\.chua\..*` 全文件匹配 | 444 | 把字符串字面量里的类名算进来 |
| 正则先剥字符串再匹配 | 233 | 正则剥不掉**折行**的字符串（`reg("x", "com.chua...", ai.djl...` 跨行） |

第一条错得最危险：它导致 `utils-support-native-datarecovery` 的
`utils-support-common-starter` 依赖被判成「死依赖」并删除，`clean compile`
立刻报 `找不到 com.chua.common.support.utils` —— 因为
`DataRecovery.java:48` 是用**内联全限定名**调 `NativeUtils.loadFromClasspath` 的。

## 为什么语法树能解决

AGENTS.md 3.2 明确豁免「文档 / 日志 / 异常信息里的类名文本 —— 不是类型引用，
是字符串内容」。而字符串字面量在 javac 语法树里是 `JCLiteral` 节点，
**结构上不可能**出现在 `JCFieldAccess`（成员选择链）的位置上。
因此 `reg("x", "com.chua.Foo", ...)` 里的字符串不需要任何特判就不会被命中。

判据是结构性的：

1. 解析目标源码，收集所有 `package` 声明，得到「真实存在的包名」及其全部点分前缀；
2. 并入一份**外部包根**白名单（`java` / `javax` / `com.fasterxml` / `ai.djl` …）——
   只靠第 1 步会漏掉 JDK 与第三方包的引用，因为这些包不会出现在被扫源码的
   `package` 声明里（实测 `java.nio.file.Files.*` 就被漏掉过）；
3. 遍历语法树里的 `JCFieldAccess`，若某条链以某个已知包前缀开头、且后面还有内容，
   就是一处全限定引用；
4. 跳过 `import`（import 正是避免全限定名的合规手段）与 `package` 声明本身；
   Javadoc 的 `{@link}` 不在方法体语法树里，本就不参与遍历。

## 用法

```bash
javac -encoding UTF-8 -d tools/fqnaudit/out tools/fqnaudit/FqnAudit.java
java --add-modules jdk.compiler -cp tools/fqnaudit/out FqnAudit <源码根目录> [--samples N]
```

## 验证（可复现）

工具不是「写完就用」，先对**人工确认过的真值**做对照：

- **必须命中**（真类型引用）：`DocOrientationTranslator.java:84/88`（`java.nio.file.Files`）、
  `C3DActionDetectionTranslator.java:113`（`NativeLoader`）、`GpuHelper.java:58/68`
  → 5/5 命中
- **不得命中**：`package` 声明（早期版本会把它算成 4 条 `1:9` 假命中，已修）
- **负例对照**：`utils-support-native-datarecovery` 在提交 `60b9417` 已修完 → 0 条
- **合成用例**（决定性）：一个类里同时写 4 处字符串形式的全限定名与 3 处代码形式的
  同名额 → 工具报 **3 条**，字符串的 4 处一条不计

`GROUND_TRUTH_FAILURES = 0`。

## 实测结果

`utils-support-deeplearning-parent/utils-support-deeplearning-onnx-starter`：
**1045 处 / 107 个文件**（占前三：`OnnxModelRegistrar.java` 493、
`PocketTtsTranslator.java` 87、`OnnxQwenTranslator.java` 23）。

## 已知局限

- 外部包根白名单需要人工维护。新增第三方库后若其包根不在列表里，该库的
  全限定引用会漏检。**宁可漏检也不要误报**：白名单是显式的，可以审。
- 只统计「以已知包前缀开头」的链。形如 `Foo.Bar`（Foo 是项目内短名类）不算全限定名，
  这是对的；但如果某处把包名写成了变量名，会漏检。
- 不做跨文件解析，因此不判断某个 `import` 是否真的存在 —— 工具报的是
  「这处引用写成了全限定形式」，这与「能否改用 import + 短名」等价，因为
  语法树里能写成点分链的包前缀必然是可 import 的。

---

# FqnFix —— 自动修复器

`FqnFix` 与 `FqnAudit` 共用同一套 AST 判定，在**同一批精确字符区间**上做替换。

```bash
javac -encoding UTF-8 -d tools/fqnaudit/out tools/fqnaudit/FqnAudit.java tools/fqnaudit/FqnFix.java
java --add-modules jdk.compiler -cp tools/fqnaudit/out FqnFix <源码根>          # 干跑，只报告
java --add-modules jdk.compiler -cp tools/fqnaudit/out FqnFix <源码根> --apply  # 应用
```

**为什么必须走 AST 区间而不是文本替换**：类名可能写在字符串里当查表键
（如 `reg("yolov8n", "com.chua...YoloV8nTranslator", ...)`），那是 AGENTS.md 豁免的
文本内容。文本替换会改坏它，而**改坏的字符串仍然编译通过**，只在运行期找不到类。

## 处理规则

| 情形 | 动作 |
|---|---|
| 同包引用 | 只去限定，不加 import |
| 跨包引用 | 替换为短名 + 补 import |
| 已 import 同类型 | 不重复加 |
| 同短名指向不同包（含文件内新引入的 import 之间） | 判为冲突，**整文件跳过**，绝不猜 |

## 使用记录：它在开发中自己犯过的错（都靠编译/干跑挡住）

1. 只比对「已有 import 的短名」存在性 → 把 172 处**已 import 的类型**误报为冲突
2. 判定「最长已知包前缀之后必须紧跟大写段」→ 当已知前缀短于真实包名时**漏报**，
   1045 处被漏成 115 处。改为「在链中找第一个大写开头的段作为类型名」
3. 把「包名」当成 import 目标 → 生成 `import com.a.b;` 这种**非法语句**，
   `clean compile` 立刻失败（已回滚修正）
4. 未排除已存在的 import → 产生 **34 条重复 import**（26 文件）

## 使用后的强制验证

1. **`clean compile`** —— 唯一的安全网。上述第 3 类错误只有编译能发现
2. **字符串字面量计数比对** —— 改坏的字符串**编译不会失败**，必须单独比对
   `"com.chua...Xxx"` 这类字面量的条数在 HEAD 与工作区是否一致
3. **`FqnAudit` 复扫** —— 应为 `FQN_HITS_TOTAL = 0`
4. **编码体检** —— 批量改写后必查 `BOM=0` / strict UTF-8 / `U+FFFD=0`，
   并**比对行尾是否被静默翻转**（翻转会把百行 diff 变成整文件 diff）

## 实战规模

首次应用：`deeplearning-onnx-starter`，**107 文件 / 1045 处**，
`clean compile` 280 源文件 BUILD SUCCESS，字符串字面量 225 处未动。
