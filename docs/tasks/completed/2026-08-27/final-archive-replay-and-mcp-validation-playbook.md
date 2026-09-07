# 最终归档场景复跑与 MCP 验证经验指南

## 任务信息

- 任务 ID：`TASK-20260827-003`
- 状态：`已完成`
- 任务日期：`2026-08-27`
- 创建时间：`2026-08-27 12:42:34 +08:00`
- 开始时间：`2026-08-27 12:58:00 +08:00`
- 最后更新时间：`2026-08-27 22:27:08 +08:00`
- 完成时间：`2026-08-27 22:27:08 +08:00`
- 创建路径：`docs/tasks/pending/2026-08-27/final-archive-replay-and-mcp-validation-playbook.md`
- 归档路径：`docs/tasks/completed/2026-08-27/final-archive-replay-and-mcp-validation-playbook.md`
- 关键词：`final validation`、`archived scenarios`、`running proxy`、`replay`、`MCP resource`、`validation playbook`、`Windows CI`
- 任务优先级：`高`
- 优先级理由：覆盖最终发布门禁、运行中 Proxy、外部链路、协议/证书/生命周期场景和 MCP 公共资源；错误归纳可能给用户不安全或不正确的配置建议。

## 背景与目标

用户要求在最后提交前，把项目已经归档、讨论并形成测试资源的场景重新执行一遍；适用场景必须启动
真实 Proxy 后执行，不能仅以低层单元测试替代。同时需要把经过验证、可跨任务复用的经验整理为 MCP
可读取的指南，使 MCP 用户能够获得合理的配置、排障、风险与验证建议。

## 范围

- 盘点 `docs/testing/evidence/**`、发布验证矩阵、任务文档和活动 fixture 中仍适用于当前版本的场景。
- 建立场景清单并分类：自动化、运行中 Proxy、本地依赖、外部服务、真实 Android 设备、人工 UI。
- 对当前可执行场景逐项复跑；要求真实 Proxy 的场景先启动最终构建，再发送实际请求并观察结果。
- 对资源不可用的场景记录明确 `NOT_RUN`、缺失条件和复测步骤，不得用其他层级 PASS 替代。
- 将稳定经验整理为版本化 Markdown 指南，并作为新的只读 MCP resource 发布。
- 指南覆盖配置前检查、分层诊断、HTTP/Socket/TLS/mTLS/证书/协议包/规则/Android/生命周期常见失败、建议顺序和停止条件。
- MCP 建议必须区分已观测事实、推断和未知；不得输出秘密、业务敏感报文或把失败说成成功。
- 完成全部待办后再执行提交、推送并触发 GitHub Windows CI；只有远程 CI 成功才报告 Windows 构建成功。

## 不在范围

