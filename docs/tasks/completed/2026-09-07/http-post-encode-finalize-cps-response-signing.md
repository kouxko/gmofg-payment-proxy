# TASK-20260907-001：Proxy 本地 Wasm HTTP 上下行 Finalize

- 任务 ID：TASK-20260907-001
- 状态：已完成
- 任务日期：2026-09-07
- 创建时间：2026-09-07 13:59:23 +08:00
- 开始时间：2026-09-07 15:23:10 +08:00；用户授权执行，先完成技术合同核验
- 最后更新时间：2026-09-07 16:57:33 +08:00
- 完成时间：2026-09-07 16:57:33 +08:00
- 创建路径：`docs/tasks/pending/2026-09-07/http-post-encode-finalize-cps-response-signing.md`
- 归档路径：`docs/tasks/completed/2026-09-07/http-post-encode-finalize-cps-response-signing.md`
- 关键词：HTTP、Finalize trait、Encode、Wasm WIT、请求关联、Mock、Schema100 兼容
- 任务优先级：高
- 优先级理由：涉及公共协议包 ABI、HTTP 写出生命周期、请求响应关联和跨模块合同，按高优先级完整流程执行；D18 已授权执行，D19 明确移除 CPS。

## 完成结论与现行合同（D23）

以下为最终实现和验收结论；后续 D22 至早期设计章节保留当时记录，已失效的“待确认/未实现/CPS”描述不作为当前状态。

- 已完成：Proxy 本地 Wasm HTTP upstream-finalize / downstream-finalize，两方向独立可选；旧包不需要重打包。新增独立 HTTP `Finalize` trait，遵循现有 HTTP Pipeline 能力与写准备风格。
- 普通路径：Rules → 条件性 Encode → 正文动作 → Finalize → 当前方向生命周期提交 → 既有 Writer。未命中规则、正文不变、Encode 跳过均不跳过存在的方法。legacy Mock 复用同一 Finalize 实现，不重跑 Rules/Encode；LocalServer 保持普通响应准备流程。
- 输入输出：单个 JSON string，必填五键 url/method/code/header/body；header 为 string 值对象，重复名大小写不敏感且最后值生效；body 为 string。upstream code=null，method 与 scheme/host/port 不变，只允许 path/query 改动；downstream 原请求 url/method 不变，code/header/body 为响应。
- 输出先完整校验，候选 Message 原子写回。未变头保留原始重复项和字节；正文复用现有 codec/framing；不自动改变故障注入。返回原样输入是包内业务不适用的显式行为；Host 不吞 guest error/trap、取消、离线或非法返回。
- Finalize 失败不进入 Writer，不提交当前方向规则生命周期；ErrorStage 与外部包调用详情贯通观察链。已完成的另一方向不回滚。guard 保留至本轮 Finalize 与提交结束。
- 数据库：保持 Schema100、旧 Manifest 和存储结构；重开旧库后 Workspace/选中项/revision、包字节、元数据及 disabled 状态逐字段保留；旧包 Decode/Encode 正常。pre-100 升级、取消既有清空政策属于未确定的更广迁移范围，本任务未修改、未宣称验收。
- 不在范围：CPS 及签名包、远端 RPC Finalize、Socket Finalize、真实支付、安装、提交、推送、CI 与发布。曾创建的 CPS 草稿已按 D19 移除，没有 CPS 交付物。

### 需求就绪与原因核验

D18 于 2026-09-07 15:23:10 +08:00 授权进入实现；D19 收窄为 Proxy；D20 确认 Header；D21 校正独立 trait；D22 要求详细架构文档。现行功能输入、输出、状态转换、错误行为和验收条件均已确定。具体样例：POST /before?value=before 与 `"before"` 经 upstream 变为 /upstream?value=upstream 与 `"upstream"`；原 200 响应经 downstream 成为 202 和 `"downstream"`，App 收到正常 HTTP。

本任务为新增能力；原始事实是旧 WIT 和运行时没有 Finalize，HTTP 只在既有 Encode/正文动作后写出。D21 发现首次实现将方法附在已有 trait，缺少独立能力端口，原因是能力抽象未按用户约定落位；已以独立 trait 和正常/Mock 共用调用实现修正，并经顺序、次数、事务、wire 测试核验。未采用重试、绕过失败或双 Pipeline 缓解。Schema100 兼容是当前已验证；pre-100 全版本迁移为未实现、未验收的范围。

### 最小改动与最优设计

| 方案 | 取舍与结果 |
| --- | --- |
| 最小修改旧必需 WIT、直接挂 Encode 尾部 | 会破坏旧包 ABI，并漏掉 Encode 跳过及 legacy Mock，拒绝 |
| 独立 HTTP Finalize trait + 旧 world 可选导出 + 现有 actor 写准备 | 采用；职责、codec/framing、运行时代和事务提交复用现有边界，无重复 Pipeline 或 RPC/Socket 扩域 |

具体依赖、所有权、错误模型、时序和替代方案见 [详细架构](../../../architecture/http-finalize.md) 与 [ADR-011](../../../architecture/decisions/ADR-011-http-finalize-capability.md)。

### 小任务实施与审查

| 小任务 | 负责人/依赖 | 结果与独立验收 |
| --- | --- | --- |
| S1 合同冻结 | 主 Agent；先于实现 | 完成：D18–D22，输入输出/可变字段/独立端口冻结 |
| S2 Wasm 合同与运行时 | runtime_contract；依赖 S1 | 完成：旧/无/单/双方向、错误签名/非函数、guest error/trap、运行时代替换停用取消 |
| S3 HTTP 接线 | http_contract；依赖 S1/S2 接口冻结 | 完成：独立 trait、五字段、原子写回、请求关联、事务和 Mock、实际 Wasm 字段测试 |
| S4 诊断/兼容/文档/wire | 主 Agent、runtime_contract；依赖接口冻结 | 完成：阶段映射与绑定、旧库/旧包、loopback 完整 HTTP 交换、架构说明/ADR/门禁 |
| S5 集成、审查、归档 | 主 Agent；依赖整批终态 | 完成：冻结实测源文件、集成验证、独立审查 APPROVE、资源和索引归档 |

原 S4 CPS 实现及相应签名验收因 D19 取消，由现行 S4 替代。共享源码所有权按模块分配，接口变动先协调；未并行修改未冻结的共享合同。批次内不单独开启形式化审查，因为接口修正仍需集成；全部子任务终态并集成验证后，由未参与实现的 finalize_overall_review 完成整体审查，67 个文件，P0/P1/P2=0，APPROVE。相关说明见 [审查记录](../../../testing/evidence/2026-09-07/TASK-20260907-001/http-finalize/outputs/overall-review.md)。无额外依赖新增。

### 验收与测试结果

- 本地相关 Rust 测试 53 项通过：Exchange 30、Component ABI 5、Infrastructure Finalize 10（包含 Schema100 1）、HTTP runtime 3、运行时生命周期 3、真实 HTTP wire 1、观察链 1。
- strict Clippy 四 crate all-targets、workspace/fixture fmt、lint、架构门禁、500 行门禁、Rust 绑定生成、源码 typecheck、diff check 通过。测试期间 57 个被测源码/配置资源 SHA256 未变化。
- 全项目 typecheck 未通过：5 个 TS2307 均在既有 TASK-20260906-001 历史证据副本；保存真实失败日志，源码检查使用临时配置排除证据，未改产品 tsconfig 或旧档案。未执行独立绑定确定性检查。
- 实际 wire 同一 path_id 的四份完整 HTTP 报文和 Content-Length/字段/字节比较 PASS；独立 single-connection origin、Listener 及返回方向关联明确，Listener/runtime/临时 DB 清理完成。TLS、真实设备、生产业务和跨平台未验证。
- 全部正式用例归并到 [http-finalize 证据](../../../testing/evidence/2026-09-07/TASK-20260907-001/http-finalize/README.md)，含环境、命令、源码/WIT/manifest/lockfile、实际组件、旧包、原始四份报文、SHA256、输出、比较、清理和复测入口。该用例下每组测试及日志有独立名称，无遗漏用例或占位文件。
- 复用关系：TASK-20260901-001 / json-pretty-wasm-example / docs/testing/evidence/2026-09-03/TASK-20260901-001/json-pretty-wasm-example/outputs/json-pretty-1.0.0.wasm；本次使用相同 SHA256 旧包，新增可选导出缺失与 Schema100 保留的断言，父档案未修改。

### 文档影响与同步结果

| 文档类别 | 决定与结果 |
| --- | --- |
| 架构、能力、数据流、协议包、ADR | 需要更新，已完成：独立 trait、次序、共享调用、所有权、错误、兼容、源码导航、验收矩阵；http-finalize.md 为权威，其他文档链接引用 |
| docs 入口、任务、测试证据索引 | 需要更新，已完成：详细架构/ADR 入口、完成记录、父子证据及本次资源链接 |
| MCP 外部包集成 | 需要更新，已完成：明确 local Wasm 适用与 RPC 不支持该扩展 |
| 根 README、操作/新人指南、Android 文档 | 无需更新：本次无启动、安装、用户操作或 Android 行为改变，开发者通过架构和外部包入口查阅 |
| 需求/测试矩阵/验证结果 | 本任务确认记录、架构验证矩阵和证据提供对应事实；无需复制另一份字段合同 |
| 示例 README、内嵌业务包、脚本说明 | 无需更新业务示例：旧包原 ABI 不变；新增测试 fixture 及构建参数已在架构/复测步骤说明；架构文档门禁同步 |

技能：rust-router 与相关所有权/并发/HTTP 指导用于实现核验，documentation-and-adrs 用于详细设计文档，独立审查使用 code-review 流程；版本/来源沿会话已读取 SKILL.md，未替代项目任务锁、就绪、证据、索引、CI 边界。没有创建新 OMX 持续运行状态，无需清理其他任务的 OMX 状态。

### 修改文件与附加文件

以下列出本任务源代码、测试、脚本及文档；其他 TASK-20260907-002 文件与索引项保留，未纳入本任务实现。

