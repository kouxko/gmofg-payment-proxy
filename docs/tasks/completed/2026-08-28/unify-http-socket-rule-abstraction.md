# 统一 HTTP 与 Socket 规则抽象、阶段和持久化合同

## 任务信息

- 任务 ID：`TASK-20260828-005`
- 状态：`已完成`
- 任务日期：`2026-08-28`
- 创建时间：`2026-08-28 11:40:28 +08:00`
- 开始时间：`2026-08-28 12:39:34 +08:00`
- 最后更新时间：`2026-08-29 00:04:06 +08:00`
- 完成时间：`2026-08-29 00:04:06 +08:00`
- 创建路径：`docs/tasks/pending/2026-08-28/unify-http-socket-rule-abstraction.md`
- 归档路径：`docs/tasks/completed/2026-08-28/unify-http-socket-rule-abstraction.md`
- 关键词：`RuleDefinition`、`RuleContent`、`HTTP Header`、`HTTP Body`、`Socket`、`RuleStage`、`Listener`、`Tauri`、`MCP`、`Workspace migration`
- 任务优先级：`高`
- 优先级理由：涉及规则领域模型、HTTP/Socket 阶段语义、破坏性持久化 Schema 替换、Tauri/MCP 公共接口、导入导出和运行时执行位置；错误实现可能改变规则顺序、导致规则静默不命中或破坏外部合同。

## 背景与目标

当前规则页面已经把普通 HTTP、HTTP Body Document 和 Socket Document 规则展示在同一工作区，但领域、
应用、持久化、命令和运行时仍维护 `rules` 与 `protocol_rules` 两套完整合同。列表视觉混排也不能代表
真实的跨 Pipeline 全局执行顺序。

目标是采用统一的顶层规则抽象、阶段坐标、持久化集合和 CRUD 合同，同时保留 HTTP 与 Socket 各自
原有的类型化条件、动作和运行时能力。HTTP Header 与 HTTP Body 必须属于同一个 HTTP 规则内容和同一次
阶段匹配上下文；协议包 Decode/Document 可以作为内部实现能力复用，但不能继续暴露为与 HTTP Header
规则并列的第二套 HTTP Body 规则系统。

## 范围

- 建立统一 `RuleDefinition` 聚合，至少统一规则 ID、revision、名称、启用状态、优先级、创建顺序、单个不可变 Listener 绑定、执行阶段和类型化内容。
- 使用带标签的 `RuleContent` 差异化表达 `HttpRuleContent` 与 `SocketRuleContent`；禁止通过大量 nullable 字段伪造统一。
- HTTP 与 Socket 共用同一四阶段坐标：`app_to_proxy`、`proxy_to_upstream`、`upstream_to_proxy`、`proxy_to_app`。
- TLS 握手保留为消息四阶段之外的 `tls_handshake` 阶段。
- HTTP Header、URL/请求信息、raw Body、协议解码后的 Body Document 条件与 HTTP 动作统一属于 `HttpRuleContent`；同一 HTTP 规则可以在 Rust 返回的阶段能力允许时组合 Header 与 Body 条件/动作。
- HTTP Body 使用协议包时，Decode/Schema/Document/Encode 仍保持强类型和精确包版本边界，但这些是 HTTP 内容能力，不再形成独立的 HTTP Body 规则实体、列表、CRUD 或持久化集合。
- Socket 继续使用现有 Schema Document 条件与动作，不增加 HTTP Header、状态码、Mock、故障、`one_shot` 或其他 HTTP 能力。
- 规则创建后 `listener_id` 不可切换；需要更换 Listener 时必须创建新规则或使用明确复制流程，不得隐式改绑。
- 阶段执行顺序固定；`priority` 只在同一阶段和同一执行作用域内比较，UI 不得暗示跨阶段全局混排。
- 采用完整统一方案：用一个带内容类型标签的规则集合直接替换当前 `rules` 与 `protocol_rules`。
- Tauri、MCP、环境候选、导入导出和前端统一使用一套规则查询与修改合同，不保留长期双写或双 CRUD 路径。
- 新实现不读取、转换或迁移旧 Workspace、旧规则集合、旧完整配置、旧导入文档或旧 API payload。
- 检测到旧应用数据时必须 fail-closed 明确报告版本不兼容并提示用户自行清除应用数据；程序不得自动删除、覆盖、修复或转换旧数据。
- 用户清除应用数据后，从新默认数据和统一规则模型开始使用。
- 前端保留一个规则工作区、一个创建入口、一个列表与统一编辑器外壳，根据 Rust 权威 capability 渲染 HTTP 或 Socket 内容编辑区。

