# 变更记录 (Changelog)

> 爬虫中心重构进度跟踪。每完成一日任务追加一条。

---

## 2026-08-XX · D0 · 重构启动

### 删除

- 删除 `vue-support-parent-starter/apps/vue-support-spider-starter/`（旧 vue 爬虫前端）
- 删除 `vue-support-parent-starter/pages/spider/`（旧 pages 包）
- 删除 `spring-support-parent-starter/spring-support-spider-starter/`（旧 spring 爬虫后端）
- 清理根目录与 temp 目录下的 spider 临时文件 / 日志 / vbs / bat / 测试脚本

### 文档

- 新建 `计划.md` —— 10 天实施计划（D1~D10）
- 新建 `架构图.md` —— 9 张 Mermaid 图
- 新建 `README.md` —— 项目入口文档
- 新建 `CHANGELOG.md` —— 本文件

### 设计系统锁定

- 暗色 first（开发工具型）
- 配色：`#0B0F17` 页面 / `#111827` 卡片 / `#3B82F6` 主色 / `#10B981` 成功 / `#EF4444` 失败 / `#06B6D4` running（带 box-shadow 呼吸）
- 字体：Inter / JetBrains Mono
- 间距：4 / 8 / 12 / 16 / 24 / 32
- 圆角：6 / 8 / 12
- 全 SVG（Lucide/Heroicons），禁用 emoji
- `cursor-pointer` + 150-250ms 微交互
- running 节点 1.5s linear infinite alternate opacity 动画
- ReFlow 画布 dot-grid 背景 `#0F172A`

### 决策确定

- 包结构：`spring-support-spider-starter`（新）+ `utils-support-flow-starter`（改造）+ 新前端 `pages/spider/`
- 缓存策略：`@Cacheable` 全表 enabled definitions，写入 `allEntries=true` 清空
- 调度器：`ScheduledExecutorService` 每秒 + `CronExpression.getNextValidTimeAfter(lastRun)` 比对
- 节点注册式：spider 包新增 7 个节点 `@PostConstruct` 注册到 `FlowNodeRegistry`
- 全链路：`SpiderTraceFilter` 生成 traceId → MDC 贯穿 → `spider_execution_log.trace_id` → 前端详情时间线
- SSE：`SseEmitter` 推送节点状态，前端 EventSource 订阅
- ReFlow 组件：基于 `@logicflow/core`，无现成就写

### 已知技术债 / 风险

- logicflow 与 Vue 3.5 + Vite 8 兼容性待 D6 验证
- SpiderScheduler 1 秒扫的性能待 D9/D10 压测
- SpiderFlowExecutionService 与 `utils-support-flow-starter` 的 traceId 字段命名用 `flowTraceId` 避免冲突
- MyBatis-Plus JSON 字段需自定义 TypeHandler

---

## 模板（参考格式，后续每一天如此追加）

```markdown
## 2026-08-XX · D<n> · <主题>

### 新增
- ...

### 修改
- ...

### 删除
- ...

### 验证
- [ ] 后端跑通 / 测试通过
- [ ] 浏览器真跑截图

### 已知问题
- ...
```