- `docs/README.md`
- `docs/architecture/README.md`
- `docs/architecture/data-flow.md`
- `docs/architecture/decisions/ADR-010-in-process-webassembly-protocol-packages.md`
- `docs/architecture/decisions/ADR-011-http-finalize-capability.md`
- `docs/architecture/exchange-pipeline.md`
- `docs/architecture/http-finalize.md`
- `docs/architecture/rules-and-protocol-packages.md`
- `docs/mcp/external-package-integration-guide.md`
- `docs/tasks/README.md`
- `docs/tasks/completed/2026-09-07/http-post-encode-finalize-cps-response-signing.md`
- `docs/testing/evidence/README.md`
- `scripts/check-architecture-docs.mjs`
- `src-tauri/crates/application/src/models/diagnostics.rs`
- `src-tauri/crates/exchange/src/error.rs`
- `src-tauri/crates/exchange/src/observation/failure.rs`
- `src-tauri/crates/exchange/src/pipeline.rs`
- `src-tauri/crates/exchange/src/tests/pipeline.rs`
- `src-tauri/crates/infrastructure/src/adapters/in_process_wasm.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/capabilities.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_finalize.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_finalize/tests.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/external_http.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/plain_json.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/socket_diagnostics/mapping.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/phase10_http_pipeline.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/phase10_http_pipeline/production_shape.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/phase10_http_pipeline/real_wasm_finalize_wire.rs`
- `src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime.rs`
- `src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs`
- `src-tauri/crates/infrastructure/src/adapters/pipeline/tests.rs`
- `src-tauri/crates/infrastructure/src/adapters/pipeline/tests/http_finalize_transaction.rs`
- `src-tauri/crates/infrastructure/src/adapters/pipeline/tests/socket_joint_transaction.rs`
- `src-tauri/crates/infrastructure/src/adapters/pipeline/tests/support.rs`
- `src-tauri/crates/infrastructure/src/adapters/protocol_package_runtime.rs`
- `src-tauri/crates/infrastructure/src/sqlite/tests/http_finalize_compatibility.rs`
- `src-tauri/crates/infrastructure/src/sqlite/tests/mod.rs`
- `src-tauri/crates/package-runtime/src/wasm_component.rs`
- `src-tauri/crates/package-runtime/src/wasm_component/finalize.rs`
- `src-tauri/crates/package-runtime/tests/fixtures/http-finalize/.gitignore`
- `src-tauri/crates/package-runtime/tests/fixtures/http-finalize/Cargo.lock`
- `src-tauri/crates/package-runtime/tests/fixtures/http-finalize/Cargo.toml`
- `src-tauri/crates/package-runtime/tests/fixtures/http-finalize/manifest.json`
- `src-tauri/crates/package-runtime/tests/fixtures/http-finalize/src/lib.rs`
- `src-tauri/crates/package-runtime/tests/fixtures/http-finalize/wit-invalid/protocol-package.wit`
- `src-tauri/crates/package-runtime/tests/wasm_http_finalize.rs`
- `src-tauri/crates/package-runtime/wit/protocol-package.wit`
- `src-tauri/crates/proxy/src/forward/tests/capability_sequence.rs`
- `src-tauri/crates/proxy/src/http.rs`
- `src-tauri/crates/proxy/src/http/capabilities.rs`
- `src-tauri/crates/proxy/src/http/contracts.rs`
- `src-tauri/crates/proxy/src/http/exchange_runtime.rs`
- `src-tauri/crates/proxy/src/http/exchange_runtime/endpoints/app.rs`
- `src-tauri/crates/proxy/src/http/exchange_runtime/endpoints/tests.rs`
- `src-tauri/crates/proxy/src/http/exchange_runtime/request_target.rs`
- `src-tauri/crates/proxy/src/http/exchange_runtime/tests.rs`
- `src-tauri/crates/proxy/src/http/finalize.rs`
- `src-tauri/crates/proxy/src/http/joint.rs`
- `src-tauri/crates/proxy/src/lib.rs`
- `src-tauri/crates/proxy/src/socket_relay.rs`
- `src-tauri/crates/proxy/src/socket_relay/processing.rs`
- `src-tauri/src/runtime_logs/exchange_ui_layer.rs`
- `src-tauri/src/runtime_logs/exchange_ui_layer/tests.rs`
- `src-tauri/src/runtime_logs/exchange_ui_layer/tests/finalize.rs`
- `src/generated/rust-types.ts`

附加文件：`docs/testing/evidence/2026-09-07/TASK-20260907-001/http-finalize/`，完整清单与 SHA256 见该目录 checksums.json；包括可执行复测脚本，不依赖仅存在于 /tmp 的资源。创建路径保留原任务 slug 以追踪早期讨论，最终标题/范围已移除 CPS。

### 完成总结与剩余边界

已实现并验收当前 Proxy 本地 Wasm HTTP 上下行 Finalize 范围，独立 trait 与详细架构交付完成，整体审查通过。测试证明本地运行与精确输入下的 HTTP 行为；全量 TypeScript 既有失败及 pre-100/设备/跨平台/业务未验收边界如上。未提交、推送、触发 CI、安装或发布。

---

## 历史需求与实施记录

以下原记录按当时语境保留，现行状态和范围以 D23 为准。

## 当前文档与集成要求（D22）

- 用户追加：“文档记得一起更新 要详细 架构设计”。这是既有文档交付的深度要求，未改变产品字段或执行范围。
- 已使用 documentation-and-adrs 技能（/Users/codin/.agents/skills/documentation-and-adrs/SKILL.md）补齐架构分层、独立 trait 与组合装配、上下行时序、Mock/Local 差异、请求关联、所有权与 guard、提交/取消、原子写回、错误与兼容、源码导航和验证矩阵。技能指导文档结构与决策说明；项目任务锁、证据和归档规则保持。
- docs/architecture/http-finalize.md 为详细合同权威；新增 ADR-011 记录设计选择、原因、后果和否决方案，扩展但不替代 ADR-010。架构门禁新增该文档、Mermaid 和 ADR 的检查。
- 两条实现线已经全部终态并冻结；已收集 ABI、HTTP 字段与 actor、Mock、真实 Wasm/HTTP wire、运行时代取消证据。主 Agent 完成最后集成检查与整体审查后才关闭任务。
- 最终 wire 使用合法 JSON 字符串正文，输入/上下游/App 实际四份报文及 path_id/端点/清理记录一同归档；旧版的纯文本 fixture 尝试失败不计为通过。

## 当前设计校正（D21）

- 用户指出：“我看没有定义Finalize trait 不是这样设计的吗”。现有实现已接通行为，但把 Finalize 方法附在 HttpJointEvaluation / HttpMessagePrepare 上，没有独立能力端口；与用户要求的现有 HTTP Pipeline 风格存在偏差。
- 调整边界：补齐独立 Finalize trait 和本地 Wasm HTTP 实现，由正常写准备与 legacy Mock 复用同一能力；保留 Encode→正文动作→Finalize→当前方向生命周期提交→Writer 顺序。不得为了抽出端口提前提交事务、丢失原始 Message 字节、增加另一条 Pipeline 或扩大到 Socket。具体所有权接线在同一批实现内统一协调。
- 已有产品合同不变；旧 trait 接线形状的验收失效，新增独立能力端口、正常与 Mock 复用、Finalize 错误不提交/不写出的验收。现有行为测试继续作为回归保护。
- 本任务数据库兼容以 Schema100 和原有包/Workspace 持久化结构不变为当前验证对象；已新增重新打开数据库并逐字段保留旧包及 Workspace 的测试。跨历史 Schema 的迁移/取消旧库清空策略仍未获明确范围，不默认实施。
- 需求就绪：Finalize 产品输入、输出、可修改字段、每次调用、Mock、旧包、失败语义已清零；D18 授权进入实现，D20 关闭 Header 决定，D21 校正能力抽象。下方设计稿均为历史，不再作为当前未确认项。

## 最新确认（D20）

- 时间：2026-09-07 15:39:45 +08:00。用户：“重复header肯定是按照json标准 最后一个生效  数据库需要有兼容性了”。
- Header已就绪：header为普通JSON对象，value仍为string；同名重复头按最后一个值投影，不采用string[]。HTTP头名按现有大小写不敏感语义识别。JSON重复成员使用用户明确选择的last-wins语义；这是一项任务合同，不把它推广为所有JSON实现的强制保证。
- 写回复用既有Message与SetHeader合同：未变化的头保持原始字节与重复项；变化的同名头替换为返回的单值；删除键删除该名字所有项。完整输入原样返回时不重建Message，避免丢失无关的重复头/空白或正文编码。
- Header依赖解除，HTTP实现及测试继续。测试覆盖重复同名、大小写变体、最后值、原样返回和差量改写。
- 数据库兼容新增要求正在确认范围：已向用户询问是否要求后续升级保留全部旧数据、版本迁移并取消自动清空旧库。当前源码schema.rs基线及当前版本均100；core.rs保留当前100，低于100的有效旧标记进入清空重建分支。Finalize本身不新增数据库表/字段、不更改Manifest持久化格式，可保持Schema100。尚未擅自改写旧版迁移或清库策略，也未操作用户真实数据库。
- 当前数据库需求不阻止已确认的Finalize独立实现；若用户确认扩大到历史版本迁移，须登记明确范围、旧版本与资源，并以独立测试保护现存数据。

## 当前交付范围（D19，替代此前 CPS 范围）

- 用户最新要求：“我们不考虑cps的相关任务 只完成proxy支持上下行Finalize”。记录时间：2026-09-07 15:28:44 +08:00。
- 当前目标只包含 Proxy 本地 Wasm HTTP 的 upstream-finalize / downstream-finalize、五字段 JSON、不可变方法与目标服务、可改 path/query、返回 code/header/body 写回、每次调用与 Mock、旧包兼容、错误观察、回归测试和文档归档。
- 移出范围：CPS 协议包、MD5/签名算法、CPS Java 对照和业务验签。刚新增的 gmofg_cps/Cargo.toml、manifest.json、src/md5.rs 已确认均为本会话创建并撤回，未影响其他示例。原任务路径保留以维持现有讨论链接；标题和索引按最终交付改写。
- 输入输出仍只有 url/method/code/header/body；downstream URL/method 为当前交易的原始请求关联，无 CPS 特有逻辑。方法与scheme/host/port不可变、仅upstream path/query可变；只支持本地Wasm。
- 已验证的技术缺口：原始HTTP支持重复头，当前只有用于展示的 array-value JSON投影，没有 string-value 的无损写回合同。已提出单值string、重复值string[]并等待用户确认；依赖该项的实现暂停，ABI/运行时和其余接线继续。
- 失效验收：CPS/Java/MD5/支付字段和包制品取消。替代为通用Wasm fixture：none/up/down/both、URL/path/query与method、header/body/code、空/未修改输入、Mock、错误签名/trap/取消以及实际HTTP字节证据。
- 小任务调整：S2运行时由 runtime_contract 实施；S3 HTTP接线由 http_contract 实施；S4原CPS包取消，主Agent处理错误阶段/观察模型/文档和整体验证。S5仍在所有实现集成后整体审查并归档。无提交、CI、发布或安装。

## 当前执行状态（D18）

