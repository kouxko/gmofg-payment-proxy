# 修复运行中 App 重放发现的 Mock 草稿与入口刷新问题

## 任务信息

- 任务 ID：`TASK-20260828-003`
- 状态：`已完成`
- 任务日期：`2026-08-28`
- 创建时间：`2026-08-28 10:21:37 +08:00`
- 开始时间：`2026-08-28 10:21:37 +08:00`
- 最后更新时间：`2026-08-28 12:38:59 +08:00`
- 完成时间：`2026-08-28 12:38:59 +08:00`
- 创建路径：`docs/tasks/pending/2026-08-28/fix-running-app-replay-findings.md`
- 归档路径：`docs/tasks/completed/2026-08-28/fix-running-app-replay-findings.md`
- 关键词：`HTTP Mock draft`、`Content-Length`、`Workspace refresh`、`listener catalog`、`toolbar`、`MCP environment apply`
- 任务优先级：`高`
- 优先级理由：涉及真实 HTTP 报文到规则公共合同、Application/Domain 边界，以及 MCP 原子应用后的跨层 UI 状态一致性；失败会使已声明功能不可用并让界面展示错误权威状态。

## 背景与目标

`TASK-20260828-001 / RUNNING-APP-REPLAY-001` 在用户已启动的 Release App 中复现两个问题：

1. 完整服务器 HTTP 响应点击“用此服务器响应创建 Mock 草稿”后，草稿校验失败。
2. MCP 环境候选增加或删除 Listener 后，代理入口页和顶栏不会及时刷新，显示与权威 Workspace 不一致。

目标是修复这两个真实运行路径，保留现有安全 Header、Workspace 原子应用和 Listener 生命周期合同，并用自动化回归及重新构建后的真实 App 重放证明修复。

## 范围

- 让正常含正文和 `Content-Length` 的完整服务器响应能够生成未保存、禁用、绑定当前 HTTP Listener 的 Mock 草稿。
- 保持 hop-by-hop Header、连接声明 Header、压缩正文、非 UTF-8 与不完整响应继续 fail-closed。
- 让 MCP/导入等外部路径提交当前 Workspace 后，入口列表、顶栏计数和相关创建能力自动刷新。
- 增加 Rust 与前端失败回归，执行受影响全量测试，并重新构建 App 重放真实 HTTP/Socket 模拟 Server 场景。

## 不在范围

