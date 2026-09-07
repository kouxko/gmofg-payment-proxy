# TASK-20260905-004：统一 HTTP/Socket 运行合同并消除重复逻辑

- 任务 ID：`TASK-20260905-004`
- 状态：已完成
- 任务日期：2026-09-05
- 创建时间：2026-09-05 23:39:26 +08:00
- 开始时间：2026-09-05 23:39:26 +08:00
- 最后更新时间：2026-09-06 10:03:54 +08:00
- 完成时间：2026-09-06 09:59:31 +08:00
- 创建路径：`docs/tasks/pending/2026-09-05/unify-http-socket-runtime-contracts.md`
- 归档路径：`docs/tasks/completed/2026-09-06/unify-http-socket-runtime-contracts.md`
- 关键词：HTTP、Socket、Exchange、LocalServer、Rules、Upstream、Downstream、DRY、typed error、hot replacement
- 任务优先级：高；涉及协议、公共内部端口、异步所有权、持久规则校验和跨层运行合同。

## 背景、目标、范围

用户先要求只读审查 HTTP/Socket 整条流程及重复，随后明确要求“处理一下 不要有重复的代码逻辑 符合软件设计准则”。目标是完成审查内的正确性修复，消除同一职责的生产重复和无实际阶段的占位转交，让网络、规则、编码和观测拥有明确边界。
范围：三个P1（Local HTTP合法响应、Plain keep-alive规则热更新、Document condition搭配HTTP动作校验）；真实Rules/Encode事务；写入准备与传输提交事实；typed错误；HTTP公共传输、Socket端点装配/中立I/O、方向规则编译去重；相应自动化测试、架构门禁和当前文档。
不在范围：新协议能力、跨origin keep-alive支持、HTTP CONNECT/Upgrade/MITM、Socket pipelining/server push、数据库Schema迁移、Android业务、远程服务/真实交易、UI产品扩展、依赖升级、提交/push/CI/发布。保留工作区现存文档和其他任务档案。

## 初次调查快照（历史，已由文末确认与验收更新）

历史已验证：TASK-20260903-002确认LocalServer原样回环，使用当时公开MockResponse生成合法响应；原用例HTTP-LOCAL-MOCK-001位于docs/testing/evidence/2026-09-03/TASK-20260903-002/HTTP-LOCAL-MOCK-001/。本次复用零上游、两方向规则和BodyCodec步骤，但新增当前公开ReplaceBodyText路径。
历史已验证：TASK-20260904-005的HTTP-CHUNKED-RESPONSE-001保护已解码响应去Transfer-Encoding后重算Content-Length；新用例引用其输入快照，不改历史资源。
当前已验证：共用Exchange主循环和Server端口；HTTP Rules stage/take与no-op Encode；Socket Rules内实际Encode；HTTP Writer完成表示内存交接；Domain validate_http_runtime_content因Document条件提前返回；Plain能力按连接创建时规则固定。上一轮三个架构静态门禁通过，源码未脏。
推断：公开Local Body Mock在发送时解析请求路径为状态码失败；Plain既有连接添加Document规则缺联合上下文而失败；需新增回归取得RED确认。
未知：Local HTTP默认status/Header产品合同；已提出异步问题，回答前不实现该分片。

## 需求确认记录与就绪门禁

- 用户授权范围：落实上一轮审查并去重，完整本地edit-test-review；未授权提交/外部发送。
- 保留：两个写出阶段；按(priority,rule_id)顺序读取working Document；Encode失败回滚且不提交hit；Encode成功后提交lifecycle，网络失败不声称送达；非UTF-8 wire bytes、BodyCodec、HTTP原始头/1xx/fault、固定endpoint、Socket raw half-close与协议严格配对、取消/关闭所有权、精确包版本均保持。
- 待确认Q1：Local HTTP是否采用200、回环effective Body、保留Content-Type/Content-Encoding、重算Content-Length、其余请求Header不带回且下行规则可改响应；或必须规则明确响应。未回答前本分片保持待确认。
- 独立就绪分片：Domain动作校验；Socket端点/I/O去重；共同编译步骤；事务/错误设计调查。其输入输出与已有合同明确，可先回归和实施。
- 示例：Document条件+Delay(0)保存拒绝；同一Plain keep-alive连接先无Document规则，新增后后续请求字段修改生效，再删除后非JSON恢复Plain行为；Encode故障精确标为Encode；write失败不产已提交事件；Local Mock按Q1执行。
- 进入实现时间：独立就绪分片 2026-09-05 23:39:26 +08:00；共享接口与依赖分片在独立架构审定后追加。

