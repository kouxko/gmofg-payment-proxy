# 当前运行 App 的 Proxy 与模拟 Server 归档场景重放

## 任务信息

- 任务 ID：`TASK-20260828-001`
- 状态：`已完成`
- 任务日期：`2026-08-28`
- 创建时间：`2026-08-28 09:40:55 +08:00`
- 开始时间：`2026-08-28 09:40:55 +08:00`
- 最后更新时间：`2026-08-28 10:12:34 +08:00`
- 完成时间：`2026-08-28 10:12:34 +08:00`
- 创建路径：`docs/tasks/pending/2026-08-28/running-app-proxy-archive-replay.md`
- 归档路径：`docs/tasks/completed/2026-08-28/running-app-proxy-archive-replay.md`
- 关键词：`running App`、`Proxy`、`mock server`、`archive replay`、`HTTP`、`Socket`、`MCP`
- 任务优先级：`高`
- 优先级理由：使用当前真实 App、网络 Listener、模拟 Server 和现有 Workspace 执行运行态验证，涉及协议、生命周期、端口和用户当前数据边界。

## 背景与目标

用户已打开当前 release App，要求复用之前归档的测试方式，通过真实操作 Proxy 与本地模拟 Server
重新执行适用场景。目标是只把本次实际请求、响应、运行状态和清理结果记为当前结论，不沿用昨天结果。

## 范围

- 复用 `TASK-20260827-003 / FINAL-REPLAY-001` 的场景分类、停止条件和重放入口。
- 确认当前 App 进程、MCP、外部包入口和当前 Workspace/Listener 运行状态。
- 当前 Workspace 为空时，使用 MCP 环境候选临时配置本机 HTTP/Socket Listener；通过 App UI 启停，完成后恢复空 Workspace。
- 验证 Proxy 观测、请求/响应转发、规则结果、错误归属和端口清理。
- 记录本次可执行场景的 PASS/FAIL，以及资源不足场景的 NOT_RUN。

## 不在范围