- 用户明确授权：“执行”。授权时间记录为 2026-09-07 15:23:10 +08:00。
- 现行产品合同：D1–D17；本地 Wasm HTTP，两独立可选 Finalize，单 JSON 输入输出，字段 url/method/code/header/body；method 和 URL scheme/host/port 不变，upstream 可改 path/query；downstream 保留 App 原请求 token 关联；业务不适用原样返回，执行故障传播。
- 状态进入进行中，旧章节“未授权/待整体确认”保留为历史，不再阻止已授权的编辑、测试、文档和审查。本次无提交、推送、CI、安装或真实支付授权。
- 就绪检查：目标、范围、五字段输入输出、每次调用/Mock/跳过行为及具体重签示例已确认；验收矩阵见 D15–D17。技术核验先关闭 Header 投影及可选 ABI 的具体接线；出现无法由现有合同决定的行为才暂停对应分支，不暂停独立可实施工作。
- 当前事实：原任务分支仍在；工作区存在另一 Payment DLL 任务源码、fixture、测试和产物改动，作为无关修改保留。任务管理与测试证据不得覆盖其目录或索引。
- 最小方案：保留旧 http-package 必需 ABI，以可选 export 扩展本地运行时；复用现有 HTTP transaction、写准备、codec/framing 和错误边界；新增独立 CPS 示例包。最优职责与该方案一致，不采用全局 token 缓存、另一个 Encode、双套 Mock 签名或整个 HTTP 编码体系重写。
- 分工：主 Agent 负责合同冻结、任务管理、集成、测试证据与交付；runtime_contract / http_contract 当前只读调查独立范围，待共享接口稳定后分配实现；CPS 包可在接口冻结后独立实施。整体审查在全部实现集成后由独立 Agent 执行。
- 流程：直接执行与原生子 Agent 协作，使用 rust-router 及 ownership/concurrency/HTTP/fintech 指导；不启动或覆盖工作区其他 OMX 工作流状态。读取 ralph 后确认本会话不启动 OMX tmux 运行时，采用同等持续实现与验证步骤。
- 历史读取：已复核 Component-only 包交付历史；当前源码为最终依据，历史构建和设备状态不记为本任务验收。

## 当前参数设计（以 D11–D14、D16、D17 为准）

Finalize 接收当前方向 Encode 及规则处理后的 HTTP 信息，返回修改后的 HTTP 信息 JSON；JSON 只包含平铺的 `url`、`method`、`code`、`header`、`body`。`header` 为 HTTP 头名到值的 JSON 对象，不使用 name/value 条目数组。Host 用返回信息构造实际 HTTP 消息，JSON 包装本身不是发给 App 的正文。downstream 处理响应，upstream 处理请求；可选方法兼容旧包，存在则每次调用，包含 Mock，不适用原样返回。此前只返回 Body、多层结构及只读 context 的方案保留为讨论历史，已由后续确认替代。当前仍仅进行设计，具体 ABI 签名尚未冻结。

当前范围补充（D16、D17）：仅支持本地 Wasm；upstream URL 的 scheme/host/port 保持不变，允许修改 path/query；method 保持不变。downstream URL 保持本次 App 请求 token 的关联语义。已询问的产品边界均已回答，仍等待用户对完整任务的实施确认；重复 Header 等技术合同须先核对既有实现，存在实质缺口时再明确记录。

## 完整任务确认稿（D15）

- 整理时间：2026-09-07 15:09:08 +08:00
- 用户要求：“列出完整任务等我确认”。本节汇总 D1–D14 的现行结论；标明建议和未决项，不把此次整理视为实施授权。状态保持待确认，确认前不修改产品源码或测试。

### 目标与范围

在 HTTP 请求与响应的业务内容准备完成后，增加独立、可选的 Finalize 扩展。CPS 协议包利用当前响应正文和本次 App 请求 URL 中的 token 重算 data.sign；Proxy 负责通用 HTTP 编排，包负责 CPS 签名算法。

范围包含 HTTP 包合同与运行时、上下行调用链、当前交易请求关联、Header/Body/状态码写回、CPS 协议包与可导入产物、测试、文档、整体审查和证据归档。Socket、CPS Android 源码改动、运行中配置/包安装、真实支付、提交推送、CI 和发布均不在本次范围；真实设备验收不得由本地测试代替。

### 已确认的行为与数据

- upstream-finalize 与 downstream-finalize 独立可选，旧包无需修改或重新打包。
- 存在的方法在该方向每次写准备时调用一次，不受规则是否命中、正文是否变化或 Encode 是否执行影响。
- 正常远端响应、本机应答及 Mock 均覆盖；保持既有规则次数，不另加 Mock 开关。
- 输入与返回均为平铺五字段 HTTP JSON：url、method、code、header、body。Header 为头名到值的 JSON 对象；Body 为字符串。不增加 original/current/request/response 包装或 Proxy 元数据。
- upstream：url/method/header/body 描述当前请求，code 为 null。downstream：url/method 关联本次请求，code/header/body 描述当前响应。
- Finalize 返回修改后的 JSON；Host 构造正常 HTTP 消息写出，App 不接收五字段包装层作为正文。
- 业务信息不足或不适用时，返回进入 Finalize 前的完整输入，保留规则和 Encode 的改动。Wasm trap、取消或错误函数签名等执行故障继续传播，不作为业务跳过。
- CPS 使用 App 原请求 token，不根据修改后的响应金额重新生成 token；按既有 Java 模型签名算法计算，更新 data.sign。

### 接口建议与流程

建议取消重复的独立 body 参数，两个可选方法分别接收一个 HTTP JSON 字符串、返回同结构 JSON 字符串：

```text
upstream-finalize(http-json: string) -> result<string, package-error>
downstream-finalize(http-json: string) -> result<string, package-error>
```

此为待确认的语义签名，不代表已完成 WIT/Wasmtime 兼容验证。现有 Decode/Encode/Display 合同保持，Encode 的既有职责仍为正文编码，Header 等 HTTP 信息由 Host 组装。

```text
该方向 Rules / 条件性 Encode / 完成正文动作
  → 组装五字段 HTTP JSON
  → 可选 Finalize
  → 返回 JSON 写回当前 HTTP 消息
  → 现有编码、framing 和 Writer
```

Finalize 纳入当前方向写准备是技术建议：成功后才提交该方向相关规则生命周期；失败不发送旧签名正文，已成功的另一方向不回滚。故障注入保持既有语义，不借 Finalize 自动修复故意制造的异常。最终正文以准备完成和实际发送证据确认，不默认增加第二次 Decode/Display。

### 实施拆分与验收

| 小任务 | 内容 | 依赖与验收 |
| --- | --- | --- |
| S1 合同冻结 | 单 JSON 签名、字段写回、Header 投影、运行方式范围 | 用户确认未决项；不增加自定默认值 |
| S2 包合同与运行时 | 保留旧合同，发现并校验独立可选 Finalize 方法，传播执行错误 | S1；旧包、单方向包、双方向包与错误签名测试 |
| S3 HTTP 接入 | 请求关联、Rules/Encode 后调用、Mock 分支、返回消息写回及最终观察 | S2；调用一次、连续/并发请求不串关联、错误和 framing 测试 |
| S4 CPS 包 | 复用已确认 Java 规则、提取 token、响应重签、业务不适用原样返回、生成可导入包 | S1/S2 合同稳定后可分文件并行；Java 对照向量与包加载验证 |
| S5 集成与交付 | 合并验证、受影响文档、整体对抗审查、测试资源与复测入口归档 | S3/S4 全部结束并集成后；审查通过且证据完整才归档完成 |

最低验收矩阵：旧包及两个方向独立可选；无规则/未命中/未变化/变化和 Encode 跳过时的调用次数；远端、本机应答及 Mock；返回 code/header/body 到实际 HTTP 消息的映射；业务跳过保留原输入与执行失败传播；CPS 修改金额/状态/嵌套字段后的 Java 签名对照、token 空格及 URL 编码；连续请求与并发关联；现有字符编码/framing/故障注入行为。测试保存实际输入输出、资源快照和 replay，归入本任务证据目录。静态检查和构建按受影响模块执行；未执行设备、真实 CPS 链路或 CI 不记为 PASS。

### 仍需冻结的边界

1. 是否采用上述单 JSON 参数和同结构 JSON 返回的具体接口签名。
2. 返回 url/method 的写回权限：downstream 的请求关联信息不等同于可改写已发送请求；upstream 是否允许修改 url/method 并影响实际上游请求尚未确认，不默认扩展路由能力。
3. header 对同名重复头的表达和写回应核对既有合同；不能无声覆盖、错误拼接或恢复被否定的 name/value 数组。若既有合同不足，应先提出具体例子再确认。
4. 远端 HTTP RPC 包是否与本地 Wasm 包一起扩展 Finalize；此前未确认，不默认增加或排除该调用方。

本节提交用户确认后停止实施推进。待确认事项不能因为用户未反对而视为批准；用户确认后只在已就绪范围内进入实现。

## 背景、目标与前后原因

用户先要求检查 `/Users/codin/Code/jp_gmofg_cps` 的通信校验逻辑，随后提出生成协议包，处理可能被规则修改的 response。源码确认实际需要更新的是响应 `data.sign`，验签依据是 App 本次请求保存的 token；响应模型没有独立 token 字段。

目标：允许 HTTP 响应业务字段经规则修改后，由 CPS 协议包依据最终正文和对应请求 token 重算签名，使返回 App 的报文能够通过现有验签。Proxy 提供通用上下文和调用阶段，CPS 业务算法由包负责。

### 本会话的需求与方案演变

| 顺序 | 用户关注点 / 当时方案 | 原因与后续变化 | 确认层级 |
| --- | --- | --- | --- |
| 1 | 检查请求内容生成 checksum 的逻辑 | 找到 `Message.calcSignWithToken`；名称是 sign，算法为带 token 的 MD5，非原始 JSON 字节 checksum | 当前源码已验证 |
| 2 | 生成协议包，修改 response 后处理校验 | 只拿响应 Body 不足：App 验签还需要原始请求 URL 中的 token | 用户需求；token/sign 含义由源码澄清 |
| 3 | 向 Encode 增加请求上下文 | 现有 Proxy 写准备阶段已有 method/path，但没有传给包；拟补齐上下文调用链 | 助手提出的候选方案，未实施 |
| 4 | 用户提出“新加一个方法呢” | 候选为 `downstream-encode-with-context`，避免直接改变旧 Encode 的参数；仍须明确 ABI 版本和旧包识别策略 | 用户提出新增方法；具体接口未确认 |
| 5 | 用户要求梳理 HTTP/Socket 调用流程 | 确认协议模式读阶段 HTTP 是 Decode→Display、Socket 是 Frame→Decode→Display；写阶段是 Rules→Encode→Writer；未修改 Document 会跳过包 Encode | 当前源码已验证 |
| 6 | 用户提出 HTTP 在最后 Encode 后再加方法 | 候选改为 Finalize：Encode 负责 Document→正文，Finalize 对最终正文计算签名；业务字段经 Encode 后已确定，职责更清楚 | 用户提出阶段位置；助手推荐该方向，详细合同待确认 |
| 7 | 用户要求“记录此任务 写明前后原因” | 保存上述演变、事实、候选接口及未决边界，供后续确认和实现 | 已授权本次登记，未授权默认补全行为 |

前方案把编码与响应重签放在同一 Encode 方法；后方案将最终派生字段处理放到独立 Finalize 阶段。后方案仍需要请求上下文，单独增加方法名称不能解决 token 来源问题。新增方法也不自动保证旧 Wasm ABI 兼容。

## 现象、预期、根因与证据分级