## 不在范围

- 不增加任何新的 Socket 匹配条件、动作、故障注入、HTTP 语义、`one_shot` 或隐式兼容能力。
- 不要求 HTTP 与 Socket 具有相同的条件和动作集合；统一的是抽象、阶段、生命周期和接口，不是协议内容。
- 不把所有规则条件和动作合并成一个无约束的大枚举，也不允许前端自行组合 Rust 未声明的能力。
- 不引入跨阶段的全局 priority 排序，不改变固定 Pipeline 阶段顺序。
- 不允许更新现有规则时切换 Listener、协议包、Schema 或内容类型。
- 不保留新旧两套持久化模型的长期双写、静默回退或兼容执行路径。
- 不提供旧 Workspace、旧规则、旧 Tauri/MCP 命令或旧导入导出格式的兼容、迁移、转换、恢复或自动清理。
- 不改变协议包 Frame/Decode/Display/Encode、HTTP 字节保真、TLS、Socket topology 或连接生命周期合同。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-08-28 11:40:28 +08:00` | 用户要求统一 HTTP 与 Socket rules，并先完成只读分析。 |
| `2026-08-28 11:40:28 +08:00` | 用户确认规则使用同一个顶层抽象，具体类型化规则内容差异化表达。 |
| `2026-08-28 11:40:28 +08:00` | 用户确认 Listener 创建后不可切换。 |
| `2026-08-28 11:40:28 +08:00` | 用户确认 Socket 保持原有规则能力，不增加 HTTP 或其他新能力。 |
| `2026-08-28 11:40:28 +08:00` | 用户确认采用完整统一方案 A：领域、持久化、Tauri/MCP 和导入导出使用统一规则合同。 |
| `2026-08-28 11:40:28 +08:00` | 用户确认 HTTP 与 Socket 规则匹配使用一致的四阶段坐标，HTTP/Socket 内容能力仍各自保持原样。 |
| `2026-08-28 11:40:28 +08:00` | 用户进一步确认 HTTP Header 与 HTTP Body 规则必须一起处理并统一为同一个 HTTP 规则内容，而不是继续维护两类 HTTP 规则。 |
| `2026-08-28 11:48:18 +08:00` | 用户明确不考虑任何旧版本兼容性，不实现旧规则、旧 Workspace、旧 API 或旧导入格式迁移。 |
| `2026-08-28 11:48:18 +08:00` | 用户确认旧应用数据由用户自行清除；程序只报告不兼容，不自动删除或转换旧数据。 |

## 未确认事项

无。

## 需求就绪检查

- 问题、用户目标和成功结果：`PASS`，统一规则抽象、阶段、集合和接口，同时保持 HTTP/Socket 内容差异。
- 范围与不在范围：`PASS`，明确不扩展 Socket 能力、不改变协议包和连接合同、不保留长期双路径。
- 输入、输出与状态变化：`PASS`，新版本只使用统一带标签集合，Listener 与类型绑定不可变；旧数据不进入新模型。
- 错误行为：`PASS`，未知内容类型、非法阶段、绑定不匹配、旧数据版本和 capability 不兼容必须 fail-closed；旧数据只提示用户自行清除，不自动删除、转换或回退。
- 具体示例：`PASS`，一条绑定 HTTP Listener 的规则可在 `proxy_to_upstream` 阶段同时匹配 Header 与协议解码 Body 字段；一条 Socket 规则在同名阶段只显示和执行现有 Socket Document 能力。
- 可重复 PASS/FAIL 验收：`PASS`，可通过全新数据初始化、旧数据明确拒绝、统一 API、四阶段运行顺序、HTTP Header+Body 联合匹配和 Socket 能力不扩展逐项判断。
- 会改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-08-28 12:39:34 +08:00`。