- 不安装协议包、不创建持久规则、不导入或删除证书、不修改设备配置；临时 Listener 必须通过原子候选提交并在用例结束前恢复。
- 发送真实生产交易、使用未授权远端业务端点或修改真实 Android 设备。
- 用单元测试结果替代用户要求的当前 App、Proxy 与模拟 Server 运行态行为。
- 在测试档案中保存提交、版本控制状态或摘要信息。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-08-28 09:40:55 +08:00` | 用户确认当前 App 已打开，并要求使用之前的测试回放方式操作 Proxy 与模拟 Server。 |
| `2026-08-28 09:44:12 +08:00` | 当前 Workspace 确认为空；为满足真实 Proxy 重放目标，采用可恢复的临时环境候选加入两个本机 Listener，运行后恢复空 Workspace，不触及证书、协议包或设备。 |

## 未确认事项

- 当前 Workspace 是否已经存在适合重放的 HTTP/Socket Listener 与规则，由只读 MCP/运行状态检查确定。
- 若缺少必要 Listener，本任务保持对应场景 `NOT_RUN`，不自动创建或替换配置。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`
- 范围与不在范围：`PASS`
- 输入、输出与状态变化：`PASS`，只使用现有配置执行请求并观察结果。
- 错误行为：`PASS`，任一运行态失败必须保留原始阶段和稳定错误，不重试掩盖。
- 具体示例：`PASS`，本地模拟 HTTP Server 返回固定响应，经现有 Proxy Listener 转发后比较客户端、Server 与 Exchange 观测。
- 可重复 PASS/FAIL 验收：`PASS`
- 改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-08-28 09:40:55 +08:00`

## 当前事实、推断与未知

### 当前已验证

- 当前 release App 进程正在运行。
- 当前 App 监听 `8765` 和 `17653`；`17653` 同时存在 IPv4/IPv6 MCP Listener。
- macOS WKWebView 不提供可用 WebDriver 自动化入口，UI 点击不能用 WebDriver 冒充已执行。

### 推断

- `8765` 是 App 外部协议包入口，不等同于当前 Workspace 的 HTTP/Socket 业务 Listener；必须通过当前状态读取确认实际入口。

### 未知

- 当前无可复用 TLS/mTLS、外部协议包和 Android 真机运行资源；对应场景保持 `NOT_RUN`。

## 最小改动与最优设计比较

| 方案 | 分析 |
| --- | --- |
| 直接运行所有历史脚本 | 可能安装包、切换 Workspace 或覆盖当前配置，不采用。 |
| 只运行自动化测试 | 不满足当前 App、Proxy 与模拟 Server 的运行态验收，不采用。 |
| 先只读发现，再按现有入口执行真实请求 | 不修改用户配置，能够逐层证明 Server、Proxy、客户端与观测结果，采用。 |

## 小任务列表

| ID | 任务 | 依赖 | 状态 | 验收 |
| --- | --- | --- | --- | --- |
| RAR-01 | 读取当前 App/MCP/Workspace/Listener 状态 | 当前 App 已启动 | 已完成 | 当前单一空 Workspace、MCP/外部包服务与运行状态已确认 |
| RAR-02 | 重放 HTTP Proxy + 模拟 Server 场景 | RAR-01 | 已完成 | HTTP 201、Server 实收、抓包和 Exchange 一致 |
| RAR-03 | 重放适用 Socket/TLS/规则场景 | RAR-01 | 已完成 | Socket 逐字节一致；日志与规则能力已验；TLS/mTLS/外部包/Android 准确 NOT_RUN |
| RAR-04 | 清理本次模拟 Server 并汇总结果 | RAR-02..03 | 已完成 | Listener 已停止、空 Workspace 已恢复、模拟 Server 已关闭 |

## 测试计划

- MCP capabilities、resources 和只读状态工具确认当前 App 与 Workspace 状态。
- 每个模拟 Server 使用临时端口、固定输入和完整请求/响应捕获。
- 通过当前 Listener 发送真实请求，比较模拟 Server 实收、客户端实收和 Exchange 观测。
- 结束后确认只终止本次启动的模拟 Server，App 与用户原有 Listener 保持运行。

## 对抗审查计划

- 检查是否把 MCP、外部包入口或 TCP 表面连接误当业务 Proxy 成功。
- 检查是否修改了当前 Workspace、规则或证书。
- 检查失败是否被重试、空响应或低层自动化 PASS 掩盖。

## 文档影响

- 新增本次派生测试档案；不修改产品和架构文档。

## 实施记录

- `2026-08-28 09:40:55 +08:00`：登记当前运行 App 的正式重放任务；确认先只读发现，再执行不会改写用户配置的模拟 Server 场景。
- `2026-08-28 09:44:12 +08:00`：MCP 确认只有一个已选择的空 Workspace；创建可恢复 HTTP/Socket 临时环境。
- `2026-08-28 09:54:56 +08:00`：第一次数据面重放通过；运行中恢复被 `RUNTIME_ACTIVE` 正确拒绝，停止 Listener 后恢复成功。
- `2026-08-28 10:02:38 +08:00`：第二次完整重放启动两个 Listener；HTTP/Socket、抓包和 Exchange 全部通过。
- `2026-08-28 10:04:05 +08:00`：真实 HTTP 服务器响应生成 Mock 草稿失败，定位为计算后的 `content-length` 与 Domain 禁止托管 Header 的合同冲突。
- `2026-08-28 10:05:15 +08:00`：停止两个 Listener，环境候选恢复为空 Workspace 并提交成功；模拟 Server 关闭。
- `2026-08-28 10:06:23 +08:00`：诊断日志按时间倒序显示通过；发现入口列表和顶栏对外部 Workspace 变化刷新不一致。
- `2026-08-28 10:12:34 +08:00`：完成证据归档和结果汇总。

## 修改文件

- 本任务文档、本次测试证据和可复测脚本；未修改生产源码、产品配置或用户证书/协议包/设备。

## 附加文件

- 派生来源：`TASK-20260827-003 / FINAL-REPLAY-001`。
- 父证据：`docs/testing/evidence/2026-08-27/TASK-20260827-003/FINAL-REPLAY-001/`。
- 本次证据：`docs/testing/evidence/2026-08-28/TASK-20260828-001/RUNNING-APP-REPLAY-001/`。

## 验收结果

- `FAILED_WITH_NOT_RUN`
- `PASS`：真实 HTTP/Socket 数据面、模拟 Server 实收、抓包、Exchange、诊断日志倒序、空 Workspace 规则能力和最终恢复。
- `FAIL`：服务器响应生成 Mock 草稿；Workspace/Listener 外部变化后的入口页和顶栏刷新。
- `NOT_RUN`：TLS/mTLS、在线外部协议包、Android 真机和远端业务交易。

## 测试结果

- HTTP：`201`，请求/响应/terminal 三阶段齐全，模拟 Server marker 与正文一致。
- Socket：发送 `29` 字节，接收 `34` 字节，结果等于 `ECHO:` 加原始输入，Exchange `completed`。
- 诊断日志：首行 `10:05:15.731`，后续时间稳定递减。
- 恢复：`preview_ready -> apply_queued -> committed`，最终入口、规则、协议规则、Android 方案与运行状态均为 0。
- 证据：`docs/testing/evidence/2026-08-28/TASK-20260828-001/RUNNING-APP-REPLAY-001/README.md`。

## CI 情况

- `N/A`：本任务只执行当前已构建 App 的运行态重放，不修改生产源码。

## 完成总结

- 当前运行 App 的本机 HTTP/Socket Proxy 与模拟 Server 重放完成，数据面和生命周期清理正确。
- 运行态验收复现两个需要后续修复的问题：Mock 草稿 Header 合同冲突，以及外部 Workspace 变化后的 UI 刷新不一致。
- 未具备真实资源的 TLS/mTLS、外部协议包和 Android 场景没有被自动化结果替代，保持 `NOT_RUN`。
