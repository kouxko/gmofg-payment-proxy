# 修复外部 Workspace 提交后管理页保持旧快照

## 任务信息

- 任务 ID：`TASK-20260828-006`
- 状态：`已完成`
- 任务日期：`2026-08-28`
- 创建时间：`2026-08-28 21:03:56 +08:00`
- 开始时间：`2026-08-28 21:03:56 +08:00`
- 最后更新时间：`2026-08-29 00:04:54 +08:00`
- 完成时间：`2026-08-29 00:04:54 +08:00`
- 创建路径：`docs/tasks/pending/2026-08-28/fix-workspace-management-external-refresh.md`
- 归档路径：`docs/tasks/completed/2026-08-28/fix-workspace-management-external-refresh.md`
- 关键词：`Workspace 管理`、`snapshot_required`、`useWorkspaceQueryInvalidation`、`MCP environment apply`、`真实 App 重放`
- 任务优先级：`高`
- 优先级理由：涉及 Application 提交事件到前端权威状态的一致性与跨层公共刷新合同；旧快照会让用户基于错误的 Listener 数量和 revision 操作 Workspace。

## 背景与目标

本任务派生自 `TASK-20260828-003 / RUNNING-APP-FINDINGS-REGRESSION-001`。重启后的正式 Release App 中，MCP Environment apply 已把当前 Workspace 从两个 Listener 替换为一个，顶部统计立即显示 `入口 1`，但保持打开的 Workspace 管理页仍显示列表 `代理入口 2 / 版本 8`，详情仍显示 `代理入口 2 / 版本 8`；权威 `workspace_get` 已是一个 Listener 和新 revision。

目标是在不依赖切页、轮询或伪造空状态的前提下，让 Workspace 管理页列表和当前详情消费现有外部提交事件并重新读取权威状态。

## 范围

- 为真实 `WorkspacesView` 接入当前 Workspace 查询失效与刷新合同。
- 处理 `snapshot_required` 和当前 Workspace 的 `workspace_changed`。
- 保证刷新开始前推进查询代次，迟到旧响应不得覆盖新数据。
- 增加真实组件回归并重新构建 App，复测顶部、列表和详情同步变化。

## 不在范围