## 当前事实、推断与边界

### 当前已验证

- 当前 Workspace 聚合分别保存 `rules` 与 `protocol_rules`。
- 普通 HTTP 规则使用 `request`、`response`、`tls_handshake`，HTTP Body/Socket Document 规则使用四阶段模型。
- HTTP Body 与 Socket 已复用 Document 规则执行核心，但普通 HTTP Header/Body 动作仍通过另一套 Rule/runtime 合同执行。
- 当前统一列表分别查询和修改普通规则、HTTP Body 规则与 Socket 规则，视觉统一不等于统一 CRUD 或全局排序。
- 所有新规则已经要求绑定当前 Workspace 中单个兼容 Listener。

### 推断

- 统一顶层聚合与阶段坐标、保留内容变体，可以删除重复生命周期和接口，同时不把 HTTP 字段泄漏到 Socket。
- HTTP Header 与协议 Body 联合匹配需要统一阶段执行上下文和能力目录，不能只把现有两个列表拼成一个 DTO。

### 未知

- 无产品合同未知项。精确模块拆分、新持久化版本号、提交批次和测试入口由实现前源码审计与计划确定，不得改变已确认合同。

## 最小改动与最优设计比较

| 方案 | 设计与影响 |
| --- | --- |
| 最小改动 | 只增加应用层统一 facade 和前端 union，底层继续维护 `rules`、`protocol_rules` 与两组 CRUD。改动较小，但保留重复模型、双路径和 HTTP Header/Body 分裂，不满足方案 A。 |
| 最优设计 | 以统一 `RuleDefinition`、`RuleStage` 和带标签 `RuleContent` 为领域事实源，直接替换为单一持久化集合与公共接口；HTTP 内容内部统一 Header/Body，Socket 保持原能力；旧数据明确不兼容，由用户自行清除应用数据。 |

采用最优设计。实施必须按可独立验收的小任务推进，不能一次不可回退地替换领域、持久化和运行时。

## 小任务列表

| ID | 任务 | 依赖 | 可并行 | 负责人 | 状态 | 验收标准 |
| --- | --- | --- | --- | --- | --- | --- |
| RUA-01 | 用回归测试锁定当前 HTTP、HTTP Body、Socket 内容与运行行为 | 无 | 否 | 主 Agent | 已完成 | 阶段、条件、动作和运行结果有可重复基线；不建立旧持久化兼容基线 |
| RUA-02 | 定义统一 `RuleDefinition`、`RuleStage`、`RuleContent` 与 capability 合同 | RUA-01 | 否 | 主 Agent | 已完成 | 类型层阻止 HTTP/Socket 内容串用，Listener/类型绑定不可变 |
| RUA-03 | 合并 HTTP Header/Body 规则上下文和阶段执行语义 | RUA-02 | 否 | 主 Agent | 已完成 | 同一 HTTP 规则可按 capability 联合匹配 Header 与 Body，字节与协议边界保持 |
| RUA-04 | 实现全新统一持久化 Schema 和旧数据拒绝边界 | RUA-02 | 否 | 主 Agent | 已完成 | 新数据完整 round-trip；旧数据明确报不兼容且不被自动删除、转换或覆盖 |
| RUA-05 | 统一 Application、Tauri、MCP、环境候选和导入导出合同 | RUA-03、RUA-04 | 否 | 主 Agent | 已完成 | 单一 CRUD/Schema 生效，旧双路径删除且无静默兼容分支 |
| RUA-06 | 让 HTTP/Socket runtime 消费统一快照并保持内容差异 | RUA-03、RUA-05 | 否 | 主 Agent | 已完成 | 四阶段、TLS、排序、热替换、取消和错误传播符合合同 |
| RUA-07 | 统一规则列表、创建入口和编辑器外壳 | RUA-05 | 可与 RUA-06 在合同冻结后并行 | 主 Agent | 已完成 | 一个工作区/列表/入口；按 stage/content 显示能力和真实顺序 |
| RUA-08 | 全量验证、文档同步和整体对抗审查 | RUA-06、RUA-07 | 否 | 主 Agent | 已完成 | 受影响层级测试、新旧版本拒绝证据、构建与高优先级审查全部完成 |

