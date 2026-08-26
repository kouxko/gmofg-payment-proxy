# MCP 对话式完整环境配置

## 任务信息

- 任务 ID：TASK-20260825-006
- 状态：进行中
- 任务日期：2026-08-25
- 创建时间：2026-08-25 21:58:53 +08:00
- 开始时间：2026-08-26 00:05:01 +08:00
- 最后更新时间：2026-08-26 11:36:39 +08:00
- 完成时间：未完成
- 创建路径：`docs/tasks/pending/2026-08-25/mcp-environment-configuration.md`
- 归档路径：未归档
- 关联提交：`a185cc8d4300e3bd5fabff82c045896543964642`（G033 合同、DTO、Schema、回归锁与证据）
- 关键词：MCP、环境配置、Workspace、Listener、证书、TLS、mTLS、协议包、规则、Android、远程访问、明文 HTTP、原子应用

## 背景

当前嵌入式 MCP 是绑定 `127.0.0.1:17653` 的无认证只读服务，只允许读取 Application 投影、
运行状态、诊断和公开证书元数据。用户要求把 MCP 扩展为对话式完整环境配置入口：外部 MCP
客户端提供证书、私钥、密码、Server 地址和其他环境信息后，Proxy 先分层验证候选环境，返回完整
变更预览；用户明确确认后，再把候选配置原子应用到指定或新建 Workspace。

该要求明确替代现有 MCP 的 loopback-only 和 read-only 产品边界。用户进一步确认 MCP 监听所有
本机网络接口，继续使用明文 HTTP，不增加客户端认证、授权、来源 IP allowlist 或 TLS。因而任何
能够访问 MCP 端口的主机都拥有读取、提交候选和修改 Proxy 配置的能力；技术验证、预览和候选令牌
只保证配置正确性、完整性和新鲜度，不证明调用者身份或权限。

## 目标

- MCP 客户端能够通过对话收集并提交一个完整 Workspace 环境链路所需的信息。
- 用户在创建候选时明确选择一个现有 Workspace，或明确选择新建 Workspace。
- 候选配置可以覆盖目标 Workspace 中该环境需要的证书、Listener、TLS/mTLS、协议包、规则、
  故障行为和 Android 路由，不修改其他 Workspace 或应用全局设置。
- Proxy 在任何持久化前完成静态、证书、网络、TLS 和协议包分层验证。
- MCP 返回不含私钥和密码的精确变更预览及逐层验证结果。
- 只有收到绑定该候选版本的一次性确认令牌后，才允许在 Application mutation gate 内原子应用。
- 任一步失败时保持原 Workspace、共享引用和受保护证书材料不变，并清理本次未提交的内存材料。
- 完成后提供可重复的自动化、网络、证书、规则、持久化和真实 App 证据。

## 实现范围

### MCP 网络与工具合同

- MCP 服务监听所有可用网络接口，不再限制为 loopback。
- 继续使用明文 Streamable HTTP；不增加 HTTPS、客户端证书、登录、token、Origin、来源 IP 或
  其他认证和授权门禁。
- 保留现有只读工具，并新增类型化的候选创建/验证/预览、确认应用、取消候选和候选状态工具。
- 写工具必须正确声明 mutation/destructive/idempotent 注解，不能继续伪装为 read-only。
- 所有请求、材料、候选、预览和输出继续遵守明确的数量、字节、并发、deadline 和取消预算。
- 未知字段、超限、合同不匹配和验证失败全部 fail-closed，禁止默认成功、静默忽略或降级。

### 输入材料

- 只接受 MCP 请求直接携带的 PEM 文本或有界二进制内容，以及对应的结构化地址和配置字段。
- 不接受任意本机文件路径，不主动读取文件，不接受远程 URL，不主动下载证书或配置。
- 私钥和密码在候选确认前只存在于进程内存；不得进入日志、诊断、预览、错误详情、审计记录或
  普通 MCP 返回值。
- 预览只允许返回公开证书元数据、角色、有效期、Subject、Issuer、SAN 和稳定指纹。
- 成功应用后，私有材料只能通过现有受保护证书存储边界持久化，Workspace 只保存安全引用。

