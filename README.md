# 爬虫中心 (Spider Center)

> 基于 `utils-support-flow-starter` + `utils-support-common-starter`（CronExpression）+ `spring-support-cache-starter` 的最小可用、运行可视化、可定时调度的爬虫中心。

---

## 特性

- **可视化编排**（`ReFlow` 基于 logicflow）—— 拖拽节点 / 连线 / 实时回放运行路径
- **7 个内置节点**：request / extract / transform / subLoop / map / output / condition
- **定时调度**：cron 表达式，每秒扫描，按需触发
- **全链路追踪**：traceId 从入口贯穿到 node 日志，按 executionNo 一键回放
- **SSE 实时推送**：运行中节点状态实时变色（呼吸光 + 徽章）
- **环境变量插值**：`{{env.COOKIE}}` 语法，节点配置可复用
- **缓存全表**：`@Cacheable` + Caffeine，写入清空策略
- **暗色 first**：devtools 风格，长时间盯屏不累

---

## 快速开始

### 后端

```bash
cd spring-support-parent-starter/spring-support-spider-starter
mvn install
# 起示例应用（待 D2 完成）
mvn -Dtest.skip=false test
```

依赖注入示例（待 D2 完成）：

```java
@SpringBootApplication
@Import(SpiderAutoConfiguration.class)
public class MyApp { }
```

### 前端

```bash
# 完整前端启动（待 D5 完成）
cd vue-support-parent-starter
pnpm dev --filter @apps/spider
# 访问 http://localhost:19092
```

### API 一览（待 D2 完成）

```
GET    /spider-api/spider/definitions             列表
POST   /spider-api/spider/definitions             新增
PUT    /spider-api/spider/definitions/{id}        更新
DELETE /spider-api/spider/definitions/{id}        删除
GET    /spider-api/spider/flows                   列表编排图
POST   /spider-api/spider/flows                   保存
GET    /spider-api/spider/data-records            列表采集数据
GET    /spider-api/spider/executions              列表执行
GET    /spider-api/spider/executions/{no}/logs    整链路日志
GET    /spider-api/spider/sse/execution/{no}      SSE 实时推送
GET    /spider-api/spider/env                     环境变量列表
PUT    /spider-api/spider/env                     环境变量更新
```

---

## 设计

详见 [`架构图.md`](./架构图.md)，包含：
1. 整体架构图
2. 数据流：用户新增 → 自动调度 → 数据入库
3. 节点 DAG 示例
4. SSE 实时链路
5. 缓存策略
6. traceId 全链路追踪
7. 环境变量插值
8. 包依赖关系
9. 部署视图

---

## 计划

详见 [`计划.md`](./计划.md)，分 D1~D10 渐进交付。

| Day | 内容 | 状态 |
|---|---|---|
| D1 | utils-support-flow-starter 加 traceId + MDC + FlowSnapshotStore + 节点 schema | ⬜ |
| D2 | spring-support-spider-starter 5 张表 + CRUD + 缓存 + TraceFilter | ⬜ |
| D3 | 7 个 spider 节点注册到 FlowNodeRegistry | ⬜ |
| D4 | SpiderScheduler + ExecutionService + 日志落库 | ⬜ |
| D5 | pages/spider/ 4 视图（ReTable + 基础表单，无 ReFlow） | ⬜ |
| D6 | ReFlow.vue 基础（logicflow 集成 + palette + 连线 + JSON） | ⬜ |
| D7 | ReFlow 配置面板（节点 schema 动态表单）+ 暗色主题 | ⬜ |
| D8 | SSE 实时（后端 SseEmitter + 前端 EventSource）+ 节点呼吸/徽章 | ⬜ |
| D9 | 全链路 traceId 视图（按 executionNo 串所有节点日志） | ⬜ |
| D10 | 收尾 + 真实 demo + 截图 + 推送 gitee | ⬜ |

---

## 变更记录

详见 [`CHANGELOG.md`](./CHANGELOG.md)

---

## 包结构

```
spring-support-parent-starter/
  spring-support-spider-starter/                  # D2 新建
    ├── entity/         5 个 entity
    ├── mapper/         MyBatis-Plus Mapper
    ├── service/        @Cacheable 缓存策略
    ├── node/           7 个 spider 节点
    ├── scheduler/      1s 轮询
    ├── execution/      FlowEngine 包装
    ├── trace/          SpiderTraceFilter
    ├── sse/            SpiderSseController
    └── config/         SpiderAutoConfiguration

utils-support-core-parent/
  utils-support-flow-starter/                     # D1 改造
    ├── FlowContext       + traceId
    ├── MDC 透出
    ├── FlowSnapshotStore 接口
    └── FlowNodeMetadata  + configSchema

vue-support-parent-starter/
  packages/ui/src/components/ReFlow/             # D6-D8 新建
    ├── ReFlow.vue        logicflow 集成
    ├── NodePalette.vue
    ├── NodeConfigPanel.vue
    └── Toolbar.vue

  pages/spider/                                  # D5-D9 新建
    ├── router/index.ts
    └── views/
        ├── DefinitionsView.vue
        ├── DesignerView.vue
        ├── DataRecordsView.vue
        └── ExecutionsView.vue
```

---

## 文档导航

| 文档 | 用途 |
|---|---|
| `计划.md` | 10 天实施计划，每阶段验证产物 |
| `架构图.md` | 9 张 Mermaid 图：架构 / 数据流 / 缓存 / traceId / SSE |
| `README.md` | 入口，特性 / 快速开始 / 设计 / 计划索引 |
| `CHANGELOG.md` | 变更记录，每天一行 |
