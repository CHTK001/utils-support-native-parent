# 项目记忆

## 模块架构
- `utils-support-*`：基础工具层（common-starter, spring-starter, springboot-starter, osgi-starter 等）
- `spring-support-*`：Spring 业务层（common-starter, ai-starter, swagger-starter 等）
- 依赖方向：spring-support → utils-support（上层依赖底层）

## 三个模块重构（2026-07-28）
- **API 层**：从 `spring-support-common-starter` 迁移到 `utils-support-springboot-starter`（包路径 `com.chua.springboot.support.api`），旧路径保留向后兼容
- **ObjectContext**：`SpringBootObjectContext` 在 `utils-support-springboot-starter` 中，集成 OSGI 模块上下文，Bean 查找优先级 Spring → OSGI → 本地 → SPI
- **AI 全局注册**：`ChatClientRegistry` + `AgentRegistry` 在 `spring-support-ai-starter` 中，启动时 SPI 发现 ChatClient 并聚合为 AggregateChatClient

## Maven 构建
- 使用 `C:\Users\Administrator\.workbuddy\mvn_run.sh` 脚本（原生 java 调 Maven launcher）
- Java 版本：25（Amazon Corretto）

## CH 编码规范
- 中文详细注释、@author CH、控制语句必须大括号、Lombok 简化代码

## 前端 Monorepo 架构（2026-07-28）
- **根目录**：`vue-support-parent-starter/`（pnpm 9.15 + Turbo + Vue 3 + TypeScript + Vite）
- **命名规范**（统一后）：apps → `vue-support-xxx-starter`（无 scope 前缀）、packages → `@repo/xxx`、pages → `@pages/xxx`、layout → `@layout/xxx`
- **Sc* 组件注册**：统一由 `@repo/core/src/components/sc-components.ts` 管理，分三层（EAGER/COMMON/SCENE），`app-bootstrap.ts` 调用 `registerScComponents(app, { eager: true, common: true })`
- **全局组件**（仅 15 个）：ScButton/ScInput/ScIcon + IconifyIconOffline/Online/FontIcon/Auth + ScTable/ScTableColumn/ScForm/ScFormItem/ScSelect/ScSwitch/ScTag/ScDialog
- **场景组件**（32 个）：SCENE_COMPONENTS 不再全局注册，各页面通过 `scripts/fix-scene-imports.mjs --fix` 自动补充局部导入
- **冗余导入清理**：`scripts/clean-redundant-imports.mjs --fix` 清除已全局注册但仍局部导入的冗余行

## AI 模块架构（2026-07-28）
- **全局 ChatClient**：`ChatClientRegistry.getDefault()` 返回聚合 ChatClient，系统内部自用
- **分组 ChatClient**：外部 API 通过 `sys_ai_app_key_group_id` → `SysAiGroupService` → `ChatClientRegistry.getByModel(model)` 隔离
- **配置驱动**：`sys_ai_global_setting(runtime)` 表驱动 LLM 配置（provider/api_key/model/temperature 等 17+ 字段）
- **分组体系**：`sys_ai_group` + `sys_ai_group_model`（多对多），`sys_ai_app_key_group_id`/`sys_ai_token_group_id` 绑定分组
- **Token 体系**：`sys_ai_token`（SHA-256 哈希存储/sk-前缀/分组校验/限流/费用统计/允许模型列表）
- **RAG**：从 ai-starter stub 替换为 common-starter 的 `MemoryRagClient` + `RagDocumentLifeCycle`（SPI 加载）
- **Agent-as-Model**：`AgentModelDefinition` 包装 Agent → `ChatClientRegistry` 注册 → `/v1/models` 暴露
- **核心门面**：`AiChatFacadeServiceImpl`（全局/分组双路由），`AiOpenaiController` 注入 groupId

## DJL ONNX 兼容性注意事项（2026-07-28）
- **tokenizer.json 格式**: DJL 0.27.0 的 Rust tokenizers 库不支持新版格式
  - 移除字段: `fuse_unk`, `byte_fallback`, `ignore_merges`, `use_regex`
  - merges 格式: `list-of-strings`（`['ï ¼', ...]`），非 `list-of-lists`（`[['ï','¼'], ...]`）
- **DJL OnnxRuntime NDArray**: 不支持 `expandDims` → 直接创建 2D 数组 `long[][]`
- **Predictor 类**: 在 `ai.djl.inference.Predictor`，非 `ai.djl.translate.Predictor`
- **PyTorch 2.13 ONNX 导出**: `dynamo=False` 使用 legacy exporter，避免 `is_causal SymBool` 和 `DynamicCache` 问题
- **Docker 容器线程限制**: `can't start new thread` → monkey-patch `ThreadPoolExecutor.submit` 串行执行 + `pip --progress-bar off`
- **内置模型机制**: `ModelRegistry.resolveModelPath()` 从 JAR 解压 model.onnx 到 `%TEMP%/chua-dl-models/`；Translator 中 tokenizer 等附加文件用 `NativeLoader.basePath("models/xxx/").extractOnly(true).load()` 统一解压（复用 NativeLoader 的 MD5 校验 + 缓存 + file/jar 协议支持，不手写 resolveClasspathResource 调用）
- **MiniMind 内置模型**: `utils-support-minimind-onnx` 作为 onnx-starter 依赖，注册路径 `models/minimind/model.onnx`（classpath），无 downloadUrl