共享领域类型、Schema 和公共接口在 RUA-02/RUA-05 完成前不得并行修改。若后续启用多 Agent，
主 Agent 必须先分配互不重叠的文件所有权并在同一批次完成后统一集成验证。

## 测试计划

- Domain：统一类型序列化/反序列化、未知字段拒绝、内容/Listener/阶段绑定、不可改绑、阶段内排序和 HTTP/Socket capability 隔离。
- HTTP：四阶段顺序、TLS、Header+raw Body、Header+协议 Document 联合条件/动作、原始字节保持、终止动作和现有故障路径。
- Socket：四阶段顺序、Schema 严格类型、字段修改、LocalResponder 方向限制、Frame/Decode/Encode 失败以及“不出现新增 HTTP 能力”。
- Persistence：全新统一集合的空/边界/混合内容 round-trip、损坏记录拒绝、旧持久化版本拒绝、拒绝路径不修改原文件，以及用户清除应用数据后的新默认初始化。
- Application/Tauri：统一 list/get/save/toggle/copy/delete、revision 冲突、绑定不可变、统一生成类型和旧命令删除。
- MCP/导入导出：统一候选 Schema、完整配置 round-trip、旧文档/旧 payload 明确拒绝、未知类型拒绝、预览/确认/应用一致性。
- Runtime：启动快照、热替换、规则命中、取消、并发连接、阶段内优先级、阶段间固定顺序和失败传播。
- Frontend：一个列表/入口、HTTP/Socket 差异编辑器、HTTP Header+Body 同规则、阶段分组、不可用原因、异步晚到和 Workspace 切换。
- 完成前执行受影响 crate 和前端定向测试，再执行仓库规定的完整 Rust/TypeScript 静态门禁、测试、构建和覆盖率检查；无法执行的层级必须记录为未验证，不得用低层 PASS 替代。

## 对抗审查计划

- 检查是否只统一 DTO/UI 却保留双持久化、双写或双运行路径。
- 检查 HTTP Header/Body 是否仍被拆成两个互不知情的规则实体，或联合规则在两个阶段被重复执行。
- 检查是否通过 nullable 字段、默认类型、自动改绑或 Schema 回退掩盖类型不兼容。
- 检查旧 Workspace、旧规则、旧命令或旧导入格式是否被意外兼容、自动转换或自动删除。
- 检查 Socket 是否被意外暴露 HTTP 条件、动作、故障或 `one_shot`。
- 检查列表排序、stage 标签和命中展示是否准确反映实际 Pipeline，不把跨阶段 priority 描述为全局顺序。
- 高优先级任务关闭前执行独立整体对抗审查，并将发现、修复和复审结论写入任务文档。

## 文档影响

- 更新 `docs/architecture/rules-and-protocol-packages.md`，删除“两套规则系统”作为当前事实的描述，改为统一聚合与差异内容合同。
- 更新数据流、持久化、安全、模块和开发指南中的规则集合、阶段、快照与旧数据不兼容说明。
- 更新用户操作说明中的创建类型、HTTP Header/Body 联合编辑、阶段顺序和 Listener 不可切换说明。
- 如公共架构决策发生替代，新增 ADR 并明确旧决策的历史状态；不得直接改写历史 ADR 结论。
- 更新 MCP Schema、工具参考、集成指南、发布验证矩阵和测试证据索引。