### Workspace 与完整链路

- 候选创建时必须由用户选择目标现有 Workspace，或提供明确的新 Workspace 名称。
- MCP 不得根据当前 UI 选择、最近使用记录或名称相似度自行决定目标 Workspace。
- 目标 Workspace 内允许配置完整环境链路，包括：
  - 一个或多个 HTTP/Socket Listener 及端口、地址、超时、转发和协议模式；
  - 下游 TLS/mTLS、上游 TLS/mTLS、SNI、Trust Store、Server/Client Identity 引用；
  - 内置或外部协议包引用、版本、Schema 和必要运行配置；
  - HTTP 基础规则、协议 Document 规则、Mock、故障和方向相关行为；
  - 该 Workspace 所需的 Android profile、应用选择和路由配置；
  - 该链路引用的托管证书材料。
- 不修改其他 Workspace，不修改应用全局 Settings，不删除无关共享协议包或证书材料。
- 对共享资源只允许创建目标 Workspace 实际可达的引用；清理时必须证明没有任何 Workspace 引用。

### 分层验证

候选必须依次完成并分别报告：

1. MCP Schema、字段、大小和交叉引用验证。
2. Workspace/Listener/规则/Android 领域不变量验证。
3. 证书格式、角色、证书链、私钥匹配、用途、有效期和引用一致性验证。
4. 协议包存在性、版本、Schema、能力和编译/外部包可用性验证。
5. DNS 解析、TCP 可达性和本机监听端口占用验证。
6. 上游 TLS/mTLS 握手、SNI、hostname/IP 和证书链验证。

本任务的“验证成功”不发送业务报文，不验证 App 到 Proxy 的真实业务请求，不证明 Frame、Decode、
Rules、Encode、Server 响应或业务结果成功。每个层级独立报告，部分成功不得描述为完整成功，也不
得授权应用。

### 预览、确认与候选生命周期

- 验证成功后生成包含目标 Workspace、资源增删改、公开证书指纹和验证证据的精确预览。
- 候选与目标 Workspace revision、相关应用 generation、输入材料指纹和验证结果绑定。
- 应用调用必须提交该候选的一次性不透明令牌；令牌不是身份认证或用户在场证明。
- 候选可持续到本次 App 进程退出，不设置额外时间 TTL。
- 令牌使用一次后立即失效；目标 Workspace、相关运行态、输入、材料指纹或相关 Application
  generation 变化时立即失效，必须重新验证、预览和确认。
- App 退出、候选取消、验证失败或应用失败时清除全部未持久化私有材料。

### 运行态与原子应用

- 任一受影响 Listener 正在运行、正在启动/停止或仍有活动连接时拒绝应用。
- MCP 不得自动停止、启动或重启 Listener，不得中断活动连接。
- 受影响 Listener 停止后必须重新读取状态、重新验证、重新生成预览并取得新确认。
- Workspace、规则、协议包引用、Android 配置、证书引用和受保护材料必须作为一个 Application
  编排的原子操作应用。
- 任一步失败必须回滚已暂存的新材料和配置，原 Workspace 与运行态保持不变；不得用部分成功、
  默认值或旧实现回退继续。

## 明确接受的安全边界

- MCP 对所有网络接口开放，使用明文 HTTP。
- 不提供客户端身份认证、授权、访问令牌、来源 IP allowlist、TLS 或凭据撤销。
- 任何可达主机都可以读取 MCP 已公开的数据、提交包含私钥和密码的候选、执行技术验证、取得自己
  的候选令牌并确认修改 Proxy。
- 网络上的窃听者可能读取明文传输的证书、私钥、密码、配置和候选令牌。
- 预览确认、候选指纹和一次性令牌只防止误用旧候选或候选被修改，不构成安全访问控制。
- 实现、UI 和文档不得把该模式描述为安全远程管理、受信任调用者、身份确认或权限隔离。

## 不在范围

