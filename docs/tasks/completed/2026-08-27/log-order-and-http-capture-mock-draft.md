# 日志倒序与 HTTP 抓包响应生成 Mock 规则草稿

## 任务信息

- 任务 ID：`TASK-20260827-002`
- 状态：`已完成`
- 任务日期：`2026-08-27`
- 创建时间：`2026-08-27 12:39:56 +08:00`
- 开始时间：`2026-08-27 12:55:00 +08:00`
- 最后更新时间：`2026-08-27 14:50:20 +08:00`
- 完成时间：`2026-08-27 14:50:20 +08:00`
- 创建路径：`docs/tasks/pending/2026-08-27/log-order-and-http-capture-mock-draft.md`
- 归档路径：`docs/tasks/completed/2026-08-27/log-order-and-http-capture-mock-draft.md`
- 关键词：`diagnostic logs`、`newest first`、`ExchangeObservation`、`HTTP capture`、`MockResponse`、`rule draft`
- 任务优先级：`高`
- 优先级理由：日志排序本身风险低，但从抓包响应生成规则草稿涉及 Exchange 配对、HTTP Header 与 Body 合法性、公共命令合同和 Application/Domain 依赖边界，不能仅作为前端拼装处理。

## 背景与目标

用户追加两个小需求：

1. 诊断日志默认按时间倒序展示，最新记录位于最上方。
2. HTTP 抓包页面可使用当前服务器响应生成一个可编辑的本地 Mock 规则草稿。

目标是在不改变抓包原始事实、不自动保存或启用规则的前提下，减少查找最新日志和从真实响应手工录入
Mock 规则的操作成本。

## 范围

- 诊断日志默认排序为 `occurred_at` 降序；时间相同时使用稳定的事件标识降序，避免分页或刷新时抖动。
- 在一条具有完整、可配对 HTTP 响应的 ExchangeObservation 上提供“创建 Mock 规则草稿”入口。
- 生成一个普通、可编辑、未保存、未启用的规则草稿，并打开现有规则编辑器。
- 第一版使用完整 request-target 精确匹配。
- 复制服务器响应状态码、允许保留的响应 Header 和可无损表示的文本 Body。
- Rust/Application 负责请求响应配对、资格判定、Header 过滤、`Content-Length` 重算和规则合法性；前端只触发命令并展示草稿。
- 二进制、压缩或非 UTF-8 响应明确拒绝并展示稳定原因。

## 不在范围