- 实际现象：用户预期会修改 response 字段；本次没有抓取实际修改后验签失败的报文，没有声称已复现线上缺陷。
- 预期行为：修改参与签名的响应字段后，返回 App 的 `data.sign` 与 App 按当前响应及本次请求 token 算出的值一致。
- 当前已验证：CPS 的 `BaseNetUtil.checkSign` 使用 `mTokenInRequest`，而不是依据新响应金额重新生成 token。
- 当前已验证：token 由商户号、终端号、时间戳和金额拼成，存在右补空格，并被放在请求 URL 路径中。
- 当前已验证：HTTP Wasm Decode 只接收正文；Encode 只接收 original-input 和 document-json，没有请求上下文。
- 当前已验证：`BufferedHttpPrepare` 从当前 HTTP transaction 构造 `HttpRequestMetadata`，`ExternalHttpPrepare` 已收到该参数，但未把它传给包 Encode。
- 当前已验证：HTTP 与 Socket 的外部包路径在 Document 未变化时跳过 Encode；Display 当前在 Rules 之前，失败只影响展示并记录错误。
- 当前已验证：HTTP mocked 响应在 `BufferedHttpPrepare` 存在跳过 downstream prepare 的分支。
- 推断：修改签名参与字段而沿用旧 sign，通常会使验签失败；实际结果需用 CPS Java 算法和具体向量验证。
- 已确认的能力缺口：响应最终处理缺少对应请求上下文，包仅凭响应 Body 无法获得本次请求 token。这是接口能力不足，不是已确认的生产故障根因。
- 未知：真实 CPS 响应样本、待修改字段范围、未修改/Mock/异常响应的 Finalize 策略、ABI 迁移策略。
- 复现计划：固定请求 token 和响应向量，改动金额或状态，分别验证旧签名失败与重签成功；本次尚未执行。

当前权威源码定位（后续实现前重新核验）：

- CPS：`/Users/codin/Code/jp_gmofg_cps/app/src/main/java/jp/gmofg/cps/https/msg/Message.java`，`calcSignWithToken`、`getSortedNameValuePair`、`isEmpty`。
- CPS：同目录工程 `https/executor/BaseNetUtil.java`，`generateToken`、`checkSign`；`PayNetUtil.generatePayUrl`。
- Proxy：[WIT](../../../../src-tauri/crates/package-runtime/wit/protocol-package.wit)、[RPC 参数](../../../../src-tauri/crates/package-contract/src/rpc.rs)。
- Proxy：[HTTP transaction 写准备](../../../../src-tauri/crates/proxy/src/http/exchange_runtime.rs)、[HTTP 请求元数据](../../../../src-tauri/crates/proxy/src/http/contracts.rs)。
- Proxy：[外部 HTTP 包适配](../../../../src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/external_http.rs)、[联合规则与编码](../../../../src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs)。
- Proxy：[通用 Pipeline](../../../../src-tauri/crates/exchange/src/pipeline.rs)、[Exchange](../../../../src-tauri/crates/exchange/src/exchange.rs)。

## 候选范围与不在范围

候选范围：HTTP 可选 Finalize 能力、请求上下文透传、CPS 响应签名包、受影响运行时合同及验收证据。当前仅实现本任务登记。

不在本次授权范围：源码实现、自动修改运行中 Listener/规则、安装协议包、真实支付交易、改 CPS Android 代码、Socket Finalize、任意共享变量机制、提交/推送/CI/发布。Socket 本次只作为已有流程对照，不默认扩展其 ABI。

## 需求确认记录与未确认事项

已确认：用户提出 HTTP Encode 后独立方法的方向，并要求记录原因；需要支持修改 response 后处理签名。未将助手建议当成完整合同批准。

待确认（任一项会影响实现，不能默认选择）：

1. Finalize 在 Document 未变化、Encode 跳过时是否执行；助手曾建议执行，尚未确认。
2. Mock、LocalServer、远端正常/错误响应分别是否执行；尤其现有 Mock 跳过 prepare，不保证自然经过新阶段。
3. 仅 downstream 还是双方向接口；本次 CPS 需求只证明 downstream 用途。
4. 上下文具体字段与可见阶段；候选为只读原始请求 method/requestTarget，是否需要改写后的请求信息另行确认。
5. 包能力声明、WIT 扩展或版本升级、旧包加载及远端 RPC 同步范围；不得新增必需 export 后直接声称兼容旧包。
6. CPS 支持的接口与字段模型、非法 JSON/缺少 data/token/未知字段处理、字符编码及签名规则的准确适用范围。
7. 签名完成后的展示/最终 Document 与实际 encoded/sent 证据如何关联，避免 UI 旧 sign 被误当成最终报文。

现有错误传播原则继续有效：不得把 Finalize 失败伪装成功或回退发送旧签名正文；具体错误码及失败阶段仍需设计确认。

## 需求就绪检查

- 问题、用户目标：已说明。
- 范围：候选已列出，Mock/方向/ABI 边界待确认。
- 输入、输出、状态和错误：候选接口已描述，未形成已批准合同。
- 具体示例：已有源码级金额/状态修改场景；真实向量待补。
- PASS/FAIL 标准：以下为候选测试计划，随确认结果冻结。
- 会改变实现的未确认事项：尚未清零。
- 就绪结论：FAIL，保持待确认；进入实现时间：无。

## 最小改动与最优设计（讨论记录，尚未定案）

| 候选 | 改动与优点 | 代价/限制 |
| --- | --- | --- |
| Encode 增加 context | 利用现有编码位置，调用链较短 | 改旧签名涉及 ABI；编码与签名耦合；未修改时跳过 |
| 新增 encode-with-context | 原 Encode 形状可保留，能力显式选择 | 仍须版本化/能力识别；重签仍耦合 Encode |
| Encode 后独立 Finalize | 最终正文与派生字段职责独立，适合此次重签方向 | 增加阶段及错误/观察合同，需覆盖 Encode 跳过和 Mock 分支 |

用户最新讨论方向是第三项；正式方案比较及定案须在需求就绪后完成。

候选流程：

```text
Decode → Display → Rules → Encode（现有条件下可跳过）
  → Finalize(finalBody, readonlyRequestContext)
  → Body 字符编码 → HTTP 长度更新 → Writer
```

候选方法：`downstream-finalize(body: string, context: HttpContext) -> result<string, PackageError>`。名字、类型和序列化尚未批准。CPS 包取请求 token，按 Java 响应模型重算 `data.sign`；Proxy 不内置 CPS 的 B2B/MD5 算法。签名不直接覆盖所有未知 JSON 字段；token 的补空格不得 trim，路径不得重复解码。

## 小任务、文档影响与验证计划

| 小任务 | 依赖/并行 | 负责人 | 状态 | 验收标准 |
| --- | --- | --- | --- | --- |
| S0 登记方案演变 | 无；主 Agent 串行持锁 | 主 Agent | 已完成 | 唯一任务 ID、pending 入口、事实与建议分开 |
| S1 冻结需求及接口版本 | S0；不可并行变更共享合同 | 待分配 | 待确认 | 上述未决事项清零并记录确认 |
| S2 Proxy 上下文和 Finalize | S1 | 待分配 | 待实现 | 方向、关联、执行条件、失败与观察符合冻结合同 |
| S3 CPS 包与向量 | S1；接口冻结后才可与 S2 分文件并行 | 待分配 | 待实现 | 与 CPS Java 计算一致，输出可导入包 |
| S4 集成、审查与归档 | S2/S3 全部终态并集成后 | 主 Agent | 待实现 | 受影响层测试、整体审查 APPROVE、资源档案完整 |

文档影响：实现时同步包 ABI/WIT、外部包集成指南、架构数据流、用户包说明及测试矩阵；本次不修改已落地架构文档冒充新能力可用。

候选正式测试：金额/状态和嵌套字段重签；Java 模型的 null/空字符串/整数零/未知字段投影；token 空格与路径编码；多连接与连续请求不串 token；Encode 跳过；已确认范围内的 Mock/LocalServer；Finalize 失败不产生成功发送；旧包兼容；最终字节与 App 验签一致。以上未执行。

证据按 `docs/testing/evidence/<执行日期>/TASK-20260907-001/<用例ID>/` 归档，保存实际请求路径、token、响应原文、修改规则、预期/实际签名、逐字段比较、工具与资源快照和 replay。真实设备验收与纯算法/Host 集成结果分开报告。

对抗审查计划：高优先级任务在实现及集成验证后进行整体审查，重点 ABI、请求关联、未修改/Mock 分支、签名投影、错误和最终观察；本次仅登记，未执行实现审查。

## 实施记录、验收与完成总结

- 2026-09-07 13:59:23 +08:00：创建任务文档并加入 docs/README.md 待实现入口；未修改源码。
- 修改文件：本任务文档、`docs/README.md`。
- 附加文件：无。
- 本次登记验证：锁内检查任务 ID/路径/入口唯一性、文档必要章节、仓库相对源码链接；通过后释放锁。
- 正式测试用例/证据：尚无；本次仅登记，网络报文、截图、构建及运行测试为 N/A。
- CI 情况：未触发。
- 功能验收：未执行，任务待确认。
- 完成总结：只完成需求和方案演变登记，未实现 Finalize，未生成 CPS 协议包，不进入完成索引。


## 设计讨论 v0.1（2026-09-07，未批准实现）

### 本轮授权与分支

- 用户要求：创建对应 feature 分支，先写设计并讨论，暂不写实现。
- 分支：`feature/0907-001-http-finalize-cps-signing`，从当前 `develop` 创建；保留原有任务及索引改动。
- 本轮范围：只读源码核验、设计草案、需求讨论；不修改产品源码、测试或运行配置。
- 状态仍为 `待确认`；进入实现时间仍为无。下面所有“建议”均为候选，不等于用户确认。
- 流程：`oh-my-codex:deep-interview`（本地插件 0.20.2，quick）用于逐项澄清；项目任务档案及锁协议保持。需求未闭合，不进入实施计划或执行工作流。

### 1. 目标与职责

目标示例：同一请求携带 token T，响应某个参与签名的字段由规则从 A 改成 B；CPS 包计算新响应的 `data.sign`，App 仍使用本次请求保存的 T 验签。不得因为响应金额变成 B 而生成另一个 token。

建议职责划分：

| 所有者 | 责任 |
| --- | --- |
| HTTP transaction | 持有当前请求与响应的配对事实，提供不可变请求元数据，交易结束/失败时释放 |
| HTTP 写准备 | 在业务正文改写完成后编排 Finalize、字符编码与现有 HTTP framing 更新 |
| 协议包运行时端口 | 调用已声明且已校验的 Finalize 能力，传递正文、只读上下文与强类型错误 |
| CPS 包 | 识别已支持接口、提取 token、按 CPS Java 模型投影并计算签名、更新 `data.sign` |
| 通用 Exchange / Writer | 保持通用写准备与实际发送职责；无需识别 CPS、token 或 MD5 |

本轮不扩展 Socket，不改 CPS Android 客户端，不引入跨请求共享变量或全局 token 缓存；具体 CPS 接口集合、Mock 与 ABI 迁移仍须讨论。

### 2. 本次重新核验的源码事实