- 修改目标之外的 Workspace 或应用全局 Settings。
- MCP 客户端认证、授权、角色、token、mTLS、HTTPS、来源 IP 限制或审计身份。
- 从本机任意路径读取证书/私钥/配置，或从 URL 下载材料。
- 自动停止、启动、重启 Listener，或终止活动连接。
- 业务报文、交易、MAC、加解密、Frame、Decode/Encode 或业务响应成功验证。
- 未确认的重试、TLS 降级、明文上游回退、旧实现回退或验证失败后继续应用。
- Push、远程 CI、发布、部署或制品交付。

## 需求确认记录

| 轮次 | 用户确认 | 固化合同 |
| --- | --- | --- |
| 1 | 选择 A | 验证后先返回完整预览，明确确认后才原子应用。 |
| 2 | 选择 A | 受影响 Listener 运行或有活动连接时拒绝写入，不自动停止或重启。 |
| 3 | 用户明确补充 | 候选创建时由用户选择现有 Workspace 或新建 Workspace。 |
| 4 | 用户先选择单 Listener，随后覆盖 | 新要求扩大为规则及所有相关内容，配置完整链路。 |
| 5 | 选择 A | 完整链路以目标 Workspace 为边界，不修改其他 Workspace 或全局设置。 |
| 6 | 选择 B | 执行静态、DNS/TCP、TLS/mTLS、SNI/链、端口和协议包验证，不发送业务报文。 |
| 7 | 选择 A | 只接收 MCP 直接提交内容；确认前私有材料仅内存暂存，不读路径或 URL。 |
| 8 | 选择 C | 候选有效至 App 退出，但一次性且状态/指纹变化立即失效。 |
| 9–10 | 所有 IP、无需验证；随后选择 A | 所有接口开放且不做客户端身份认证；技术验证、预览和确认继续保留。 |
| 11 | 选择 B | 继续使用明文 HTTP，不增加传输 TLS。 |
| 12 | 用户明确“不要证据” | G034 不创建单次执行证据目录、不更新测试证据索引；保留源码测试、独立审查和本地提交，任务整体继续进行中。 |

## 未确认事项

无阻塞规划的产品需求。实现前的架构规划仍需把以下技术合同具体化，但不得改变上述产品决定：

- 全接口绑定在 macOS/Windows 上的 IPv4/IPv6 监听组合和端口冲突错误模型。
- 候选内存所有者、容量、逐候选/全局字节预算、并发和取消清理机制。
- 跨 SQLite Workspace 数据与系统受保护证书存储的原子提交/补偿边界。
- 完整链路候选 DTO、严格版本和 MCP tool input/output Schema。
- 外部协议包在线验证是否只检查现有状态，还是允许无副作用的 bounded health probe。

这些项目必须在架构/测试规格中给出单一答案；如果出现多个产品语义，不得由实现自行选择。

## 最小改动与最优设计

| 方案 | 修改范围 | 优点 | 风险或技术债 | 结论 |
| --- | --- | --- | --- | --- |
| 直接把现有 Tauri Command 暴露为 MCP 写工具 | MCP catalog/dispatch 加多个透传调用 | 表面 diff 小 | 绕过完整候选、跨资源原子性、运行态锁和秘密生命周期；形成多步部分成功 | 拒绝 |
| 在 Application 增加完整环境候选与原子应用用例，MCP 只做类型化适配 | Domain/Application/Infrastructure/MCP/测试/文档 | 单一业务合同，UI/MCP 可复用，错误、所有权和回滚可测试 | 改动较大，需要新 ADR 与严格回归 | 采用，属于最小正确且最优设计 |
| 通过应用 ZIP 导入替代候选模型 | 复用完整备份导入 | 可覆盖大量数据 | 语义是整应用替换，违反 Workspace 边界并扩大破坏面 | 拒绝 |

## 小任务列表