- 规则模板库、模板持久化、批量生成或历史模板管理。
- 自动保存、自动启用、自动应用到 Listener。
- 第一版的 method + target 组合匹配或模糊匹配。
- 在前端复制 Rust 领域规则、解析原始报文或自行过滤 Header。
- 修改 HTTP 抓包、代理转发或响应内容本身。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-08-27` | 用户要求日志最新记录显示在最上方。 |
| `2026-08-27` | 用户要求从当前 HTTP 抓包的服务器响应创建规则模板；结合既有已确认需求，产物定义为未保存的普通 Mock 规则草稿。 |
| `2026-08-27` | 前端负责入口和编辑器跳转；协议配对、Header/Body 安全处理与规则合法性由 Rust 负责，避免双实现。 |

## 未确认事项

- 无会改变第一版实现方向的事项。日志指当前诊断日志页面；Mock 草稿沿用既有规则编辑器和当前 Workspace/Listener 上下文。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`
- 范围与不在范围：`PASS`
- 输入、输出与状态变化：`PASS`
- 错误行为：`PASS`，不完整或不可安全转换的响应必须拒绝，不得生成默认成功草稿。
- 具体示例：`PASS`，完整 UTF-8 JSON 响应生成未保存 Mock 草稿；gzip/二进制响应被拒绝并说明原因。
- 可重复 PASS/FAIL 验收：`PASS`
- 改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-08-27 12:55:00 +08:00`。

## 当前事实与根因边界

### 当前已验证

- Application 诊断查询已有结果反转逻辑，需要进一步确认数据库排序、分页和前端是否再次排序。
- 当前存在 `rule_create_from_session` 路径，但只从旧会话语义创建基础匹配条件，不能复制当前 ExchangeObservation 的服务器响应。
- 当前抓包模型已经使用 ExchangeObservation，响应配对与规则合法性不应由前端推断。

### 推断

- 日志若仅在前端反转当前页，会造成分页后的整体时间顺序错误；应先确认权威查询顺序，再决定最小修改点。
- 若前端直接从展示字段拼装 MockResponse，会漏掉 hop-by-hop Header、压缩/编码、Content-Length 和完整配对约束。

### 未知

- 当前诊断查询在所有存储实现上的原始顺序与分页游标合同，需在实现前用现有测试确认。

### 正确修复边界

- 日志：在产生分页结果的权威边界保证稳定倒序，前端只按返回顺序展示。
- 抓包转规则：新增或扩展 Application 用例消费完整 ExchangeObservation，构造合法 Draft；前端不复制协议/领域逻辑。

## 最小改动与最优设计比较

| 方案 | 分析 |
| --- | --- |
| 纯前端修改 | 日志当前页反转简单，但可能破坏分页全局顺序；抓包响应在前端拼规则会复制协议和领域规则，存在错误 Header/Body 与非法规则风险，淘汰。 |
| 最小正确实现 | 日志在现有权威查询边界锁定倒序；复用 ExchangeObservation 与现有规则编辑器，在 Application 增加单一“响应生成未保存 Mock 草稿”用例。无模板库、无自动保存，采用。 |
| 扩展模板系统 | 新建可复用模板持久化、管理和批量应用能力，超出需求且增加新权威来源，淘汰。 |

## 小任务列表

| ID | 任务 | 依赖 | 可并行 | 状态 | 验收 |
| --- | --- | --- | --- | --- | --- |
| LHM-01 | 锁定诊断日志权威排序与分页行为 | 无 | 是 | 已完成 | 最新时间及稳定同秒顺序测试通过 |
| LHM-02 | 锁定 ExchangeObservation → Mock Draft 合同 | 无 | 是 | 已完成 | 完整响应正例与不完整/二进制/压缩负例通过 |
| LHM-03 | 实现 Application 用例和 Tauri 命令 | LHM-02 | 否 | 已完成 | Rust 构造合法未保存草稿且不持久化 |
| LHM-04 | 前端入口、编辑器跳转和错误展示 | LHM-01, LHM-03 | 否 | 已完成 | UI 展示倒序并从当前响应打开草稿 |
| LHM-05 | 整体验证、文档同步和对抗审查 | LHM-01..04 | 否 | 已完成 | 受影响 Rust/前端测试及审查通过 |

## 测试计划

- 日志查询：多时间点、相同时间稳定 tie-break、刷新、分页边界与空列表。
- Application：完整 UTF-8 响应生成精确 request-target + MockResponse 草稿；不保存、不启用。
- Header：过滤 hop-by-hop/transport Header，重算或删除旧 `Content-Length`，保留允许的重复 Header 语义。
- Body：UTF-8 文本和空 Body 正例；二进制、压缩、非 UTF-8、不完整配对负例。
- UI：按钮资格、命令参数、成功打开编辑器、失败提示、未保存状态和日志倒序展示。
- 回归：现有规则创建、抓包展示、代理流量和日志查询行为不被改变。
- 静态门禁：Rust fmt/clippy/tests、前端 typecheck/tests、architecture/source-size 和文档链接。

## 对抗审查计划

- 检查前端是否复制协议/领域判断、草稿是否被自动保存/启用、错误响应是否被误当完整响应。
- 检查 Header 注入、hop-by-hop 字段、压缩/编码、Content-Length、重复 Header 和非 UTF-8 泄漏。
- 检查分页倒序是否只是当前页反转、相同时间是否不稳定。
- 完成前整体审查必须为 `APPROVE/CLEAR`。

## 文档影响

- `docs/user-operation-guide.md`
- 必要的 Exchange/规则架构文档
- 本任务文档与测试记录

测试记录不得包含 Git、提交记录、HEAD、哈希、暂存或未跟踪状态信息。

## 实施记录

- `2026-08-27 12:39:56 +08:00`：登记用户追加需求；复用既有已确认的抓包响应生成 Mock 草稿边界，明确其不是纯前端实现。
- `2026-08-27 14:50:20 +08:00`：完成日志稳定倒序、HTTP Exchange 响应资格判定、MockResponse 草稿、Tauri 命令和 UI 跳转；独立审查发现并补齐 `Proxy-Connection` 过滤，所有回归重新通过。

## 修改文件

- `src-tauri/crates/application/src/facade/diagnostics.rs` 与诊断排序回归。
- Exchange HTTP UTF-8 证据投影、`facade/rules/exchange_mock.rs`、Tauri rules command 与生成绑定。
- `src/features/capture/**`、`src/features/rules/**` 的草稿跳转、异步保护和测试。
- 用户操作说明、MCP 验证指南与场景复跑记录。

## 附加文件

- `.omx/context/http-history-local-mock-template-20260825T124357Z.md`
- `.omx/interviews/http-history-local-mock-template-20260825T125336Z.md`
- `.omx/specs/deep-interview-http-history-local-mock-template.md`

## 验收结果

- `PASS`。日志由权威 Application 查询按 `occurred_at`、`event_id` 稳定倒序；完整服务器响应可生成未保存、禁用的 Mock 草稿，不安全或不完整响应 fail-closed。

## 测试结果

- Application Mock 草稿 `3/3 PASS`；HTTP hop-by-hop、`Connection` nominated 与混合大小写 `Proxy-Connection` 回归通过。
- 前端抓包/规则聚焦 `14/14 PASS`；前端全量 `67` 个文件、`659` 项 PASS。
- Rust workspace 全量、严格 Clippy、格式、架构、源码规模和 Windows 静态编译检查 PASS。

## CI 情况

- 用户授权的远程 Windows 验证由最终交付任务统一执行；本任务只记录本地实现与回归结果。

## 完成总结

- 完成“最新日志在上”和“当前服务器 HTTP 响应生成 Mock 草稿”。草稿不会自动保存或启用，Rust 负责配对、编码与 Header 安全，前端只负责触发和打开编辑器。