## 根因与修复边界

候选/已确认因果链：条件种类被用作HTTP action校验门禁；连接级静态能力与可热替换规则生命周期不一致；泛型Rules/Encode无法容纳joint actor事务而采用pending map/预制输出；缓冲HTTP协议端口和物理I/O完成语义混用；独立装配和transport实现复制同一职责。
最小复现：先在既有Domain/Infrastructure/Runtime套件中加入精确失败用例，保留RED输入与结果；发现不成立的审查项记录排除原因，不机械改造。

## 最小改动与最优设计、清理计划

方案A逐处补分支：局部小，但继续保留no-op阶段、字符串错误和重复装配；不作为整体收敛方案。
方案B复用现有Exchange/Pipeline扩展点，显式方向事务/准备输出与传输结果，条件和动作独立校验，抽取现有重复中立步骤：采用。保持协议DTO隔离，不做万能消息类，不引入依赖或双运行路径。
分片前先锁定失败/行为基线；共享接口先设计冻结；每次只去除一类重复；全批集成后独立整体对抗审查。

| 分片 | 所有权/内容 | 依赖 | 验收 |
|---|---|---|---|
| A | Domain HTTP action校验与Application回归 | 无 | Document条件非法动作RED/GREEN |
| B | Socket plan/socket、scripted及中立I/O去重 | 无，typed error后集成 | Local/Remote Direct/Protocol/TLS配置一致，取消/half-close保持 |
| C | Exchange事务/typed error及HTTP/Socket接入 | 架构设计冻结 | 无pending stage/take/no-op阶段；真实Encode与错误方向 |
| D | Plain keep-alive热更新及共同规则编译 | C | 两方向模式切换真实长连接通过 |
| E | Local HTTP合法响应与HTTP收发统一/提交观测 | Q1、C | 当前公开Body Mock、raw头/1xx/framing/fault/写失败 |
| F | 集成、当前文档、审查、归档 | A-E | 全受影响测试/静态/独立审查APPROVE |

## 测试、证据与文档计划

Cargo从根目录使用--manifest-path src-tauri/Cargo.toml；先Domain针对性，再Exchange/Runtime/Infrastructure/Application相关测试，最终fmt、Clippy、Cargo check、架构/文档/static gate。真实localhost TCP/TLS测试证明受影响HTTP/Socket输入输出；不使用真实外部服务。UI未改变则UI smoke为N/A。CI/发布/真机业务为NOT_RUN且不作为本任务目标。
证据目录：docs/testing/evidence/2026-09-05/TASK-20260905-004/，各分片README+metadata记录命令、输入、预期、实际、复测和必要资源；不制造hash证据或无内容占位目录。子Agent只写分配的用例内容，主Agent持锁更新索引和任务字段。
需检查当前文档：docs/architecture/exchange-pipeline.md、data-flow.md、rules-and-protocol-packages.md、modules.md、runtime-observability.md以及受影响需求/操作/MCP说明。历史ADR/档案不改写。
整体审查：实现批次全终态、主Agent集成验证后由独立code-reviewer/architect执行；修复相关P0/P1/P2再复验。

## 实施记录与验收

- 2026-09-05 23:39:26 +08:00：完成当前状态与相关历史复核、需求分片与清理计划，登记任务。采用TDD技能；无需额外用户许可即可推进已就绪分片。原生Agent负责有界并行工作，不依赖tmux团队运行时。
- 修改文件/附加文件/测试结果：实施后追加。
- 验收：未完成；Q1待确认。
- CI、提交、push、发布：NOT_RUN。

### 2026-09-05 23:56:41 +08:00：独立分片集成与后续就绪

A已完成：复用既有HTTP validator；Domain 102/102、Application 435/435及严格Clippy通过，RED/GREEN见[rule-validation](../../../testing/evidence/2026-09-05/TASK-20260905-004/rule-validation/README.md)。B配置已完成：共同Socket端点/TLS/连接/timeout装配，Listener 84/84通过，见[socket-plan](../../../testing/evidence/2026-09-05/TASK-20260905-004/socket-plan/README.md)。方向编译共同schema/entry构建已完成，Infrastructure 475/475及Clippy通过，见[rule-compilation](../../../testing/evidence/2026-09-05/TASK-20260905-004/rule-compilation/README.md)。