- `proxy/src/http/exchange_runtime.rs` 的 `HttpTransaction` 持有 method/uri，`BufferedHttpPrepare` 在当前交易内生成请求元数据；交易通过 `begin`/`complete`/`fail` 管理，不需要新建全局关联表。
- `proxy/src/http/contracts.rs` 的 `HttpRequestMetadata` 是 method 与 URI 的 `path_and_query()`；它不是原始完整 absolute-form 请求行，也不是已替包解码的 token。
- `infrastructure/.../joint_document.rs::encode_into` 在 Document 未变化时直接返回，包 Encode 不会执行。
- `infrastructure/.../pipeline/rule_runtime/actor.rs::prepare_evaluated_message` 先执行 joint Encode，再执行 `apply_rule_actions`；后者包含正文修改。其成功返回后 actor 才提交规则生命周期增量。
- `BufferedHttpPrepare` 在 downstream mocked 分支跳过当前 prepare；LocalServer 与这种 terminal Mock 不是同一个分支。
- `exchange/src/pipeline.rs::Write::write` 在 prepare 返回后记录 Encoded，Writer 成功提交后才有对应发送事实。新增 Finalize 错误需要显式定位，不能被现有 Encode 包装误标。
- `proxy/src/message.rs::replace_body` 已负责正文变化后的 framing：存在 Transfer-Encoding 时移除 Content-Length，否则更新长度。不能把任务早期“HTTP 长度更新”简化为无条件添加 Content-Length。
- `proxy/src/fault/response.rs` 在发送端还可能执行 InvalidJson 对应的 ReplaceBody、截断、错误长度等故障。Finalize 的“最终”应明确为业务正文处理终点；不应默认修复用户主动注入的故障。

### 3. 建议的阶段与事务边界

逻辑顺序候选（不代表当前代码已经这样实现）：

```text
当前请求元数据 ────────────────────────────┐
                                         ↓
响应 → Decode → Display → Rules → 条件性 Encode
     → 完成本轮正文修改 → Finalize(正文, 请求上下文)
     → 字符编码 / 现有 framing 更新 → 写准备成功
     → 提交本方向规则生命周期 → 故障注入 / Writer → 实际发送证据
```

建议 Finalize 纳入现有规则写准备事务：Rules、Encode、Finalize、字符编码中任一步失败，当前方向不提交该次规则生命周期增量，也不发送旧签名正文。已经成功提交的 upstream 请求不因 downstream 失败而倒退；该方案不是整个 HTTP 请求响应的数据库事务。

不能直接把 Finalize 接到 `runtime.encode_http()` 的返回处：后面可能还有正文修改。也不能放到 `apply_response_joint_policy()` 已返回之后却仍声称与规则提交原子，因为当时 actor 已经提交生命周期。

当前代码已在多个动作处产生 bytes。实现设计仍需比较：在现有编码后读取最终 Message 的文本并调用 Finalize（改动较小，但增加 codec 往返），或把文本定稿集中到一次边界（改动更大）。无论选择哪项，必须保护原始未修改字节、Shift-JIS/UTF-8 合同与 Content-Encoding 限制；不能顺带改写整个 HTTP 编码体系。先确认功能边界，再确定这个内部落点。

### 4. 候选接口、能力与观测

语义接口候选，非可直接发布的 WIT：

```text
HttpFinalizeContext {
  requestMethod: string
  requestTarget: string  // 当前请求 path + query，保留转义形式
}

downstream-finalize(body: string, context: HttpFinalizeContext)
  -> Result<string, PackageError>
```

- 建议上下文保持通用、只读、逐次调用传入；不把 `cpsToken` 变成 Proxy 公共字段。包按自身路径合同提取 token，保留原始 token 的补空格；百分号解码次数、加号语义须由实际 HTTP 客户端构造链和向量确认。
- 建议先考虑 downstream 可选能力；当前需求尚未证明 upstream Finalize 的用途。名称、字段和方向均未定案。
- 现有 HTTP WIT 只有 Decode/Encode/Display。不能直接加必需 export 后宣称旧包兼容。候选 A：保留旧合同并增加显式版本化扩展；候选 B：升级 HTTP ABI 并迁移全部受影响包。具体 Wasmtime 加载实现和兼容验证在 ABI 决策后核对官方合同，当前不声明其中任何加载方案已验证。
- 建议“未声明能力”由装配阶段明确不调用；“声明但缺失/加载失败/调用失败”必须报错，不能悄悄当成无能力包。远端 HTTP RPC 同步范围仍待定。
- 建议用 `Finalize` 失败阶段承载包身份、方向、方法和原始错误；需要同时检查 actor 与通用 Write 的错误包装，不能只新增一个枚举值。
- 建议保留现有读时 Display 和规则后 Document 的事实含义，最终重签正文由准备完成的 Encoded 与实际 Sent 报文证明；不能把旧 Display 的 sign 标成最终签名。是否增加最终解析展示需单独确认，不能默认再次 Decode/Display（会产生额外调用和失败语义）。

### 5. CPS 算法边界

本次对 CPS 源码的只读核验确认：

- `Message.java::calcSignWithToken` 计算 `MD5("{" + sortedFields + "&key=B2B" + token + "}")` 并输出大写十六进制；它依据 Java 模型字段，不是直接对响应 JSON 原文做 MD5。
- 字段包含父类字段并按名称排序；排除 sign、名字包含 shadow 的字段，以及 null 和 Integer 0。空字符串不等于 null；其他类型不能套用 Integer 0 的排除规则。
- 已识别模型递归处理嵌套对象；字段值拼接有去 ASCII 空格行为，而追加到 key 后的 token 不可 trim。集合/其他类型的 Java `toString()` 行为必须用原算法向量锁定，不能换成一般 JSON canonicalization。
- `BaseNetUtil.checkSign` 使用 `mTokenInRequest`；`generateToken` 包含商户/终端定宽补空格、时间与补零金额。
- 已找到 Pay、Confirm、Refund、Reverse 响应模型及错误体模型；源码存在不表示此次全部纳入交付。建议先确定接口集合，再固定对应投影与错误响应规则。

非法 JSON、缺 data/token/sign、未知接口与未知字段都不能由 Host 猜测。候选包策略必须逐类定义：签名所需数据确实缺失时报错，或对明确声明不处理的接口原样返回；二者适用范围仍需用户确认，不能把失败伪装成不适用。

### 6. 本轮优先讨论的决定

| 决定 | 建议供讨论 | 其他选择及影响 | 状态 |
| --- | --- | --- | --- |
| 正常响应在 Document 未改、Encode 跳过时是否调用 | 声明 Finalize 的包每次正常响应都调用，让阶段独立于 Encode 优化 | 只在修改后调用可保留未修改签名，但“修改”须覆盖 Document 与正文动作，而非仅判断 Document | 待确认 |
| 方向 | 先 downstream，契合 CPS 响应签名用途 | 双方向会扩大 ABI、上下文和验证范围 | 待确认 |
| Mock / LocalServer | LocalServer 走共同响应路径；Mock 单独定义调用一次的路径且不重跑原来跳过的 Rules | 排除 Mock 可缩小范围，但 Mock CPS 正文不会自动重签 | 待确认 |
| HTTP 错误与无正文响应 | 按接口与响应模型明确适用性 | 不能仅凭 HTTP status 判断 CPS 是否有合法待签名响应；HEAD/204 等也需明确 | 待确认 |
| 请求上下文 | 当前请求 method + 保留转义的 path/query，只读传入 | headers/body/改写后请求会扩大 API；现有 CPS 用途尚未要求 | 待确认 |
| ABI 与远端包 | 显式扩展能力并校验，旧包不隐式获得新行为 | 整体升级并迁移也可选，须明确受影响旧包范围 | 待确认 |
| 最终显示 | 保留阶段事实，最终报文可核对；不默认再调用 Decode/Display | 增加最终 Document 会增加调用及观测合同 | 待确认 |

首轮问题聚焦第一项。其余建议不会因为本轮未反对而自动成为需求。

### 7. 候选验收与设计验证

未来验收需包含：Java 原算法固定向量、真实支持模型字段修改后重签；token 空格/编码；连续请求和多连接不串 token；Encode 跳过、正文动作后重签；Finalize 失败不发送并回滚当前方向增量；明确范围内的 LocalServer/Mock；旧包与错误能力声明；framing 与 Shift-JIS/UTF-8；读时/规则后/最终发送证据分离；故障注入保持已选语义。

本轮仅验证设计文档、分支和当前源码依据。算法测试、正式实现测试、构建、真实 CPS 交易、设备、安装、CI 均未执行，不把设计草案记为功能 PASS。需求就绪检查仍不通过，完整实现方案比较、实施分片和正式整体审查在需求冻结后进行。

- 2026-09-07 14:09:16 +08:00：已完成本轮分支创建与设计草案，待用户讨论；无源码实现。


## 需求确认追加记录（设计讨论）

### D1：每次调用 Finalize

- 确认时间：2026-09-07 14:13:15 +08:00
- 本轮问题：声明 Finalize 能力的 HTTP 包，正常远端响应未被规则修改、Encode 被跳过时，是否仍调用 Finalize。
- 用户原话：“需要 每次都要调用”。
- 已确认合同：每次正常响应写准备都调用 Finalize；是否命中规则、Document 是否变化、正文是否变化以及包 Encode 是否执行，均不是跳过 Finalize 的条件。
- 原候选变化：v0.1 的“每次调用 / 仅修改后调用”二选一已经收敛；不再采用“仅修改后调用”。前文候选表保留讨论历史，以本确认记录为准。
- 实现影响：Finalize 的调用路径必须独立于 `encode_into` 的 unchanged 提前返回，并位于本轮业务正文修改完成之后。不能只把调用放在包 Encode 成功的分支内。
- 新验收项：未配置匹配规则、规则未命中、命中但结果值不变、Document 或正文实际变化四类响应，均须证明 Finalize 每次被调用一次；前几类允许 Encode 按现有合同跳过。调用次数与真实签名结果分别断言。
- 尚未由本轮回答确定：terminal Mock、HTTP 错误/无正文响应、方向扩展、上下文字段、ABI 兼容、CPS 接口集合与最终展示。首轮问题范围是正常远端响应，下一轮用 Mock 场景确认“每次”的边界。
- 需求就绪：仍未通过；仅更新设计，无源码/测试实现，进入实现时间仍为无。


### D2：Mock 也必须经过 Finalize；澄清当前与历史路径