| ID | 小任务 | 依赖 | 可并行 | 负责人 | 状态 | 验收标准 | Commit | 小任务审查 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| T1 | 架构计划、ADR、安全边界和行为回归锁 | 无 | 否 | 主 Agent + 独立 Architect/Critic | 已完成 | 写入权限、候选、验证、原子性和明文远程风险形成可测合同 | 未创建 | 已执行；Architect16 与 Critic7 均 APPROVE，P0/P1/P2 为 0 |
| T2 | Domain/Application 完整环境候选模型与生命周期 | T1 | 否 | 主 Agent | 已完成 | 类型化候选、预算、一次性令牌、失效和清理测试通过 | `a185cc8d4300e3bd5fabff82c045896543964642`（G033）；`62c081441194519c80952ff7e46a47fcaebe4630`（G034） | G033、G034 均完成独立整体复审；最终 APPROVE，P0/P1/P2 为 0 |
| T3 | 分层静态、证书、网络和 TLS 验证编排 | T2 | 否 | 主 Agent | 待实现 | 六层结果独立、失败 fail-closed、无业务报文 | 未创建 | 执行，涉及 TLS/协议边界 |
| T4 | Workspace 完整链路原子应用与回滚 | T2、T3 | 否 | 主 Agent | 待实现 | 所有资源全成或全败，运行 Listener 拒绝，失败无残留 | 未创建 | 执行，涉及持久化和跨资源原子性 |
| T5 | MCP 全接口明文传输与写工具合同 | T2、T3、T4 | 否 | 主 Agent | 待实现 | 远程可调用、无认证、严格 Schema/预算、注解和错误合同正确 | 未创建 | 执行，涉及远程公共接口和安全 |
| T6 | 完整链路 E2E、UI/文档风险提示和证据 | T5 | 否 | 主 Agent | 待实现 | 现有/新 Workspace、证书、Listener、规则、包、Android 路径可复测 | 未创建 | 执行，涉及整体产品合同 |
| T7 | 整体对抗审查、修复、复验、提交和归档 | T6 | 否 | 主 Agent + 独立审查者 | 待实现 | 最终结论 APPROVE，P0/P1/范围内 P2 清零 | 未创建 | 强制整体审查 |

共享的候选 DTO、MCP Schema、Workspace 合同、证书生命周期和原子应用接口未稳定前不得并行实现。
只有独立测试/文档文件且不修改共享合同的工作，才可在主 Agent 明确所有权后并行。

## 测试计划

### MCP-CONFIG-CONTRACT-001：MCP 工具和远程明文边界

- 验证所有接口可达、无认证、HTTP 明文、写工具注解、严格 Schema、大小/deadline/并发限制。
- 证明现有只读工具不回归，并明确该测试不证明安全访问控制。
- G033 合同、DTO、Schema、fixture 与回归锁证据：
  [MCP-CONFIG-CONTRACT-001](../../testing/evidence/2026-08-26/TASK-20260825-006/MCP-CONFIG-CONTRACT-001/README.md)。
- 所有接口明文传输与远程调用仍由后续 G036/G032 验证；本证据没有提前声称运行时写工具可调用。

### MCP-CONFIG-CANDIDATE-001：候选生命周期与秘密清理

- 覆盖一次性令牌、App 退出清理、状态/generation/指纹变化失效、取消、超限、并发和错误路径。
- 断言私钥和密码不出现在预览、日志、错误、诊断和普通返回值。
- 计划证据：`docs/testing/evidence/<执行日期>/TASK-20260825-006/MCP-CONFIG-CANDIDATE-001/`。

### MCP-CONFIG-VALIDATION-001：分层技术验证

- 使用可归档证书、目标地址、协议包和规则 fixture，分别验证静态、证书、DNS/TCP、TLS/mTLS、
  SNI/hostname/chain、端口和包编译结果。
- 保存逐层预期/实际结果，并证明没有发送业务报文。
- 计划证据：`docs/testing/evidence/<执行日期>/TASK-20260825-006/MCP-CONFIG-VALIDATION-001/`。

### MCP-CONFIG-ATOMIC-001：原子应用与回滚

- 覆盖现有 Workspace 和新建 Workspace、每个持久化阶段故障、证书存储故障、SQLite 故障、
  stale token、运行 Listener 和活动连接拒绝。
- 逐字段比较应用前后 Workspace、引用、材料和运行态，证明失败时零残留。
- 计划证据：`docs/testing/evidence/<执行日期>/TASK-20260825-006/MCP-CONFIG-ATOMIC-001/`。

### MCP-CONFIG-CHAIN-001：完整 Workspace 链路配置