后续独立就绪：错误独立携带code/stage/direction并保留external failure，移除文本解析；Socket共用中立I/O而保留Raw half-close/Protocol strict pairing。HTTP已取得固定17/17、Forward17/17行为基线；共用connected egress流程（目标解析与TCP/TLS连接仍在各适配器），保留现有合法请求、故障、framing合同。上述独立部分进入实施，无需等待Local默认响应Q1。C/D显式事务与动态能力等待架构接口冻结。

### 2026-09-06 00:36:23 +08:00：共享接口冻结与写出证据

架构审定后进入C/D实施：Exchange增加PrepareWrite，普通Rules+Encode与joint事务均经同一Write执行prepared→encoded→transport。HTTP复用HttpExchangeState.current，明确传递Message/request metadata/joint evaluation/prepared actions，不新增全局或旁路暂存slot；删除旧stage/take与no-op Encode。Plain每笔Decode前锁定listener gate及program快照，模式与规则代次整笔一致。

Writer结果冻结为WriteOutcome::Committed/ReadReady：只有真实写完+flush产生sent；保留当前HTTP允许提前最终响应的合同，ReadReady继续读取/转发响应，绝不为了flush receipt新增fail-close。Socket实际write/flush仅返回Committed。HTTP上游用connection-owned task及一次性write receipt/response接收器分离写与读；下游output携带write_completed Sender，Writer等待Receiver，ResponseBody消费完成并且ResponseIo实际flush成功后ack。原生架构Agent独立复核early-response边界，确认该enum比隐藏flag或伪造成功更小且正确。

Socket typed IO分片61/61及shared error回归通过，证据[typed-socket-io](../../../testing/evidence/2026-09-06/TASK-20260905-004/typed-socket-io/README.md)。Plain新增确定RED：同一真实Plain capability添加Document规则后CONFIG_INVALID，证据[plain-hot-replacement](../../../testing/evidence/2026-09-05/TASK-20260905-004/plain-hot-replacement/README.md)。下游新增RED证明Body EOF被错误当成transport完成，实际输出保存在http-transport/outputs/red-response-commit.log；现已接入共用tracker，最终GREEN待集成测试完成。

HTTP/Forward共用egress、bounded body collection和cancel/timeout函数；删除Forward重复DropResponse模块和仅Body-consumed的CompletionBody。保留Forward无pipeline的已有streaming public合同，与buffered protocol模式的差异不擅自改为全量缓存；公共传输阶段继续复用。当前只有Local响应Q1仍待用户选择，未新增默认。

### 集成前源码冻结与文档同步

全部已就绪实现分片已收集，所有实现 Agent 已结束；Local Q1 仍未收到用户答复。主 Agent 停止源码修改后执行全 workspace/all-targets/all-features cargo check，PASS；write_commit 7/7 PASS。完整后端测试、strict Clippy、fmt、架构/规则守卫与脚本 lint 正在执行，独立 code-reviewer 同时只读审查集成源码。

- C：PrepareWrite/HttpMessagePrepare显式事务；删除全局JointHttpRuleRuntime与no-op Encode。Joint失败保留原Message，Encode失败回滚lifecycle，错误保留code/stage/direction及外部包详情。
- D：Plain每笔Decode取得Listener gate和program快照；同一capability与真实TCP keep-alive添加/删除Document规则均GREEN。TCP使用真实Listener/Fixed Server与聚焦LiveHotPolicy，生产actor事务由独立production_shape回归证明，两个验证层级不混称。
- E已就绪部分：共用HTTP egress与下游write tracker；真实write+flush才提交；early final response通过ReadReady继续读。Forward无Pipeline流式入口保留。Forward关闭判断改用HTTP_ENDPOINT_CHANGED/HTTP_EXCHANGE_CLOSED，不再解析错误文本；误导文本用例取得RED/GREEN。
- 错误转换收敛到ProxyError::into_exchange_error，两个HTTP调用点复用；已有外层错误码保持，exchange_code用于保留精确内部分类。
- 源码超长模块按职责拆分：Socket observer归socket_config；HTTP App endpoint归endpoints/app；Plain JSON与live TCP测试独立文件。本次触及文件无新增500行超限。

#### 验证档案