## 实施记录

- `2026-08-28 11:40:28 +08:00`：完成只读源码与历史分析；用户确认统一抽象、统一四阶段、Listener 不可切换、Socket 不扩展、完整方案 A，以及 HTTP Header/Body 属于同一个 HTTP 规则内容。本轮仅登记任务，未修改生产代码、未运行正式测试或 CI。
- `2026-08-28 11:48:18 +08:00`：用户取消全部旧版本兼容与迁移要求；确认旧应用数据由用户自行清除。任务改为全新统一 Schema 直接替换，旧数据只 fail-closed 报不兼容，程序不得自动删除或转换。
- `2026-08-28 12:39:34 +08:00`：开始以统一领域类型和拒绝旧数据回归替换双规则模型。
- `2026-08-28 15:20:00 +08:00`：完成统一 Application/Tauri/MCP/Environment 合同和前端单一工作区，生成绑定恢复稳定。
- `2026-08-28 18:30:00 +08:00`：完成 HTTP Header+Document 联合 actor、阶段顺序、连接隔离、失败回滚与 revision 冲突重试。
- `2026-08-28 20:30:00 +08:00`：关闭独立审查发现的旧数据改写、运行时回滚、Document Schema/阶段、Socket stage、Fault 旧入口和前端 capability 问题。
- `2026-08-28 21:40:47 +08:00`：完成全量自动化、正式 App 构建、文档同步、证据与最终复审。

## 修改文件

- `src-tauri/crates/domain/src/unified_rule.rs`、`workspace.rs`：统一聚合、阶段、内容与验证。
- `src-tauri/crates/application/src/facade/unified_rules.rs`、`unified_rule_editor/`：统一 CRUD、编辑能力和持久化前校验。
- `src-tauri/crates/infrastructure/src/adapters/`：单一持久化集合、联合 HTTP actor、运行时快照与拒绝旧数据边界。
- `src-tauri/src/commands/`、`src-tauri/src/mcp/`、`src/generated/rust-types.ts`：统一公共接口和 Schema。
- `src/features/rules/`：单一规则工作区、列表、创建入口和差异化编辑器。
- `docs/architecture/`、`docs/mcp/`、`docs/user-operation-guide.md`：当前统一模型、执行顺序和操作说明。

## 附加文件

- [UNIFIED-RULE-CONTRACT-001](../../testing/evidence/2026-08-28/TASK-20260828-005/UNIFIED-RULE-CONTRACT-001/README.md)

## 验收结果

- `PASS`：统一领域、持久化、公共接口、运行时和前端合同通过自动化、正式构建与独立整体审查。

## 测试结果

- Domain `159/159`；Application 主测试 `461` 项及 `14/7/5/12` 集成组；Infrastructure `642/642`；Host `30/30`；根包 `133/133`；MCP `82/82`。
- 前端最终全量 `61 files / 531 tests`；类型、lint、架构、边界、源码大小和正式 App 构建通过。
- 联合运行时覆盖 one-shot 编码失败、HTTP 动作失败、跨阶段 Document 可见性、双连接隔离和 revision 冲突重试。

## CI 情况

- `PASS`：Windows CI 的 Companion、前端、覆盖率、Rust 测试、严格 lint 和独立运行时门禁全部通过；Windows MSI、NSIS 与便携版构建成功。

## 完成总结

- HTTP 与 Socket 已使用单一 `RuleDefinition`、统一阶段、单一持久化集合和公共接口。HTTP Header/Body 在同一规则与同一联合运行时内匹配，Socket 保持原有 Document 能力；旧数据明确拒绝且不自动删除、转换或覆盖。