- 输入包含证书、HTTP/Socket Listener、TLS/mTLS、协议包、规则、故障和 Android 路由。
- 验证预览、确认和持久化恢复后的逐字段一致性；其他 Workspace 和全局设置逐字段不变。
- 计划证据：`docs/testing/evidence/<执行日期>/TASK-20260825-006/MCP-CONFIG-CHAIN-001/`。

### MCP-CONFIG-APP-001：真实 App 与 MCP 客户端冒烟

- 在打包 App 上从非 loopback 地址调用 MCP，创建候选、查看预览、确认应用并重启 App 检查恢复。
- 不发送业务交易；网络/TLS 验证只证明其对应层级。
- 计划证据：`docs/testing/evidence/<执行日期>/TASK-20260825-006/MCP-CONFIG-APP-001/`。

## 对抗审查计划

- T1–T5 均执行针对性独立审查，重点覆盖公共 MCP 写接口、明文远程风险、秘密生命周期、并发取消、
  stale token、跨资源原子性、错误传播和无额外兜底。
- 全部小任务完成后必须由未参与实现的独立 Agent 执行整体对抗审查。
- 整体结论必须为 `APPROVE`；全部任务相关 P0、P1 和范围内 P2 修复并复验后才能归档。
- 超出范围但有效的发现创建新任务，不在本任务中静默扩张。

## 文档影响分析

| 文档 | 当前判断 |
| --- | --- |
| `README.md` | 需要更新：能力列表和安全边界不再是只读/loopback。 |
| `docs/README.md` | 已登记；完成时移除 pending 入口。 |
| `docs/requirements.md` | 需要更新：MCP 写入、完整链路和远程明文合同。 |
| `docs/user-operation-guide.md` | 需要更新：候选、验证、预览、确认、取消和错误处理。 |
| `docs/onboarding-guide.md` | 需要更新：MCP 远程暴露和代码入口。 |
| `docs/architecture/decisions/ADR-004-embedded-read-only-mcp.md` | 需要标记为已替代并创建新 ADR；不得改写历史决定。 |
| `docs/architecture/security-and-persistence.md` | 需要更新：明确无认证明文远程写入和秘密边界。 |
| `docs/architecture/modules.md`、`data-flow.md` | 需要更新：Application 候选/验证/原子应用数据流。 |
| `docs/mcp/tool-reference.md`、`app-integration-guide.md`、`certificate-concepts.md` | 需要更新：写工具、材料输入、风险和复测。 |
| `docs/testing/release-validation-matrix.md` | 需要更新：MCP 配置合同、远程访问和完整链路用例。 |
| Android、规则和协议包文档 | 实现后逐项复核，按实际接口更新或记录无需更新。 |

## Skill 使用记录

- Skill：`oh-my-codex:deep-interview`
- 来源：oh-my-codex Codex plugin
- 版本：0.20.2
- 替代步骤：需求歧义梳理、边界压力测试、非目标与决策边界确认。
- 理由：原始需求同时涉及远程 MCP、秘密材料、完整 Workspace 配置、技术验证和原子 mutation，存在
  多个会改变实现的合理合同。
- 保留门禁：任务登记、权限/安全边界、测试证据、整体对抗审查、Commit 和归档均未被替代。
- 结果：11 轮确认完成，最终产品范围已写入本任务文档；`.omx/context/mcp-environment-configuration-20260825T124250Z.md`
  保存过程上下文，但本任务文档是项目内的权威需求记录。

- Skill：`oh-my-codex:ralplan --deliberate`
- 来源：oh-my-codex Codex plugin
- 版本：0.20.2
- 替代步骤：架构方案比较、Planner→Architect→Critic 共识门禁、ADR、失败预演和分层测试规划。
- 理由：本任务改变远程公共 MCP 写接口、秘密生命周期、跨资源原子持久化和运行态边界，属于高风险设计。
- 保留门禁：任务档案、用户已确认的无认证明文边界、测试证据、独立整体审查、Commit 和归档。
- 结果：已完成；经过 16 个 Planner 修订版本与连续独立 Architect/Critic 对抗复核，最终
  Architect16 和 Critic7 均为 `APPROVE`，P0/P1/P2 为 0。共识产物明确 DTO、状态机、错误码、
  受影响资源、秘密生命周期、租约、SQLite 原子提交、全接口明文无认证传输和证据门禁；生产源码
  在共识完成前未修改。