- 确认时间：2026-09-07 14:16:49 +08:00
- 用户确认：“mock响应也需要处理 mock应该是规则的一部分呀 Finalize是在规则之后处理的吧”。
- 已确认合同：Mock 响应纳入 Finalize，每次响应业务正文经规则确定后都需要调用；不因为正文来自 Mock 而排除。不新增“Mock 是否 Finalize”的产品开关。
- 原候选变化：D1 未决项中的 Mock 调用边界已确认，v0.1 的“排除 Mock”选项失效。用户是在统一规则后处理语义下确认，不是要求另一套独立 Mock 签名流程。
- 本次当前已验证：`docs/architecture/data-flow.md:129` 明确新建/编辑已不暴露旧 `MockResponse`；当前本机应答使用 LocalHttpServer 产生响应，再由 Proxy → App 的 `ReplaceBodyText` 规则修改 Body。`application/src/facade/rule_capabilities.rs` 也排除 MockResponse 并保留 ReplaceBodyText。
- 本次当前已验证：底层旧 `TerminalAction::MockResponse` 仍由规则产生（`pipeline/rule_actions.rs:177`），运行端在请求动作阶段生成响应并设置 `transaction.mocked=true`（`proxy/src/http/exchange_runtime/endpoints.rs:419`）；`BufferedHttpPrepare` 的 downstream mocked 分支跳过当前 prepare（`exchange_runtime.rs:364`），App Writer 直接形成 Send（`endpoints/app.rs:149`）。这是一条真实存在的旧兼容特殊路径，不是当前 UI 本机应答规则的常规路径。
- 沟通纠正：上一轮提问没有先区分当前本机应答/ReplaceBodyText 与底层旧 MockResponse，容易让用户误以为 Mock 不属于规则。Mock 本来就是规则效果；特殊的是旧兼容分支的运行落点。此前“不重复下游 Rules”的措辞也不能用来证明上下游两组不同阶段的规则本来会重复。用户未要求改变既有规则执行次数，本任务设计不能借接入 Finalize 擅自改变它。
- 设计影响：正常远端响应、本机应答加正文规则、仍受支持的旧 MockResponse 结果都必须覆盖规则后 Finalize。实现时处理旧分支旁路，不能把这个工程落点再次当成产品是否启用的选择。
- 候选验收追加：分别以 LocalHttpServer + ReplaceBodyText 与旧 MockResponse 验证最终业务正文进入 Finalize 一次；不访问真实上游的事实、原请求 token 关联、最终签名和规则执行次数分别记录。Finalize 不得对 Mock 生成前的请求正文误签名，也不得重复执行已处理的规则。
- 仍待设计：旧 Mock 在 upstream 规则提交后才物化响应；如何与候选“当前方向写准备失败回滚”合同衔接需细化，不能把已提交 upstream 生命周期默认为可由 downstream 失败回滚。ABI、接口输入和错误/无正文等剩余议题继续待讨论。
- 本轮仅确认与解释设计；无产品源码或测试改动。任务保持待确认，未进入实现。


### D3–D6：方向、完整上下文、旧包兼容与原样返回

- 确认时间：2026-09-07 14:27:41 +08:00
- 用户逐项决定：
  1. “upstream也要有”。
  2. “将能提供的信息都提供给 Finalize 避免以后还要在修改”。
  3. “Finalize是可选的 能兼容以前的”。
  4. “不考虑这些 没有就跳过不处理 返回原来的信息”。
- D3：HTTP upstream、downstream 都提供 Finalize；每个方向的方法存在时每次都调用，不依赖 Rules 命中、Document 变化或 Encode 执行。D2 的 Mock 覆盖继续有效。
- D4：上下文不再限制为 method/path；应提供本次交换在当前阶段能获得的完整消息、请求响应关联、连接及处理信息。未知或尚未发生的事实不能虚构；原始与处理后信息必须明确区分。
- D5：Finalize 是可选扩展；旧包不需要修改或重打包即可保持旧行为。上/下方向应能独立判断是否实现，不能把新增方法变成旧包加载的必需条件。
- D6：CPS 无所需信息或不适用时跳过处理并返回进入 Finalize 前的原信息。此前列出的未知接口、缺 token、非法 JSON、无正文、缺必要签名模型数据等，不再作为必须逐项访谈的产品边界；按用户选择原样返回。这里的“原来”是已经经过规则和 Encode 的输入，不是撤销规则回到网络最初报文。
- 范围辨别：D6 针对 Finalize 对业务输入的适用性；没有授权把 Wasm trap、取消、失效 runtime、错误函数签名等执行故障吞掉。对这些故障沿用既有错误传播。Finalize 之前的 Decode/Rules 合同也不会由 Host 暗中放宽；CPS 包自身设计必须避免把约定可跳过输入提前变成不必要的处理失败。
- 原候选变化：v0.1 的“只做 downstream”“只给 method/path”“整体升级旧包”“缺数据一律失败”不再作为当前方案；保留原文用于解释演变，以 D1–D6 为当前产品决定。
- 本轮业务边界已明确；下一步讨论具体接口及接入设计，不再次询问以上四项。用户仍只授权设计，未进入实现。

## 设计讨论 v0.2（依据 D1–D6，接口结构草案）

本节汇总已确认行为，并提出可审阅的技术形状；字段名、序列化布局和内部装配不等同于已经批准或实现的公共 ABI。前文 v0.1 作为历史草案保留。

### 1. 两个独立的可选方法

```text
upstream-finalize(body, context)   -> Result<body, PackageError>
downstream-finalize(body, context) -> Result<body, PackageError>
```

- 同一 HTTP 包可以两个都实现、仅实现一个或两个都没有。
- 方法存在：每次该方向写准备都调用一次；没有方法：保持该方向原有行为。
- 信息不足/业务不适用：包返回调用前的原正文。Host 保留现有 Message 原字节，不做多余的 JSON 格式化、trim 或重新编码。
- 上下文只读；当前草案仍让方法返回本方向正文。提供全部上下文不意味着自动开放修改另一方向请求、连接或运行配置。
- 返回新正文后才按现有 codec/framing 合同更新消息；包不接管 Content-Length/Transfer-Encoding 的物理组帧。

### 2. 统一阶段位置

```text
App 请求
  → upstream Decode / Display
  → upstream Rules / 条件性 Encode / 完成正文动作
  → 可选 upstream Finalize
  → 上游写准备完成与当前方向规则提交
  → RemoteServer 或 LocalServer，或规则生成 Mock 响应
  → downstream Decode / Display
  → downstream 已约定的 Rules / 条件性 Encode / 完成正文动作
  → 可选 downstream Finalize
  → 下游写准备完成与当前方向规则提交
  → 故障注入 / 实际发送
```

这是一张语义流程图。旧 MockResponse 不会为了新增 Finalize 而擅自增加此前跳过的下游 Rules/Encode，但其生成的最终业务正文仍需经过同一个 downstream Finalize 能力。并非复制第二套 CPS 算法。故障注入保持其目的，不因 Finalize 默认修复故意产生的非法报文。

对正常联合规则路径，Finalize 应在当前方向规则增量提交前完成；旧 Mock 的 upstream 规则已经提交时，downstream 失败不回滚已经提交的 upstream。最终具体的 HTTP prepare/actor 端口拆分仍需在设计审查中验证这个边界。

### 3. 完整上下文建议

建议使用统一、带版本的上下文对象；WIT 边界采用 `context-json: string`，Host 内部仍用具名强类型结构构建。这样新增可选字段不改变函数参数列表；包只读取需要的字段。字段语义若发生破坏性变化，仍需要版本管理，不能承诺以后任何新需求都无需变化。

| 信息组 | 建议提供内容 | 当前可获得性与接入要求 |
| --- | --- | --- |
| 调用身份 | contextVersion、direction、精确 package id/version、workspace/listener、runtime epoch、connection id、当前 transaction 关联信息 | package/factory/connection 已有事实；transaction 当前隐式一问一答，结构化关联标识的具体形式待内部设计 |
| 原始请求 | 请求行、method、URI/path/query、headers、原始 Body、已成功解码的文本与 Document | 请求进入时有 Message/URI，需在修改及 writer take 之前保留不可变快照 |
| 处理后请求 | 当前阶段的请求行/headers/body、规则后 Document、upstream Finalize 后的准备结果 | upstream 正在处理；downstream 应可读取已保留的 upstream 准备快照；准备完成不能标成实际发送成功 |
| 原始响应 | status/响应行、headers、Body、已解码文本、Document，已有 informational heads | upstream 调用时尚未产生，必须显式不可用；downstream 读取时已有，应保留规则前快照 |
| 处理后响应 | 当前规则与 Encode 后的 status/headers/body/Document，Finalize 前输入 | downstream 可获得；本次 Finalize 自身输出在调用前不存在，不能提前放入上下文 |
| 消息表示 | Body 原始 bytes 的无损表示、codec/charset、文本可用性、重复且有序的 headers、原始头信息、bodyModified | Message/RawHeader/HttpContext 已有；禁止把 headers 压成丢重复值的单值字典，禁止把 lossy UTF-8 当作签名原文 |
| 连接与路由 | peer、acceptedAt、channel、listener 归属、Local/Remote/Mock 来源、已解析 URI/目标、已有下游 TLS peer 事实 | ConnectionContext、factory、transaction 和 frozen endpoint 分别持有；要以当前请求路由事实组装，不查实时可变配置来冒充运行快照 |
| 上游安全事实 | 已解析连接地址、明文/TLS、已报告的 TLS version/cipher、对端身份与客户端身份提交事实 | 现有 UpstreamSecurityEvidence 在建连后回调报告，但 transaction 未保留；可按同笔交换保存已发生信息，首次 upstream Finalize 早于建连时不可用 |
| 处理事实 | 原始/working Document、规则命中与动作结果、Encode 是否执行、待执行的故障动作、Mock/close 标记 | Envelope、joint evaluation、actor 与 transaction 分散持有；需在各自拥有阶段提供快照，不能拿 connection 全部历史作为本次规则结果 |

“全部可提供信息”按本次 HTTP 交换及其运行上下文展开；不建立全局可变变量包，也不把完整数据库/其他连接数据混入当前调用。上游还没有响应、Mock 没有真实上游连接等情况以真实不可用状态表达；上下文 schema 将区分未产生与空 Body，不能用虚构空成功响应填充。

原始字节建议在 JSON 边界使用项目已有的无损二进制表示；文本仅在现有 codec 能准确解码时提供。原始/当前消息采用不可变共享快照，尽量复用 Bytes/Arc；只存活于当前 transaction，在完成、失败或取消时释放。完整上下文会增加跨 Wasm 边界的序列化量，后续测试需要覆盖真实大小而非仅空上下文。

当前源码依据：`proxy/src/http/exchange_runtime.rs:67`（transaction 字段）、`endpoints.rs:285`（request.take）、`proxy/src/message.rs:108`（Message/RawHeader）、`proxy/src/transport/contracts.rs:17`（连接）及 `:41`（上游 TLS 事实）、`exchange/src/envelope.rs`（读时事实）、`infrastructure/.../pipeline/rule_runtime/actor.rs:29`（actor 输入）、`http/capabilities.rs:24`（连接身份及 factory 元数据）。

### 4. 旧包兼容接入建议

- 保持旧 HTTP 必需 Decode/Encode/Display 合同及现有严格 Manifest；不要求旧包补 Finalize 声明。
- 新包通过额外可选导出提供对应方向能力；加载时分别检查 export 存在性与正确签名，再固定本实例可调用能力。
- 不存在与存在但无效必须区分；不得把 typed lookup 的所有错误统一当成“旧包无 Finalize”。
- 不通过“先按新包加载失败，再尝试旧包”的异常回退实现兼容，避免掩盖真正无效的组件；具体 Instance/bindgen 接入正在依据固定 Wasmtime 48.0.1 官方源码核验。
- 运行时内同一精确包版本仍由现有 Store/代次管理器串行调用，Finalize 不引入第二个实例、独立缓存 token 或独立生命周期。
- 本轮对象为当前 HTTP Wasm 包。远端 HTTP RPC 的当前支持状态与扩展合同需按现有适配能力核对，不能顺带宣称远端协议也已支持新增方法。