- [HTTP公共传输与下游提交](../../../testing/evidence/2026-09-05/TASK-20260905-004/http-transport/README.md)
- [联合写准备与Encode回滚](../../../testing/evidence/2026-09-05/TASK-20260905-004/joint-write/README.md)
- [Plain长连接热更新](../../../testing/evidence/2026-09-05/TASK-20260905-004/plain-hot-replacement/README.md)
- [HTTP上游真实提交](../../../testing/evidence/2026-09-06/TASK-20260905-004/http-upstream-commit/README.md)
- [稳定集成验证与资源索引](../../../testing/evidence/2026-09-06/TASK-20260905-004/integrated-verification/README.md)
- [修改文件清单](../../../testing/evidence/2026-09-06/TASK-20260905-004/integrated-verification/outputs/changed-files.txt)

#### 文档影响分析与同步

| 文档组 | 决定与结果 |
|---|---|
| exchange-pipeline/data-flow/runtime-observability | 需要更新：同步Prepare、Committed/ReadReady、真正flush与错误位置；已更新 |
| modules/rules-and-protocol-packages | 需要更新：模块职责、共同事务与条件/动作独立校验；已更新 |
| 根README/用户操作/需求/新人指南/MCP/Android/示例/内嵌帮助 | 无需更新：未改变UI字段、公共配置、包API或操作步骤；Local产品默认合同仍待确认 |
| 历史ADR与旧任务/证据 | 无需改写；保留既有决策和资源，当前任务链接旧回归 |
| 任务与测试索引 | 更新中；当前任务保留pending，不能把Q1未完成记为整体完成 |

#### 已知范围外静态失败

4个未改文件在HEAD与当前内容完全一致，原有行数为546/516/516/504；见集成证据existing-source-size.json。本次不扩大到无关文件拆分，最终报告会把该门禁与已通过检查分开。

### 最终独立范围交付与剩余需求

已完成 A/B/C/D 及 E 中不依赖 Q1 的传输收敛。F 的当前文档、测试证据和独立审查已完成；Local HTTP 响应合同仍未回答，所以不将 E/F 或整体任务记为完成，也不归档。

- 删除共同职责的重复实现：Socket plan/TLS/config 装配、Raw/Protocol 远端连接准备与 I/O、HTTP/Forward egress/Body 上限/取消超时、方向规则编译、HTTP/Joint 外部包错误转换。协议 Frame 与 HTTP 报文边界、raw half-close 和 strict pairing 各自保留。
- PrepareWrite 表达 Rules+Encode 写准备；移除全局 pending stage/take 和 no-op Encode。两方向共用调用合同，Local/Remote 继续使用 Server 端口。
- 真实 write+flush 才产生 Committed/sent；合法 HTTP 早响应使用 ReadReady。Plain 规则按消息快照更新；Document 条件不再绕过 HTTP 动作校验。
- 整体独立审查先后发现并关闭：Socket 空/超限输出检查晚于 lifecycle commit、两处生产重复、Frame Reject 丢失类型信息。最终 APPROVE（已实现独立范围），P0/P1/P2=0。
- Frame Reject 有 RED→GREEN；空/超限回归首次记录为 GREEN，没有虚构 RED。规则 Encode/输出校验失败均 commit=0/hit=0，合法输出一次提交。

#### 最终验证结果

全 workspace/all-targets/all-features cargo check PASS；最终 Infrastructure lib 479/479、Runtime lib 202/202、write_commit 7/7、Frame Reject 1/1；strict workspace Clippy、fmt、architecture、脚本 lint、diff check、60 项 HTTP/Socket 守卫均 PASS。两次正式矩阵的测试前后源码状态一致；最后五文件快照逐字节比较一致。

全量运行原始结果为1634通过/3失败：两条旧 HTTP 错误文本前缀断言已改为结构化错误码断言，最终 Infrastructure 全套通过；未修改 MCP 非 loopback HTTP 用例仍超时，单独重跑同样失败，基础 TCP 同接口可连接，根因未知。另有4个未修改源码的原有500行门禁失败。上述限制单独记录，不声称完整工作区全部通过。

最后一轮仅修改5个文件（Frame Reject产生点、对应Runtime分类测试、两条旧断言、Socket测试构造helper），只重跑受影响层级和workspace静态检查。完整资源、前后状态、真实输出、失败记录、复测命令和最终审查见[集成证据](../../../testing/evidence/2026-09-06/TASK-20260905-004/integrated-verification/README.md)及[最终独立审查](../../../testing/evidence/2026-09-06/TASK-20260905-004/integrated-verification/outputs/review-final.md)。所有当前任务用例已在上方和测试索引建立链接。

#### 待确认事项与停止点