## 实施记录

### 2026-08-25 21:58:53 +08:00 — 任务登记

- 主 Agent 扫描 pending、completed 和当日测试证据，确认已使用 `TASK-20260825-001` 至
  `TASK-20260825-005`，分配并预留 `TASK-20260825-006`。
- 创建本 pending 任务文档，并在 `docs/README.md` 增加唯一入口。
- 本轮只登记任务，不修改生产源码、配置、脚本或测试。
- CI：未执行；未 Push；未提交。

### 2026-08-25 22:02:37 +08:00 — 登记验证

- 精确扫描确认 `TASK-20260825-006` 只有本任务文档一个任务所有者。
- `docs/README.md` 中本任务入口数量为 1，目标文件存在且路径可解析。
- 任务信息字段和治理规范要求的章节全部存在。
- `git diff --check` 对本任务两个登记文件通过。
- 第一次唯一性检查的 shell 正则包含反引号并被错误解释，该次结果作废；使用纯字面量重新执行后
  `REGISTRATION_CHECK=PASS`，未因失败检查修改文件。

### 2026-08-26 00:05:01 +08:00 — T1 启动

- 按 `docs/README.md` 待实现任务顺序，在 TASK-007 归档后启动本任务。
- 使用 `oh-my-codex:ralplan --deliberate`，先生成架构 PRD、测试规格和 Architect→Critic 共识证据。
- 规划阶段只允许读取仓库和写入 `.omx/context`、`.omx/plans`、`.omx/specs` 规划产物，不修改生产源码。
- CI：未执行；未 Push。

### 2026-08-26 06:31:44 +08:00 — T1 架构共识完成

- Ralplan 最终版本为 revision 16；权威 PRD、测试规格和共识文件位于 `.omx/plans/`。
- 明确采用 Application-owned candidate/lease 生命周期、Infrastructure-owned protected-material
  preparation 与唯一 `EnvironmentCommitPort` SQLite `IMMEDIATE` transaction；MCP 只负责严格传输和
  Schema 适配。
- 固定 v1 完整 DTO 与 tagged wire、封闭状态/错误/Warning 字面量、受影响资源 diff、既有规则身份、
  apply/cancel/shutdown 线性化、包状态优先级、terminal result 有界保留和 Android 非 idle 拒绝合同。
- 固定所有接口、明文 HTTP、无认证、无 Host/Origin/来源 IP/CIDR 门禁；协议错误仍按协议正确性拒绝。
- Architect16 独立审查：`APPROVE`，P0/P1/P2 为 0。
- Critic7 独立审查：`APPROVE`，P0/P1/P2 为 0；Ralplan technical consensus complete，可进入 G033。
- 规划阶段未修改生产源码、测试或运行配置；生产测试不适用，未执行。
- CI：未执行；未 Push；未创建 Commit。

### 2026-08-26 08:35:18 +08:00 — G033 合同、DTO 与回归锁完成

- 先由独立测试工程师建立 RED：HTTP 嵌套 unknown field、TerminalAction payload unknown field、
  sample-derived Schema 与公共 literal 漂移分别稳定失败；测试本身编译和 fixture JSON 均有效。
- Application 新增 `environment_configuration_candidate.v1` 的严格 DTO：所有 owned struct/tagged
  variant 拒绝未知字段；HTTP condition/action/terminal 使用 Application-owned strict wire 后再转换到
  Domain canonical 类型；Protocol Document 继续使用精确 `{type,value}`；WeakNetwork 强制完整对象、
  显式 null 和已确认数值边界。
- MCP 新增独立 contract registry 与 Rust Schema builder，五个工具的 input/output、terminal tagged
  union、WarningCode/ErrorCode、annotation 和公共 literal registry 均由可执行回归锁定；Schema builder
  与 checked-in golden snapshot 相互独立。
- 五个写工具未加入当前 active catalog、dispatch、server 或 backend；运行时继续保持 ADR-004 的
  loopback-only/read-only 实现事实，直到后续 G036 接入。