- 不放宽 Domain 对用户规则直接设置托管 Header 的禁止。
- 不自动保存或启用生成的 Mock 草稿。
- 不增加轮询、静默重试、双缓存权威或页面切换兜底。
- 不改变 MCP 环境候选、Workspace revision、Listener 启停或数据库事务语义。
- 不并入 `TASK-20260828-002` 的 Socket 连接状态文案任务。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-08-28 10:19:58 +08:00` | 用户要求修复实际测试出现的问题，即本次运行中 App 重放记录的两个 FAIL。 |

## 未确认事项

无；两个失败已有真实 App 复现、稳定输入、实际输出和明确期望。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`
- 范围与不在范围：`PASS`
- 输入、输出与状态变化：`PASS`
- 错误行为：`PASS`，安全 Header 与不安全响应继续拒绝；刷新失败不得伪造空状态或成功。
- 具体示例：`PASS`，201 JSON 响应包含 `Content-Length` 时生成禁用 Mock 草稿；MCP 从 0→2→0 Listener 后 UI 无需切页即显示对应数量。
- 可重复 PASS/FAIL 验收：`PASS`
- 改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-08-28 10:21:37 +08:00`

## 问题与根因分析

### 问题一：Mock 草稿 Header 合同冲突

- 实际：真实 App 提示“该 Header 由 Rust 转发管线统一管理，规则不得直接设置”，草稿未创建。
- 预期：完整 UTF-8 服务器响应生成未保存、禁用 Mock 草稿，正文长度由运行管线权威管理。
- 当前已验证：Application `response_metadata` 删除响应原始 `content-length` 后重新加入计算值；随后 Domain 对规则 Header 禁止 `content-length`，同一内部生成草稿被拒绝。
- 根因：生成草稿错误地把运行管线负责的派生 Header 当作规则用户数据保存，形成内部生产者与 Domain 安全约束冲突。
- 正确边界：草稿不携带 `content-length`，运行时发送 MockResponse 时继续按最终正文计算长度；Domain 禁止规则直接设置托管 Header 的合同不变。

### 问题二：Workspace 外部变更后的 UI 缓存失效缺失

- 实际：MCP 提交 Listener 后，当前代理入口页仍显示空；切页才加载新列表。恢复为空后列表也需切页才清空，顶栏仍显示旧的入口数量。
- 预期：当前 Workspace 发生已提交变化时，入口列表、顶栏与规则创建能力从同一事件失效并重新读取权威数据。
- 当前已验证：后端持久 Workspace 已正确变为 0→2→0，运行状态也正确；错误只存在于前端展示刷新链路。
- 候选原因：Workspace 提交事件没有统一失效入口 catalog、toolbar overview 和相关 navigation query，或监听者只覆盖本页内部保存路径。
- 根因确认方式：先用前端集成测试模拟外部 `workspace_changed`/当前应用完成事件，证明查询未刷新；再沿实际事件入口修复单一 invalidation 边界。

## 最小改动与最优设计比较

| 方案 | 分析 |
| --- | --- |
| Mock 草稿允许 `content-length` 通过 Domain | diff 小但破坏托管 Header 安全合同，拒绝。 |
| Mock 草稿不保存派生长度 Header | 运行管线继续权威计算长度，无双实现和安全放宽，采用。 |
| UI 增加轮询或切页后刷新 | 掩盖事件缺失并增加延迟/重复请求，拒绝。 |
| 在现有 Workspace 提交事件边界统一失效受影响查询 | 维持后端单一权威，当前页面和顶栏立即一致，采用。 |

## 小任务列表

| ID | 任务 | 依赖 | 状态 | 验收 |
| --- | --- | --- | --- | --- |
| RRF-01 | 增加 Mock 草稿真实 `Content-Length` 失败回归并修复 | 无 | 已完成 | 草稿生成通过，托管 Header 不进入规则，负例不回退 |
| RRF-02 | 增加外部 Workspace 变化 UI 刷新失败回归并修复 | 无 | 已完成 | 0→2→0 无需切页，入口列表、顶栏和能力一致 |
| RRF-03 | 受影响全量、静态门禁与真实 App 重放 | RRF-01..02 | 已完成 | 自动化全绿；运行中 App HTTP/Socket 与 Workspace 刷新重放通过 |
| RRF-04 | 对抗审查、归档和完成索引 | RRF-03 | 已完成 | 无 P0/P1/P2，证据与任务状态一致 |

## 测试计划

- Application Mock 草稿：真实响应 Header 含 `Content-Length`、`Connection`、`Proxy-Connection` 与普通自定义 Header；断言草稿生成、禁用、正文保持，托管 Header 被排除。
- Domain：用户规则直接设置 `content-length` 继续拒绝。
- Frontend：模拟当前 Workspace 在页面打开期间外部提交 0→2→0 Listener；断言入口页、顶栏和规则能力自动更新，迟到响应不回滚。
- 完成后运行受影响 Rust/Frontend 全量、类型检查、严格 Clippy、格式、架构和 source-size。
- 构建新的 Release App，重复 `RUNNING-APP-REPLAY-001` 并实际点击 Mock 草稿入口。

## 对抗审查计划

- 检查是否通过放宽托管 Header、删除安全校验或双重长度计算掩盖问题。
- 检查刷新是否依赖页面切换、定时器、额外轮询或错误状态伪装。
- 检查外部 Workspace 事件是否会让非当前 Workspace、迟到请求或运行中 Listener 错误覆盖当前界面。

## 文档影响

- 更新本任务、派生测试证据和必要的运行验证说明；产品功能文档仅在现有描述与实际合同不一致时修改。

## 实施记录

- `2026-08-28 10:21:37 +08:00`：登记两个真实 App FAIL 的修复任务；确认先写失败回归，再修改生产代码。
- `2026-08-28 12:38:59 +08:00`：Mock 草稿移除托管长度 Header；Application commit 成功后发布 `snapshot_required`；前端统一失效查询代次并刷新集合与当前 Workspace。自动化、Release 构建、运行中 App 数据平面和 Workspace 刷新重放通过，独立审查无 P0/P1/P2。

## 修改文件

- Application Mock 草稿 facade 与回归测试。
- Application Environment Apply worker、事件回归和 MCP 真实链路测试。
- 前端 Workspace 查询失效 Hook、App 顶栏、Listener、Rules 接线及相邻回归测试。

## 附加文件

- 父任务：`TASK-20260828-001`
- 父用例：`RUNNING-APP-REPLAY-001`
- 父证据：`docs/testing/evidence/2026-08-28/TASK-20260828-001/RUNNING-APP-REPLAY-001/`
- [RUNNING-APP-FINDINGS-REGRESSION-001](../../../testing/evidence/2026-08-28/TASK-20260828-003/RUNNING-APP-FINDINGS-REGRESSION-001/)

## 验收结果

- `PASS_WITH_NOT_RUN`：两个生产根因均已修复；自动化与运行中 App HTTP/Socket、连接状态和 Workspace 刷新重放通过。用户确认无需继续额外的 Mock 按钮人工探索。

## 测试结果

- Mock focused `4/4`、前端全量 `681/681`、Application `484`、MCP 生命周期 `6/6`；严格 Clippy、Rust fmt、TypeScript 类型、架构和源码大小门禁通过。

## CI 情况

- `PENDING`：用户已授权全部任务完成后统一推送并触发 Windows CI，本任务不单独触发。

## 完成总结

- 已修复真实 HTTP 响应生成 Mock 草稿的内部 Header 合同冲突，以及外部 Workspace commit 后 UI 缓存不失效的问题；未放宽安全 Header，也未增加轮询或页面切换兜底。