仅 Q1 会改变下一步实现：Local HTTP 默认是否返回200、回环上行处理后的Body、只带回Content-Type/Content-Encoding并重算Content-Length，其余请求Header不带回，下行规则仍可修改响应。未确认前保留原行为，不发明默认响应。依据仓库AGENTS.md第4节需求零假设，任务状态改为待确认，完成时间保持未完成；获得答复后继续Local独立分片的回归、实现、验证和最终任务归档。

没有提交、push、PR、CI、发布或外部业务调用。工作区原有其他任务档案和修改继续保留。

### 用户确认 Q1 并扩展源码行数治理范围

用户最新明确答复：“是的 允许修改 同时要求4 个旧文件行数超限也需要修复”。Q1 已确认：Local HTTP 默认200、回环上行处理后的Body、仅保留Content-Type/Content-Encoding并重算Content-Length、其他请求头不带回，下行规则仍可改响应。原先“待确认、保持原样请求首行”的实现限制失效；通用 Exchange LocalServer 的协议中立回环保持，HTTP适配边界负责构造合法HTTP响应。

新增范围：拆分原有四个超长文件 listener_model.rs(546)、android-network-view.test.tsx(516)、listener_certificates.rs(516)、environment_configuration/listener.rs(504)。按职责提取内部实现或测试fixture/测试组，保持公共DTO、序列化、用户界面行为和测试断言不变；不压缩代码、不提高门禁、不加入忽略项。

需求就绪：输入/输出/错误/状态均有当前合同；例：POST /payment经上行Body修改后，Local返回200+该Body，下行ReplaceBodyText/SetHeader/状态动作继续生效。验收：真实TCP Local端到端默认响应、Header白名单、二进制/空Body/长度、两方向规则和keep-alive；四文件及新文件全部不超过500行，受影响Domain/Application/UI测试保持，最终全局行数门禁PASS。未确认事项清零；高优先级不变。

最小与最优：在Local HTTP适配点构建响应并复用Message现有framing方法，最小且符合职责；禁止在通用Exchange加入HTTP状态码分支。四文件按现有模块布局提取内聚职责，优先移动而非新抽象，不改公共合同。Local先新增RED再修复；纯拆分先运行既有覆盖并保存前后发现测试数。

分工：主Agent负责Local HTTP、任务索引、文档、最终集成；executor负责四文件与必需同层新文件（和主Agent无共享源码）。所有实现结束后收集并稳定验证，再独立整体对抗审查与正式归档。证据新增同任务2026-09-06/local-http-default-response、source-size-cleanup和final-verification，不改写已归档历史用例。复用TASK-20260903-002/HTTP-LOCAL-MOCK-001的Local拓扑、零真实上游、双向规则和BodyCodec步骤；本次采用当前公开ReplaceBodyText而非仅内部MockResponse完整响应替换。

原MCP非loopback超时未归因于当前变更；最后完整验证重新记录真实结果。无提交、push、PR、CI或发布授权。

- 本次确认与重新进入实现时间：2026-09-06 09:24:27 +08:00。

### 最终完成与归档

用户确认后的 Local HTTP 和四文件尺寸治理已全部实现、测试并通过独立整体审查。上方初次调查、待确认Q1和旧门禁失败均为历史记录，当前未确认事项和任务相关未关闭问题为0。

- A/B/C/D/E/F：全部完成。Local adapter 构造200响应、回环有效Body、只保留Content-Type/Content-Encoding并重算Content-Length；公共Exchange LocalServer保持协议中立；下行规则可继续修改Body/Header/状态。
- 四旧文件按职责拆分：listener_model→socket DTO；listener_certificates→运行保护测试；environment listener→证书材料投影；Android view测试→共同fixture与network-intent测试。公共DTO/序列化/界面行为不变，拆分前后原测试数31/11/212/18保持。
- 按门禁计数，四旧文件546/516/504/516→400/299/411/416；HTTP exchange_runtime测试也整体抽到子文件，父文件501→445。新增文件同样通过全仓500行门禁，无忽略、抬高阈值、代码压缩或新增依赖。
- 当前HTTP/Socket共享Exchange两方向与PrepareWrite合同；共同传输、Socket装配/I/O、编译与类型化错误各有唯一职责入口；Frame、HTTP framing、Raw half-close等协议差异保留。无全局pending stage/take或占位no-op Encode；Encode/输出校验失败不提交规则lifecycle，write+flush才产生Committed。