- 不增加轮询、页面切换兜底、额外缓存权威或静默重试。
- 不改变 Application 事件格式、Environment apply、Workspace revision 或 Listener 生命周期。
- 不修改 Mock 草稿、规则执行或 Android 功能。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-08-28 21:03:22 +08:00` | 用户重启电脑后要求重新尝试；真实 App 窗口恢复正常，但 Workspace 页面旧快照仍稳定复现，因此继续修复实际测试问题。 |

## 未确认事项

无；现象、权威后端结果、受影响页面与期望刷新行为均已通过真实 App 明确。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`
- 范围与不在范围：`PASS`
- 输入、输出与状态变化：`PASS`
- 错误行为：`PASS`，读取失败继续展示现有错误，不用旧值冒充新权威状态。
- 具体示例：`PASS`，顶部为 `入口 1` 时列表和详情不得继续显示 `2 / revision 8`。
- 可重复 PASS/FAIL 验收：`PASS`
- 改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-08-28 21:03:56 +08:00`

## 问题与根因分析

### 实际现象与环境

- 环境：重新构建的正式 macOS App、隔离应用目录、本机 MCP、HTTP/TCP 模拟 Server。
- 外部提交前 Workspace 为两个 Listener、revision 8；外部提交后权威状态为一个 Listener、新 revision。
- 顶部统计刷新为一个 Listener；Workspace 管理页列表和详情保持旧值。

### 预期行为和依据

`TASK-20260828-003` 已定义外部 Workspace 提交后当前页面无需切页即刷新；现有 `snapshot_required` 是 Application commit 成功后的统一事件。

### 最小复现

1. 打开 Workspace 管理页并保留页面。
2. 通过 MCP Environment apply 替换当前 Workspace Listener 集合。
3. 等待提交事件处理完成。
4. 比较顶部入口统计、列表 Listener 数量、详情 Listener 数量与 revision。

### 当前已验证

- 正式 App 重启后 UI 正常渲染，空白窗口未复现。
- 后端提交成功且顶部统计收到事件并刷新。
- `WorkspacesView` 创建 list/detail 查询，但未调用现有 `useWorkspaceQueryInvalidation`；顶栏 `GlobalStatusBar` 已调用该 Hook。

### 推断、未知与候选原因

- 推断：`WorkspacesView` 缺少事件接线是列表/详情不刷新的直接原因。
- 未知：接线后是否存在 draft 覆盖新详情的问题；通过组件回归和真实重放验证。
- 已排除：Application 未提交、顶部未收到事件、需要页面级轮询。

### 根因与正确修复边界

根因是修复只覆盖了合成 Hook 测试和顶栏/其他页面，遗漏 Workspace 管理页自身的 list/detail 查询。正确边界是在该页面复用同一查询失效 Hook，并用真实组件事件测试锁定列表、详情和迟到响应。

## 最小改动与最优设计比较

| 方案 | 分析 |
| --- | --- |
| 页面收到事件后直接修改 Listener 数量 | 复制领域状态且无法获得新 revision，拒绝。 |
| 增加轮询或切页刷新 | 掩盖事件接线缺失并增加延迟，拒绝。 |
| `WorkspacesView` 复用统一查询失效 Hook | 最小改动，继续以后端为唯一权威，采用。 |

## 小任务列表

| ID | 任务 | 依赖 | 状态 | 验收 |
| --- | --- | --- | --- | --- |
| WER-01 | 增加真实 WorkspacesView 外部事件失败回归 | 无 | 已完成 | 修改前稳定失败，覆盖列表和详情 |
| WER-02 | 接入统一查询失效 Hook | WER-01 | 已完成 | 无轮询、无切页，列表与详情更新 |
| WER-03 | 前端全量、静态门禁和正式 App 重放 | WER-02 | 已完成 | 自动化与真实 UI 一致；重启后重复启动数据面保持 NOT_RUN |
| WER-04 | 证据、对抗审查与归档 | WER-03 | 已完成 | 结果可复测且无遗留阻断 |

## 测试计划

- WorkspacesView 组件：发出 `snapshot_required`，模拟 list/detail 从旧值更新到新值；断言页面无需重挂载或切页。
- 迟到响应：事件先 invalidate，再返回旧请求；旧值不得覆盖新值。
- 前端聚焦、全量、类型、lint、边界、source-size 与正式构建。
- 正式 App：保持 Workspace 页打开，通过 MCP apply 改变 Listener 集合，核对顶部、列表和详情。

## 对抗审查计划

- 检查是否新增轮询、页面导航兜底、直接改计数或重复事件订阅逻辑。
- 检查 draft 是否遮蔽外部 revision，以及非当前 Workspace 事件是否错误刷新当前详情。
- 检查实际 Release App 与自动化是否使用同一事件类型。

## 文档影响

- 更新本任务、派生证据和测试索引；产品说明无需变更。

## 实施记录

- `2026-08-28 21:03:56 +08:00`：完成真实现象、权威后端结果与遗漏接线的根因定位，登记任务并进入 TDD。
- `2026-08-28 21:07:52 +08:00`：新增真实 BootstrapProvider、Channel 与 WorkspacesView 回归，修改前列表和详情稳定保持旧值。
- `2026-08-28 21:21:00 +08:00`：接入统一查询失效 Hook；独立审查发现并修复整份 draft 遮蔽新详情的问题，仅保留未保存名称覆盖。
- `2026-08-28 21:24:00 +08:00`：补齐刷新失败旧快照标识、首次读取失败和错误空态，审查最终 P0/P1/P2 为 0。
- `2026-08-28 21:35:55 +08:00`：最新正式 App 重启后正常渲染，外部配置周期使列表和详情从 revision 16 无切页刷新到 18，运行态恢复为停止。
- `2026-08-28 21:42:26 +08:00`：完成全量前端、正式构建、证据和归档。

## 修改文件

- `src/features/workspaces/workspaces-view.tsx`：接入统一事件失效、合并最新详情与名称草稿、显示刷新失败旧快照和首次读取失败。
- `src/features/workspaces/workspaces-external-refresh.test.tsx`：真实事件成功、草稿遮蔽、刷新失败和首次失败回归。
- `src/features/workspaces/workspaces-view.test.tsx`：相邻 CRUD 与刷新行为保护。
- `docs/testing/evidence/2026-08-28/TASK-20260828-006/WORKSPACE-LIVE-REFRESH-001/`：自动化、正式 App 和派生重放证据。

## 附加文件

- 父任务：`TASK-20260828-003`
- 父用例：`RUNNING-APP-FINDINGS-REGRESSION-001`
- 父证据：`docs/testing/evidence/2026-08-28/TASK-20260828-003/RUNNING-APP-FINDINGS-REGRESSION-001/`
- 本次证据：[WORKSPACE-LIVE-REFRESH-001](../../testing/evidence/2026-08-28/TASK-20260828-006/WORKSPACE-LIVE-REFRESH-001/README.md)

## 验收结果

- `PASS_WITH_NOT_RUN`：TASK006 的成功刷新、草稿合并和错误边界通过；重启后 UI 点击控制通道异常，重复启动数据面未执行。

## 测试结果

- Workspace 聚焦 `18/18`；前端全量 `61 files / 531 tests`；类型、lint、边界和源码大小通过。
- 最新正式 App 构建通过；重启后正常渲染，列表与详情 revision `16 → 18` 实时一致。
- 独立复审 `APPROVE`，P0/P1/P2 均为 0。

## CI 情况

- `PASS`：Windows CI 的 Companion、前端、覆盖率、Rust 测试、严格 lint 和独立运行时门禁全部通过；Windows MSI、NSIS 与便携版构建成功。

## 完成总结

- Workspace 管理页现在复用统一 Application 事件失效合同。外部更新会刷新列表和详情；未保存名称保留，但 Listener 数量和 revision 始终来自最新权威详情；读取失败会明确区分旧快照与首次不可用状态。