- 把历史一次性结果硬编码为当前环境事实。
- 新增会修改 Workspace、Listener、规则或证书的“自动修复”工具。
- 为了得到 PASS 而改写历史 expected、伪造外部服务或跳过必要的运行中 Proxy 验证。
- 在测试记录中保存 Git、提交、HEAD、哈希、暂存或未跟踪状态信息。
- 自动发布、部署或创建额外远程环境。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-08-27` | 用户要求最终提交前重新执行已归档测试场景。 |
| `2026-08-27` | 适用场景应在 Proxy 启动后进行真实测试。 |
| `2026-08-27` | 已讨论场景需整理为通用经验并提供给 MCP，使用户获得合理建议。 |
| `2026-08-27` | 所有任务完成后提交并推送到 GitHub，触发 Windows CI；成功结果以后者为准。 |

## 未确认事项

- 无阻塞登记与盘点的问题。MCP 交付采用现有版本化只读 Markdown resource 模式，不新增写工具；这是当前架构中最小且职责正确的实现。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`
- 范围与不在范围：`PASS`
- 输入、输出与状态变化：`PASS`
- 错误行为：`PASS`，不可执行场景必须 `NOT_RUN`，MCP 建议必须 fail-closed 且不伪造事实。
- 具体示例：`PASS`，TLS/mTLS 场景先检查 TCP、握手、证书链、主机名和客户端身份，再决定是否进入业务报文；任一层失败即停止并给出对应建议。
- 可重复 PASS/FAIL 验收：`PASS`
- 改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-08-27 12:58:00 +08:00`。

## 当前事实、推断与未知

### 当前已验证

- 仓库已有按任务/用例归档的测试目录、发布验证矩阵和多类活动 fixture。
- MCP 已有版本化只读 Markdown resources，可新增指南而不扩展写权限。
- 部分归档场景依赖外部 Host、证书、真实设备或人工 UI，不能保证当前环境始终可执行。

### 推断

- 直接“全部重跑”但不先分类，会把资源验证、单元测试、真实链路和人工观察混为同一种 PASS。
- MCP 若直接读取历史结果，可能把过期环境状态当成当前事实；指南应抽取方法、顺序、边界和停止条件，而不是固定结论。

### 未知

- 最终复跑时真实 Android 设备、外部 TLS Host 和其他外部依赖是否在线。
- GitHub Windows runner 的最终结果，必须在推送后实际等待确认。

### 正确边界

- 本地归档负责可复现输入、步骤和当次结果；MCP resource 负责通用方法与安全建议。
- MCP 在需要当前状态时继续调用现有只读工具，不把指南当成实时事实来源。

## 最小改动与最优设计比较

| 方案 | 分析 |
| --- | --- |
| 只写一份总结文档 | 修改少，但 MCP 无稳定资源入口，且容易把历史结果与通用方法混合，不采用。 |
| 把经验塞进每个工具 description | 客户端可见，但重复、难维护、Schema 噪声大，且无法承载分层决策，不采用。 |
| 版本化 MCP 验证指南 resource | 复用现有 resource 架构；文档是单一权威，MCP 可发现和读取，工具合同不膨胀。结合场景清单与最终复跑，采用。 |

## 小任务列表

| ID | 任务 | 依赖 | 可并行 | 状态 | 验收 |
| --- | --- | --- | --- | --- | --- |
| FVR-01 | 盘点归档、fixture 与发布矩阵并建立可执行分类 | 全部功能任务冻结 | 否 | 已完成 | 每个适用场景有来源、前置、执行层级和预期 |
| FVR-02 | 完成自动化与静态门禁复跑 | FVR-01 | 是 | 已完成 | 所有适用本地门禁 fresh PASS |
| FVR-03 | 启动最终 Proxy 并复跑本地/协议/网络场景 | FVR-01 | 否 | 已完成 | 实际请求、运行状态和输出可复核 |
| FVR-04 | 复跑外部服务、真机和人工 UI 场景 | FVR-01 | 可按资源并行 | 已完成 | 可用场景 PASS，资源缺失场景准确 NOT_RUN |
| FVR-05 | 编写并发布 MCP 验证经验指南 resource | FVR-01..04 | 否 | 已完成 | resource 可列举、读取，内容与实际边界一致 |
| FVR-06 | 整体对抗审查、最终提交、推送和 Windows CI | FVR-02..05 | 否 | 已完成 | 整体审查 APPROVE，完整 CI 与 Windows 安装包构建成功 |

## 测试计划

- 场景清单：目录、任务、用例、前置条件、当前适用性和复测入口一一对应。
- 自动化：受影响 Rust crates、前端测试/typecheck、bindings、fmt/clippy、architecture/source-size。
- 运行中 Proxy：HTTP、Socket、TLS/mTLS、规则、抓包、协议包和生命周期场景按资源逐项执行。
- 设备/外部：实际可用时运行；不可用时保留准确状态与重放步骤。
- MCP：resource list/read、URI/version/mime/title、内容边界、工具交叉引用和未知状态表述测试。
- Windows：推送后等待 GitHub CI，检查 Windows job 的构建和测试结果。

## 对抗审查计划

- 检查历史 PASS 是否被错误沿用、NOT_RUN 是否被掩盖、低层测试是否替代真实链路。
- 检查 MCP 是否泄漏秘密、输出未经验证的当前状态、建议危险降级或越过 Application 权限边界。
- 检查指南与工具参考、诊断架构、用户操作说明及当前源码是否一致。
- 完成前整体审查必须为 `APPROVE/CLEAR`。

## 文档影响

- 新的 MCP 验证经验指南 Markdown。
- `src-tauri/src/mcp/resources.rs` 和 resource 合同测试。
- `docs/mcp/tool-reference.md`、`docs/mcp/diagnostic-architecture.md`、必要的操作/验证文档。
- 本任务文档及最终测试记录。

## 实施记录

- `2026-08-27 12:42:34 +08:00`：登记最终归档复跑和 MCP 通用经验需求；确定采用只读版本化 resource，不新增自动修改工具。
- `2026-08-27 14:51:10 +08:00`：完成场景分类、归档资源复跑、全本地门禁、隔离 App 的 MCP 指南读取、完整候选 create/apply/status、退出重启和端口释放；Android 真机与授权 Tango 交易因资源缺失保持 `NOT_RUN`。
- `2026-08-27 15:33:29 +08:00`：完成多 CA Bundle 功能归档与 Android 多设备最终错误归属回归；重新执行 Rust 工作区、前端全量和全部静态门禁。生产非回环 IPv4 MCP 实调在本机受透明代理截流，严格期限超时并保持 `NOT_RUN`，交远程 Windows 环境复验。
- `2026-08-27 22:27:08 +08:00`：完成本地最终回归、运行中 App/MCP 冒烟、整体对抗审查、完整远程 CI 和 Windows x64 安装包/便携包构建；所有远程门禁成功，任务关闭。

## 修改文件

- `docs/testing/final-replay-scenario-inventory-20260827.md` 与本次测试记录。
- `docs/mcp/validation-playbook.md`、`src-tauri/src/mcp/resources.rs` 及 MCP resource 合同测试。
- 归档测试记录中不允许的版本控制状态、提交和摘要类材料已移除；剩余 JSON 与文本已完成禁止项扫描。

## 附加文件

- `docs/testing/evidence/**`
- `docs/testing/release-validation-matrix.md`
- 各已完成任务引用的活动 fixture 与 replay 入口。

## 验收结果

- `PASS_WITH_NOT_RUN`：所有可执行自动化、静态、外部包、远端零业务字节 TLS、隔离 App 和远程 Windows 场景通过；Android 真机 A/B 与授权 Tango 交易因资源缺失准确记录为 `NOT_RUN`。

## 测试结果

- 前端完整测试 `671/671 PASS`；Application `480/480 PASS`、Infrastructure `669/669 PASS`、Proxy/Runtime `225/225 PASS`。
- MCP 定向 `71/71 PASS`；隔离 App create=`preview_ready`、apply=`apply_queued`、status=`committed`，关闭后端口释放且重启恢复。
- Nuvei Python `14/14`、AU EFTEX `72/72`、Deno ISO `14/14`、Nuvei Rhai `6/6` PASS。
- 架构、源码规模、格式、严格 Clippy、生产构建、品牌扫描和 Windows 静态编译检查 PASS。

## CI 情况

- `PASS`：完整 CI 的 Android Companion、Linux 覆盖门禁和 Windows 验证全部成功。
- `PASS`：Windows x64 构建成功，已生成 MSI、NSIS 安装程序和便携包；安装包为未签名测试版本。

## 完成总结

- 已完成归档场景分类与复跑、运行中 App/MCP 验证、通用 MCP 验证指南、整体审查、完整 CI 和 Windows 测试包交付；不可执行的真机与授权外部交易继续保持明确 `NOT_RUN`。