独立整体审查 APPROVE，最终P0/P1/P2=0。Local+四文件并行实现全部结束、收集并验证后进行该批次与任务最终审查；未在每个纯移动小项重复单独审查，原因是现有基线足够且整体审查覆盖所有移动。归档审查指出未用测试导入，删除后相关18测试、typecheck、零警告lint和diff复验通过。

最终验证：Rust workspace/all-targets/all-features 45组1640通过/0失败/0忽略；全前端72文件542通过，末次相关18通过；HTTP/Socket守卫60通过；Cargo check、严格Clippy、fmt、全仓架构、全仓500行、typecheck、eslint --max-warnings 0、diff check均PASS。原MCP非loopback用例本次通过，旧超时保留为历史，没有增加重试/回退。

最终Cargo日志使用post-fix独立目录；首轮审查误启动副本覆盖Cargo日志的事件已如实记录。测试支持文件命名、Rust缩进、证据目录类型扫描问题均在对应边界修复。最终124份文件快照逐字节一致，122源码路径在最终复验期间状态稳定；post-fix之后唯一源码变化为删除一个未用测试导入，受影响检查已复验，不机械重跑未受影响Rust。

文档影响：已同步exchange-pipeline/data-flow/modules/rules-and-protocol-packages/runtime-observability以及requirements、user-operation-guide的Local默认响应合同。历史ADR/其他任务不改写；当前任务归档与索引更新。普通README/MCP/Android/示例没有新增公共字段或操作变化，除任务入口外无需扩展。修改文件完整清单见最终证据outputs/changed-files.txt及outputs/final-resource-index.json；最后一行清理见resources/review-delta。未提交、push、PR、CI、发布或打包；真实设备/外部业务/UI截图不属于本次源码合同范围。

| 用例 | 结果 | 证据 |
| --- | --- | --- |
| http-transport | PASS | [证据](../../../testing/evidence/2026-09-05/TASK-20260905-004/http-transport/README.md) |
| joint-write | PASS | [证据](../../../testing/evidence/2026-09-05/TASK-20260905-004/joint-write/README.md) |
| plain-hot-replacement | PASS | [证据](../../../testing/evidence/2026-09-05/TASK-20260905-004/plain-hot-replacement/README.md) |
| rule-compilation | PASS | [证据](../../../testing/evidence/2026-09-05/TASK-20260905-004/rule-compilation/README.md) |
| rule-validation | PASS | [证据](../../../testing/evidence/2026-09-05/TASK-20260905-004/rule-validation/README.md) |
| socket-plan | PASS | [证据](../../../testing/evidence/2026-09-05/TASK-20260905-004/socket-plan/README.md) |
| final-verification | PASS | [证据](../../../testing/evidence/2026-09-06/TASK-20260905-004/final-verification/README.md) |
| http-upstream-commit | PASS | [证据](../../../testing/evidence/2026-09-06/TASK-20260905-004/http-upstream-commit/README.md) |
| integrated-verification | 历史独立范围通过，Q1当时待确认 | [证据](../../../testing/evidence/2026-09-06/TASK-20260905-004/integrated-verification/README.md) |
| local-http-default-response | PASS | [证据](../../../testing/evidence/2026-09-06/TASK-20260905-004/local-http-default-response/README.md) |
| source-size-cleanup | PASS | [证据](../../../testing/evidence/2026-09-06/TASK-20260905-004/source-size-cleanup/README.md) |
| typed-socket-io | PASS | [证据](../../../testing/evidence/2026-09-06/TASK-20260905-004/typed-socket-io/README.md) |

- 实际完成与归档时间：2026-09-06 09:59:31 +08:00。剩余任务相关风险/未确认事项：无；平台/外部业务验证限制如各证据N/A记录。

### 用户授权本地提交

- 时间：2026-09-06 10:03:54 +08:00；用户明确要求“提交”。
- 已创建本地提交 `5fd8bfbb`：`refactor: unify HTTP and Socket runtime contracts`，122个已审查代码、测试、脚本与当前架构/需求/操作文档文件。
- 提交前124文件与最终验收快照逐字节一致；暂存集合严格等于122个任务路径，暂存内容/工作区一致，diff check通过。沿用此前通过的Rust1640/前端542/守卫60等证据，无源码变化，不重复运行全量测试。
- 提交后HEAD文件集合与预期相同，代码内容一致，暂存区为空。任务与测试证据档案继续保留本地未提交；其他任务和共享索引修改保留。未push或触发CI。
- 本地提交核对记录：../../../testing/evidence/2026-09-06/TASK-20260905-004/final-verification/outputs/local-commit.json。