- ADR-008 接受最终全接口明文写入方向并明确分阶段启用；ADR-004 标记为被替代但保留当前阶段事实。
  架构文档门禁新增精确双向 supersession 校验，不能用任意 `Superseded` 字样绕过。
- 三轮独立审查先后发现并修复 10 项 P1/P2，包括完整 input/output Schema、strict nested serde、单一
  full-shape、新/既有 target fixture、selector optional/null、WarningCode/ErrorCode 分离、literal 双向
  coverage、`mss_clamp` 正整数与 lint suppression；最终结论 `APPROVE`，P0/P1/P2 为 0。
- 证据归档：
  [MCP-CONFIG-CONTRACT-001](../../testing/evidence/2026-08-26/TASK-20260825-006/MCP-CONFIG-CONTRACT-001/README.md)，
  32 个文件，7/7 活动 fixture 字节一致，29 个 G033 文件精确补丁 0 missing/0 extra 且 reverse-apply PASS。
- 本地实现提交：`a185cc8d4300e3bd5fabff82c045896543964642`。
- CI：未执行；未 Push。

### 2026-08-26 11:36:39 +08:00 — G034 Application 候选生命周期完成

- 先以 50 项外部 RED 合同锁定候选状态、容量、一次性 token、FIFO apply、取消、shutdown、终态保留、
  私密材料清理和进程内生命周期；独立审查发现公开 transition 可伪造、shutdown drain 丢通知、诊断
  可携带秘密和 public `target_key` 漂移后，新增 6 项回归并把测试机械迁入 crate 内。
- Application 现在唯一持有 candidate registry；验证完成、worker claim、invalidate 和 shutdown transition
  均为 crate-sealed。公开 DTO 只允许序列化，不能从外部伪造 snapshot、diagnostic 或 committed receipt。
- New target 内部容量 identity 使用 trim 后 exact UTF-8 十六进制，保持大小写和 NFC/NFD 差异；公开
  `target_key` 继续符合 G033 权威合同 `new:store lab`，且不信任 wire 中提供的 key。
- apply queue 为 Application-owned FIFO；未完成 work guard 的 Drop/panic 以 `COMMIT_FAILED` 收尾；
  shutdown drain 在 registry 锁内发布计数，并由确定性双 worker 逆序测试覆盖丢通知窗口。
- token 与未提交材料使用现有 workspace `zeroize`；token 消耗标记保留至终态淘汰；diagnostic message
  仅由注册状态码映射到稳定安全文本，私钥、密码、PEM 和 AQID 不进入 status、Debug 或序列化诊断。
- 三轮独立整体复审最终结论：代码审查 `APPROVE`、验证 `VERIFIED`，P0/P1/P2 均为 0。
- staged-only 临时 worktree 验证：lifecycle 56/56、Application 305/305、strict Clippy 和 Rust fmt 全部通过；
  临时 worktree 已删除。
- 本地实现提交：`62c081441194519c80952ff7e46a47fcaebe4630`。
- 按用户明确要求，本切片不创建 `MCP-CONFIG-CANDIDATE-001` 证据目录、不更新证据索引；这不改变
  G035–G038 及任务整体仍待实现的状态。
- CI：未执行；未 Push。

## 修改文件