### 5. CPS 处理与原样返回

包处理逻辑候选：从只读原始请求识别可处理数据及 token；检查当前方向正文能否按已有 CPS 模型处理；能处理则计算目标字段并返回新正文；没有适用模型、没有 token/必要字段、正文为空或不能解析时，直接返回输入正文。

upstream 方法同样遵循该合同：接口提供能力不意味着对所有请求都强行加签；没有可处理信息就返回原文。downstream 成功处理时依据原请求 token 更新 `data.sign`。不向 Proxy 通用核心加入 CPS 字段或 MD5 业务逻辑。

兼容与跳过分三类验收：旧包无方法时旧行为不变；方法存在但业务输入不适用时被调用且返回原文；方法存在但执行故障时保留真实失败。用户第 4 项不是 catch-all 成功兜底。

### 6. 验证与当前停点

候选测试集相应调整：两个方向均覆盖无规则/未命中/结果不变/正文变化；Mock/LocalServer；无hook/up-only/down-only/both；未重打包旧组件；hook同名但错误类型；完整上下文原始与当前快照、重复headers和非UTF-8 bytes；upstream无响应与Mock无上游事实；连续请求与并发连接隔离；不适用原文逐字节保持；有效CPS向量与Java一致；错误/取消/Store生命周期；规则增量与最终发送证据。

本轮已落实用户产品决定，不再就 D1–D6 重复提问。v0.2 是具体接口结构及接入位置的讨论稿，未写产品源码、测试、配置，未构建、安装、提交、推送或触发 CI；任务保持待确认，等待设计讨论完成，不代表进入实现。


### 7. v0.2 本轮核验结论

- 核验时间：2026-09-07 14:31:51 +08:00
- 固定依赖为 Wasmtime 48.0.1；本机 Cargo registry 中同版官方源码已确认 `Instance::get_export` 返回导出种类和索引，generated binding 提供 `new(store, &Instance)`。这支持“保留同一个 Instance、旧绑定与可选导出分别检查”的设计方向；不需要把 Finalize 加成旧 world 的必需导出。
- 官方源码定位：`wasmtime-48.0.1/src/runtime/component/instance.rs:289`、`wasmtime-internal-wit-bindgen-48.0.1/src/lib.rs:1088`。导出查找必须先区分完全缺失、同名非函数、函数签名错误三种情况；仅 get_func 返回 None 不足以证明整个导出不存在。
- 本轮 bounded best-practice-research 子任务用于收集官方 API 证据；版本网页证据未形成完整结论，未宣称外部文档核验全部完成。主 Agent 以实际读取的本机同版官方源码确认上述有限结论；后续 typed call/资源清理及真实旧组件兼容仍须实施时验证。
- 设计验证：当前 feature 分支、D1–D6 与 v0.2 章节、任务待确认状态、唯一待办入口、已有源码链接、空白及锁释放检查通过；无产品源码、运行配置和测试实现改动。
- 停点：本轮设计更新完成，技术原型、兼容测试、CPS 算法向量、构建及运行验证均未执行；不把设计可行性当成旧包兼容验收 PASS。


### D7：context 使用 JSON 字符串

- 确认时间：2026-09-07 14:35:34 +08:00
- 用户要求：“直接context参数用json描述吧 其他的方法不是这样的吗”。
- 已确认参数表示：Finalize 的 context 使用 JSON 描述，在 Wasm/WIT 边界以 `context-json: string` 传入；包解析该 JSON 并按需读取。沿用现有 JSON 文本参数方式，不为上下文另建必需的 WIT record 字段列表。
- 本次源码核对：`protocol-package.wit` 中 HTTP/Socket Encode 均接收 `document-json: string`，Display 接收同一 JSON 字符串；HTTP Decode 的 input 是原始 Body 文本，Socket Frame/Decode 的输入为 bytes，因此不能泛称所有方法的所有参数都是 JSON。远端 RPC 的 params 则是 JSON 对象，EncodeParams 中 document 为自然 JSON Document，不是整个系统都使用双重 JSON 字符串。
- 更新后的方法形状（独立可选扩展签名，尚未实现）：

```wit
upstream-finalize: func(body: string, context-json: string) -> result<string, package-error>;
downstream-finalize: func(body: string, context-json: string) -> result<string, package-error>;
```

- body 继续表示本方向规则/Encode 后的正文文本，不要求它本身是 JSON；返回值同样是正文。context-json 是一个上下文 JSON 对象的序列化字符串。参数不把正文与上下文重复包进新的 RPC 信封。
- 上下文内容沿用 v0.2 完整字段组。可扩展 JSON 不等于没有字段语义合同；版本、原始/处理后消息、可用性、字节表示仍在上下文结构中明确，以便包作者稳定使用。
- 本轮只更新设计与必要的术语说明；D1–D6 不变，不改既有 Encode/Display/Decode 签名，不进行产品实现或 CI。


### D8：context 仅包含 HTTP 上下文

- 确认时间：2026-09-07 14:40:56 +08:00
- 用户新要求：“只返回http相关的上下文信息 proxy内部的就不需要返回了”。
- 原需求及变化：D4 曾要求提供当前阶段能获得的全部信息；本轮将其明确收敛为 HTTP 请求/响应信息。v0.2 以及上一轮口头 JSON 示例中的 Proxy 运行内部信息不再属于 Finalize 的公开 context。
- 保留范围：原始与当前请求/响应、HTTP 版本、请求 method/URI/request-target、响应 status/响应行、完整有序 headers、原始 HTTP head、正文原始 bytes 与可准确解码的正文文本。
- 移除范围：exchange/transaction/runtime/workspace/listener/channel 标识、package 元数据、连接建立时间和网络/TLS运行诊断、规则命中与动作信息、Document/workingDocument、Encode 执行状态、内部故障计划、Mock/Local/Remote 来源标记。方向由 upstream/downstream 方法本身表达；本轮结构不增加 Proxy 内部元数据封装。
- 完整 HTTP 信息仍采用不可变快照，原始/当前不能混淆；upstream 调用时尚无响应，response 为 null。Mock 生成的响应使用相同 HTTP 消息结构，不暴露内部 mocked 标志。
- 结构草案如下，字符串占位符仅用于说明类型，不是正式输入；具体字段名仍属于设计稿。

```json
{
  "request": {
    "original": "<HttpRequest>",
    "current": "<HttpRequest>"
  },
  "response": {
    "original": "<HttpResponse>",
    "current": "<HttpResponse>"
  }
}
```

- HttpRequest 候选内容：startLine、httpVersion、method、uri、requestTarget、headers、rawHeadBase64、body.text、body.bytesBase64；HttpResponse 使用 startLine/httpVersion/statusCode/headers/rawHeadBase64/body，不塞入无关的请求字段。
- headers 保持数组形式以保留重复字段和顺序；原始字节保持无损，不能依赖会丢失字节的文本转换。当前 Message 表示 Finalize 调用前的业务正文快照；请求侧完成后保留给下游读取，不能标成已经实际发送成功。
- 失效验收项：v0.2 中要求公开内部身份、规则执行状态、TLS/连接事实的 context 验收取消；原始/当前请求关联、HTTP Header/Body 完整性与无正文场景仍保留。新验收应证明 context 只含明确 HTTP 字段，没有泄漏上述内部字段。
- 实现影响：无需为 Finalize 收集规则执行列表、运行时诊断或 TLS 回调；仍需保留本次 transaction 内的 HTTP 请求/响应快照。内部配对和生命周期继续由 Proxy 自身负责，不作为包输入输出合同。
- D1/D2/D3/D5/D6/D7 保持：双方向独立可选、每次调用、Mock 覆盖、旧包兼容、不适用原样返回、context JSON 字符串。正文返回值合同不变。
- 本轮仅更新设计，任务仍待确认，未进入源码实现。


### D9：按方向直接描述消息，移除外层 request/response

- 讨论时间：2026-09-07 14:47:42 +08:00
- 用户再次强调 upstream 给 request、downstream 给 response，随后指出：“都区分方向了 为啥还要有request这些 这一层不能去掉吗”。
- 结构调整：context 的主体直接对应当前方法方向；upstream 的 original/current 是请求，downstream 的 original/current 是响应。移除 D8 草案中同时套 request/response 的外壳，不再把整笔 HTTP exchange 当成两个方向相同的公开输入结构。
- 保持方向合同：upstream 只返回处理后的请求正文；downstream 只返回处理后的响应正文。读取原请求关联信息不表示 downstream 有权修改请求。
- 仍必须解决的已有目标：CPS 响应签名需要 App 原请求 token，响应正文自身没有这个 token。去掉外层结构不应悄悄把签名必要输入删掉，也不应把 token 存进跨请求全局变量。
- 提供给用户的候选整理：downstream 以 original/current 描述响应，在顶层保留 `requestMethod`、`requestTarget` 等原请求只读关联字段；CPS 包自行提取 token，Proxy 不增加 CPS 专用 token 字段。字段集合与是否继续提供其他原请求 HTTP 信息仍是设计候选，本轮不把“去掉一层”推导为用户已批准删去所有其他请求信息。
- 本轮示意：

```json
{
  "original": "<原始响应>",
  "current": "<下游规则处理后的响应>",
  "requestMethod": "<原请求 method>",
  "requestTarget": "<原请求 path + query，保留转义>"
}
```

- 上述 requestMethod/requestTarget 始终取 App 原请求事实；不拿 upstream 改写后的值替代 CPS 验签所依赖的原值。
- 前文 D8 外层结构及完整交换 JSON 示意由本节方向化草案替代；HTTP-only、JSON参数、可选旧包兼容、每次调用及不适用原样返回合同不变。
- 设计记录更新通过当前分支、任务状态/入口、空白与锁所有权检查；未修改产品代码、测试或运行配置，未进入实现。


### D10：删除 original 消息快照

- 确认时间：2026-09-07 14:48:54 +08:00
- 用户要求：“original也不需要”。
- 当前合同：Finalize context 不再提供 original 原始消息快照；主体仅提供当前方向规则/Encode 处理后的 HTTP 消息。D8/D9 及 v0.2 的 original/current 双快照公开结构由此收敛。
- downstream 候选结构：`{ current: <当前响应>, requestMethod: <请求method>, requestTarget: <请求path+query> }`；不再包含 original 或完整嵌套 request/response。请求关联字段具体集合仍沿用 D9 的候选状态，本轮没有重新扩大为完整请求快照。
- 对称方向：upstream 主体是当前请求；downstream 主体是当前响应。输出继续只是当前方向正文，context-json 仍是只读 HTTP 上下文。
- CPS 必要关联：删除 original 字段并不把响应签名依据改成新 token。用于提取 App 请求 token 的 requestTarget 来源语义沿用 D9；只保留关联事实即可，不再因 Finalize 公开上下文而保留整份原始 HTTP 请求/响应。
- 影响：取消为 context 提供完整 original Header/Body/Document 快照的设计及相关测试；保留当前 HTTP 消息无损表示、请求关联不串单与输入不足原样返回验证。既有抓包自身的原始观察不受本次参数裁剪影响。
- 本轮只更新设计，无源码/测试/配置实现；任务状态仍为待确认。