- `docs/tasks/pending/2026-08-25/mcp-environment-configuration.md`
- `docs/README.md`
- `docs/testing/evidence/README.md`
- `docs/testing/evidence/2026-08-26/TASK-20260825-006/MCP-CONFIG-CONTRACT-001/**`
- `docs/architecture/decisions/ADR-004-embedded-read-only-mcp.md`
- `docs/architecture/decisions/ADR-008-mcp-environment-configuration.md`
- `scripts/check-architecture-docs.mjs`
- `scripts/check-architecture-docs.test.mjs`
- `src-tauri/crates/application/src/environment_configuration/**`
- `src-tauri/crates/application/src/facade/environment_candidates.rs`
- `src-tauri/crates/application/src/requirements_tests/environment_configuration_candidate_lifecycle.rs`
- `src-tauri/crates/application/src/requirements_tests/environment_configuration_candidate_lifecycle/**`
- `src-tauri/crates/application/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/crates/application/src/lib.rs`
- `src-tauri/crates/application/tests/environment_configuration_contract.rs`
- `src-tauri/crates/application/tests/environment_configuration_document_contract.rs`
- `src-tauri/crates/application/tests/environment_configuration_negative_contract.rs`
- `src-tauri/src/mcp/environment_contract.rs`
- `src-tauri/src/mcp/environment_contract/schema/**`
- `src-tauri/src/mcp/mod.rs`
- `src-tauri/src/mcp/tests.rs`
- `src-tauri/src/mcp/tests/environment_configuration_contract.rs`
- `src-tauri/src/mcp/tests/environment_configuration_schema_contract.rs`
- `src-tauri/src/mcp/tests/environment_configuration_schema_contract/**`
- `src-tauri/src/mcp/tests/fixtures/environment_configuration_candidate_v1/**`

## 附加文件

- 需求访谈上下文：`.omx/context/mcp-environment-configuration-20260825T124250Z.md`（OMX 本地状态，
  不替代本任务文档）。
- 最终 PRD：`.omx/plans/prd-mcp-environment-configuration.md`。
- 最终测试规格：`.omx/plans/test-spec-mcp-environment-configuration.md`。
- 最终共识与实施交接：`.omx/plans/mcp-environment-configuration-consensus.md`。
- 最终 Architect 记录：`.omx/plans/mcp-environment-configuration-architect-review-16.md`。
- 最终 Critic 记录：`.omx/plans/mcp-environment-configuration-critic-review-7.md`。
- G033 可复现证据：
  `docs/testing/evidence/2026-08-26/TASK-20260825-006/MCP-CONFIG-CONTRACT-001/`。

## 验收结果

- 任务登记：PASS。
- T1 架构计划与回归合同：PASS；Architect16/Critic7 双 `APPROVE`，P0/P1/P2 为 0。
- G033 合同、DTO、Schema 与回归锁：PASS；第三轮独立审查 `APPROVE`，P0/P1/P2 为 0。
- G034 Application 候选生命周期：PASS；56 项 focused 与 Application 305 项全绿，第三轮独立审查
  `APPROVE` / `VERIFIED`，P0/P1/P2 为 0。
- 运行时写工具接入：未开始；五个工具仍未进入 active catalog/dispatch/server/backend。
- 候选生命周期已完成；租约、分层验证、原子提交和全接口传输未完成，继续后续 Goal。
- 整体对抗审查：未执行。

## 测试结果

- 任务 ID 唯一性、README 唯一入口、路径、必填字段、章节和 diff：PASS。
- 架构规划审查：Architect16 与 Critic7 均 `APPROVE`；最终 revision 16 无待解决 P0/P1/P2。
- Application：211 个单元测试及 14/7/5/12 个合同/集成测试全部通过，共 249/249。
- G034 staged-only：candidate lifecycle 56/56；Application all-targets/all-features 305/305；strict
  Clippy `-D warnings`、Rust fmt、architecture、source-size、`git diff --check` 全部通过。
- MCP：31/31 相关测试通过；现有 read-tool 256 KiB 输入、8 MiB 输出、8 秒 deadline 未改变。
- strict Clippy（Application + Tauri all-targets/all-features，`-D warnings`）、Rust fmt、architecture、
  source-size 与 `git diff --check`：PASS。
- 活动 fixture 7/7 与证据快照逐字节一致；精确变更补丁 29 sections、0 missing、0 extra、
  reverse-apply check PASS。
- 独立审查：第三轮 `APPROVE`，P0/P1/P2 为 0。

## CI 情况

- 远程 CI：未执行，用户未授权。
- Push：未执行。

## 完成总结

未完成。任务登记、T1 架构共识、G033 合同/DTO/Schema/回归锁和 G034 Application 候选生命周期
已经完成；下一步进入 G035 分层验证与租约编排。后续仍需完成原子提交、全接口 MCP 接入、完整 App
验证、整体对抗审查和归档。G034 单次执行证据按用户明确要求未创建。