### D11：平铺五项 HTTP 信息

- 确认时间：2026-09-07 14:52:30 +08:00
- 用户纠正：“不要这样设计 只需要 url method http code header body 这些”。
- 最新公开 context 结构：平铺 url、method、httpCode、header、body 五项。字段命名以本节草案表达，不再保留 current/original/request/response 包装、requestMethod/requestTarget 旁路字段、rawHeadBase64、body.bytesBase64、charset 或 Proxy 元数据。
- 需求演变：本节替代 D8–D10 的多层消息/关联信息组织方式。先前“尽量提供全部信息”已由用户明确收敛为这五类 HTTP 信息，不能再以可扩展或兼容未来为由恢复被移除的字段。
- downstream：url/method 描述对应请求，httpCode/header/body 描述当前规则和 Encode 处理后的响应。响应自身没有独立请求方法，不能把 method 解释为响应方法。CPS 从 url 提取本次请求 token，Proxy 不单独增加 token 字段。
- upstream：url/method/header/body 描述当前请求；此时尚无响应状态码，httpCode 为 null，不伪造 200。
- header 建议保持有序 name/value 数组，以保留同名重复 HTTP 头；body 为当前方向正文字符串。除了这五个顶层字段，不再加入运行信息或额外快照。

```json
{
  "url": "https://example.com/pay/TOKEN",
  "method": "POST",
  "httpCode": 200,
  "header": [
    {
      "name": "Content-Type",
      "value": "application/json; charset=utf-8"
    }
  ],
  "body": "{\"data\":{\"amount\":200,\"sign\":\"OLD_SIGN\"}}"
}
```

- 上述示例仅表示字段结构，不是真实 CPS 测试向量。downstream 的 url 应保留 App 本次请求所用 token 的来源语义，不从响应金额生成新 token；具体 URI 构造与已有请求捕获的接线在技术设计中核验，不增加另一个公开 URL 字段。
- 本轮仅裁剪并明确 context JSON 的内容，不据此擅自改变已讨论的函数参数个数或返回值。上下行 Finalize 可选、旧包兼容、存在则每次调用、Mock 覆盖、不适用返回进入 Finalize 前原正文仍有效。
- 失效验收：完整快照及无损 Base64 字段的公开参数测试取消；改为准确的 url/method/status/header/body 映射、header 重复值、上下行状态码语义以及不包含额外内部字段。Host 自身如何保留原始字节仍是内部发送正确性要求，不作为新 context 字段。
- 当前仅更新设计，无产品代码、测试或配置变更，未进入实现。


### D12：header 使用 HTTP 头名到值的 JSON 对象

- 确认时间：2026-09-07 14:54:35 +08:00
- 用户纠正：“header不要自己设计”，并提供浏览器网络面板截图，要求采用截图所示头名对应值的直接表达。
- 当前结构：header 使用普通 JSON 对象，key 为实际 HTTP 头名，value 为对应头值；不使用助手此前提出的 `[{name, value}]` 条目数组，不额外包一层自定义 Header 类型。
- 原设计变化：D11 中 header 数组建议撤回；现有原始 HTTP 模型可以继续内部保留原始头，但其内部表示不直接决定面向包的 context JSON 形状。
- 截图可直接证明的例子为 Content-Type=image/svg+xml、Content-Length=589、Cache-Control=max-age=2592000、Server=TLB。截图不包含 Body，下面正文是占位说明，不作为实际抓包或测试证据。

```json
{
  "url": "https://example.com/resource.svg",
  "method": "GET",
  "httpCode": 200,
  "header": {
    "Content-Type": "image/svg+xml",
    "Content-Length": "589",
    "Cache-Control": "max-age=2592000",
    "Server": "TLB"
  },
  "body": "<响应正文>"
}
```

- HTTP 头值在示例中按字符串表达，包括 Content-Length。五个顶层字段及方向语义不变：downstream 的 URL/method 关联本次请求，code/header/body 描述当前响应。
- 截图只覆盖单个头名对应单个值；同名重复头的无损表达未由这张截图直接确定。后续技术设计应核对已有 Header 投影合同，不默默覆盖或自创不适用的拼接规则；不因此恢复用户已经否定的 name/value 条目数组。本轮不增加额外问题或扩大实现范围。
- 设计文档更新、五字段 JSON/header-object 解析和任务入口检查通过；无产品实现或 CI。


### D13：Finalize 返回修改后的 HTTP 信息 JSON

- 确认时间：2026-09-07 15:01:16 +08:00
- 用户确认流程：“当进行encode后 生成的header body 调用finalize 返回修改后的json 再然后返回给app对吧”。
- 已确认的 downstream 流程：规则和 Encode 完成当前响应准备 → 将 url/method/httpCode/header/body 交给 downstream-finalize → 返回修改后的同结构 JSON → Host 按返回的 httpCode/header/body 构造 HTTP 响应 → 写出给 App。此处 url/method 是对应请求的关联信息。
- Encode 的现有职责仍为正文编码；这里的 Header 与其他 HTTP 信息由 Host 从当前消息组装，不能误述为现有包 Encode 已经返回完整 HTTP JSON。规则动作修改正文时，Finalize 必须看到规则与 Encode 均完成后的实际正文。
- 输出合同变化：替代 D10/D11 及此前草案中“只返回 Body”“context 只读”的限制；Header、正文等 HTTP 信息通过返回 JSON 交回 Host。App 收到正常 HTTP 状态行、Header 和 Body，不会把这五字段包装 JSON 当作业务正文接收。
- JSON 已经包含 body，单个 JSON 输入并返回同结构 JSON 是接口收敛候选；本轮确认的是数据流和返回语义，不据此宣称具体 WIT 函数签名、错误类型或全部字段可写范围已经冻结。upstream 对称处理请求，不能把 downstream 响应的状态码写出语义套到请求。
- 既有调用合同保持：方法存在则每次调用，即使 Document 未改或 Encode 被跳过；Mock 在规则确定响应后也经过 Finalize。方法缺失保持旧包行为；业务信息不适用时返回进入 Finalize 前的完整输入 JSON，保留先前规则改动，不回退到规则前消息。
- 验收调整：增加修改后 JSON 到实际 HTTP 响应的状态码、Header、Body 映射验证，证明 App 不收到包装层。实际写出仍需遵守现有 HTTP framing 与错误传播合同；具体字段验证、重复头投影和 ABI 兼容测试留待技术设计，不自行增加静默修复、默认值或错误兜底。
- 本次只更新需求与设计记录，任务仍待确认；未修改产品代码，未执行产品测试、提交或 CI。


### D14：状态码字段简化为 code

- 确认时间：2026-09-07 15:05:26 +08:00
- 用户要求：“httpCode能简化为code吗”。
- 当前公开 JSON 字段统一为 url、method、code、header、body；Finalize 输入与返回 JSON 均使用 code。此项替代此前草案中的 httpCode 命名，不增加别名或双字段兼容路径。
- 仅调整名称，语义不变：downstream 的 code 为当前响应 HTTP 状态码，upstream 的 code 为 null。其余字段结构、调用阶段、返回语义及可选方法合同不变。

```json
{
  "url": "https://example.com/pay/TOKEN",
  "method": "POST",
  "code": 200,
  "header": {
    "Content-Type": "application/json"
  },
  "body": "{\"data\":{\"sign\":\"UPDATED_SIGN\"}}"
}
```

- 示例仅说明当前字段结构，不是真实 CPS 验收向量。此前记录保留旧名用于追溯，当前设计以本节及文首摘要为准。
- 已检查五字段 JSON、Header 对象、任务状态与索引对应；本次仅修改设计文档，未开始实现或运行产品测试。


### D16：URL 修改限制与本地 Wasm 范围

- 确认时间：2026-09-07 15:13:19 +08:00
- 用户回答：1. “url不能变 只能修改path?query这后面的数据”；2. “先支持本地wasm吧”。
- URL 边界按本轮解释记录：upstream Finalize 可以修改同一 URL 的 path/query，scheme、host、port 保持不变，不通过此能力更换目标服务。公开 JSON 仍使用一个 url 字段，不增加 path/query 旁路字段。
- 请求目标写出需要反映获准的 path/query 修改；若包返回越界的 scheme/host/port 改动，必须可观测地报告违反合同，不静默忽略或改为另一个目标；具体错误类型沿用项目强类型错误模型，在技术设计中落定。
- CPS 的 downstream url 仍须关联 App 原请求所用 token；upstream path/query 获准变化不代表可拿改写后的 token 替换 App 保存的原 token，也不允许 downstream 反向改写已发送请求。
- 运行范围确定为本地 Wasm HTTP 协议包；远端 HTTP RPC 的 Finalize 扩展不在本任务实现及验收范围。D15 的远端范围待定项由本节关闭。
- 用户本轮只回答 URL 修改范围，没有明确 method 是否可修改。保留这一字段权限为唯一待询问的产品事项，不默认扩大请求方法改写能力。单 JSON 输入输出延续当前方案；重复 Header 先核对既有实现，不机械增加产品访谈。
- 验收追加：path/query 修改实际写出、scheme/host/port 不可变、越界返回的错误路径、原 App token 关联不受 upstream 修改污染；本地 Wasm 新旧包与方向独立兼容。远端 RPC Finalize 验收为 N/A（不在范围）。
- 本轮仅同步设计决定；用户要求等待完整任务确认仍有效，状态保持待确认，不开始源码或测试实现。


### D17：method 保持不变

- 确认时间：2026-09-07 15:16:45 +08:00
- 针对“method 也保持不变，对吗？”，用户回答：“不变”。
- 当前合同：Finalize 不改变 HTTP 请求方法，输入与返回 JSON 的 method 保持一致。upstream 可变的 URL 部分仍仅为 path/query；scheme/host/port 不变。downstream 的 method 仍为对应请求的关联信息，不用于改变已发送请求。
- 本项关闭 D16 中的 method 权限待定事项；前文“待确认”表及候选设计为历史讨论，以 D1–D17 的最新决定和文首摘要为准。
- 验收追加：合法返回保持 method；返回不同 method 时按违反不可变字段合同报告错误，不静默改写或忽略。具体错误类型在实现设计中复用现有强类型错误模型。
- 已询问的产品范围均有答复；重复 Header 投影、可选 Wasm ABI 等仍需技术核验，不能把未经验证的技术方案描述为已经完成。用户之前要求“列出完整任务等我确认”，本轮仅确认字段边界，不视为完整任务开工授权。
- 任务保持待确认，进入实现时间仍为无。本轮只更新任务设计，未修改产品源码或测试，未执行提交、CI 或部署。
