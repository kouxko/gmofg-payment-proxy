# TASK-20260905-002：Listener 草稿 DTO 与领域构造边界修复

- 任务 ID：`TASK-20260905-002`
- 状态：`已完成`
- 任务日期：`2026-09-05`
- 创建时间：`2026-09-05 15:09:07 +08:00`
- 开始时间：`2026-09-05 15:09:07 +08:00`
- 最后更新时间：`2026-09-05 21:04:09 +08:00`
- 完成时间：`2026-09-05 16:59:02 +08:00`
- 创建路径：`docs/tasks/pending/2026-09-05/listener-draft-dto-design.md`
- 归档路径：`docs/tasks/completed/2026-09-05/listener-draft-dto-design.md`
- 关键词：Listener、Draft DTO、Rust factory、编辑态、领域校验、合同保留
- 任务优先级：高；源于已授权整体架构审查 #9，跨前端/应用/领域公共合同，设计工作不等于新增API实现授权。

> 下列初始登记内容保留阶段历史；当前范围、实现就绪和执行结果见“源码核对后的范围细化”及其后续记录。

## 背景、目标、范围

由 [TASK-20260905-001](../../pending/2026-09-05/fix-reviewed-architecture-contracts.md) #9 分离。现有前端Listener转换/默认逻辑存在边界问题，但未完成Socket草稿的ProtocolPackageRef空字段不能直接经严格领域DTO反序列化。目标是形成基于源码的Draft DTO/API具体方案和可判断的验收标准，再按需求门禁进入实现。

当前范围：只读核实现有字段、factory/命令/调用方，比较最小方案与正确职责，给出具体输入输出、合法编辑态、失败边界、测试与迁移步骤。
不在当前实现范围：新增IPC、改领域反序列化、改变现有默认值、Schema、TLS证书lease、HTTP表单映射或Socket编辑行为。

## 需求确认与就绪

- 用户已要求架构审查并确认执行修复；第9点先核对既有factory可复用边界，不能破坏已完成任务。
- 设计工作就绪：PASS，目标是明确方案，不选择未知产品语义；进入设计时间同开始时间。
- 实现就绪：NOT_READY，需先形成具体DTO/接口方案，区分可由工程决定的内部结构与会改变产品行为的确认事项。
- 具体例子：用户切换到Socket协议模式但尚未选择协议包，空id/version是编辑中状态；不得因把ProxyListener直接送入严格反序列化而导致切换失败，也不得把未完整草稿当作可启动配置。

## 当前事实、根因与边界

- 当前已验证线索：原任务已核实Rust提供listener_new/copy；ProtocolPackageRef严格反序列化拒绝空包标识。
- 根因候选：用于保存/启动的完整领域DTO与中间编辑态共用表示，前端被迫复制构造/转换规则。只读设计切片将补具体符号和证据。
- 未知：需要跨边界搬迁的字段与最小输入输出；在方案完成前不引入默认/兼容层。
- 复现与验收：基于现有切换/编辑/保存回归，方案必须列明编辑合法、保存/启动拒绝不完整的两级门禁。

## 最小改动与最优设计

待源码切片补齐；优先复用Rust factory和已有业务校验，UI保留已批准强类型表单映射，不创建长期双执行路径。

## 小任务、测试、审查与文档

| 小任务 | 所有者 | 依赖 | 状态 | 验收 |
| --- | --- | --- | --- | --- |
| 设计事实/方案 | backend_fixes只读切片，主Agent整理 | 当前源码 | 进行中 | 字段、命令、编辑态、方案、测试和待确认项明确 |
| 方案审查与文档验证 | 独立审查/主Agent | 方案完成 | 待实现 | 不破坏既有合同；结构/链接/依据正确 |
| 实现 | 未分配 | 需求门禁 | 待确认 | 未授权的产品语义不得默认为实现 |

纯设计验证检查权威源码、引用、结构和方案可实施性；协议报文、UI截图、构建/CI不适用于设计阶段，后续实现按受影响边界补测试。

## 原任务与测试复用

- 父任务：TASK-20260905-001；父用例frontend-contracts与INTEGRATED-VERIFICATION-001。
- [前端父证据](../../../testing/evidence/2026-09-05/TASK-20260905-001/frontend-contracts/README.md)：保持逐Listener草稿、冲突、权威revision与证书lease。
- [集成父证据](../../../testing/evidence/2026-09-05/TASK-20260905-001/integrated-verification/README.md)：保持已实现批次合同。
- HTTP强类型表单、Socket空包编辑、TLS草稿材料必须成为后续自动回归的明确输入/预期。

## 实施、验收、测试和交付

- 当前只创建任务并进行只读设计；未修改生产实现。
- 修改文件：本任务文档与任务入口；附加方案待切片完成后补充。
- 验收/测试：未完成；CI/推送/发布：NOT_RUN，不在授权范围。

## 源码核对后的范围细化（本节为当前方案）

原阶段只读设计已完成，确认全部当前产品行为可以从源码和53项既有转换测试锁定。保持行为的Application DTO位置、Option表示和Rust factory复用属于工程选择，不因实现量或新增DTO本身要求用户重复确认。原“当前只设计、NOT_READY”的记录保留作阶段历史；现进入方案修订审查，审查APPROVE后实施原整体审查已授权的#9修复。

仍排除：恢复被切换掉的旧草稿/旧上游/TLS、记忆已关闭的TLS证书、取消首次推荐包、删除旧IPC、规则动作表单、数据库/备份/MCP/runtime/schema变化。#3 Android恢复选择不属于本任务。

### 当前已验证事实与根因

- `listener-data-plane.ts:13/46/65/73`重复HTTP/Socket默认和数据平面重置；`request-routing-card.tsx:31/118`承担HTTP响应模式转换；`socket-listener-model.ts:27/50/54/78`承担工作模式、TLS、空包和拓扑转换。
- Rust权威默认已存在：domain `listener_model.rs:204/332/490`、`socket_topology.rs:11/84`；application `facade/listeners.rs:21/32`已提供new/copy。
- `ProtocolPackageRef`的id/version和`CertificateReferenceId`严格反序列化不能表达空字符串；TS使用空字符串作为未选包/身份/trust哨兵，导致IPC在进入字段校验前失败。
- 已确认因果链：不完整编辑态与严格领域配置共用ProxyListener → 前端复制构造并使用非法强类型哨兵 → 严格serde提前失败。修复Application草稿边界，不能放松domain。

### 最小方案与最优设计比较

| 方案 | 评价 |
| --- | --- |
| 仅新增接受ProxyListener的转换函数 | 无法接受缺包/身份/trust的合法编辑态，不能解决根因，拒绝 |
| 5个Draft API（new/copy/transition/validate/save） | 新建与保存重复，可能复制事务或重入gate，独立审查要求收缩，拒绝 |
| 3个Draft API + 既有new/save | 采用；仅新增copy/transition/validate，复用默认、严格保存事务和领域校验，无第二运行/保存路径 |

### 最终共享合同

1. Application `ListenerDraft`镜像Listener公共字段与data_plane；只有实际可暂缺字段改Option：Socket scripted package、Socket下游TLS server_identity、Optional/Required client-auth trust（复用该模式的HTTP下游TLS也使用Draft auth）。其他原本可反序列化的空host/port/HTTP URL保留primitive；已有Option字段复用。Malformed enum/非空但非法UUID/package仍fail-closed；不支持任意字符串宽松解码。
2. 前端本地`ListenerWorkspaceDraft`仅listeners改为ListenerDraft[]；权威ProxyWorkspace/ProxyListener、全局WorkspaceValidationViewModel、DB/备份/MCP/runtime不变。完整严格对象是Draft合法子集，只做形状映射不添加前端默认。
3. 保留`listener_new`（Rust默认）与`listener_save`（唯一保存事务）。只新增`listener_draft_copy(source)`、`listener_draft_transition(draft,intent,recommended_package?)`、`listener_draft_validate(workspace_id,expected_revision,draft,certificate_references)`。旧命令保持既有消费者合同，不新增draft_new/save。
4. transition为单一tagged intent：DataPlane、HttpResponseMode、SocketTopology、Processing、AppTLS、ServerTLS；使用Rust已有factory和纯转换函数。正常leaf文本/数值/证书/包选择留在UI，不每次IPC。Rust不处理草稿证书导入/discard/commit。
5. `ListenerDraftValidationViewModel { valid, normalized: Option<ProxyWorkspace>, field_errors }`为专用结果，不修改全局VM；不变量`valid == normalized.is_some()`。先做workspace存在性与revision CAS，再返回Draft字段错误，再复用现有领域/包校验；不能过期草稿先报缺字段。持同一mutation gate期间不得再调用取得该gate的public方法，抽取现有内部helper复用。
6. valid成功后UI使用严格normalized Workspace继续既有listener_save/start路径。invalid的normalized为None且绝不调用save/start；保存事务再次CAS/运行锁/证书/领域/包验证，既有错误传播保持。
7. 异步transition防陈旧：UI捕获workspace id、listener id、当前draft版本/指纹与请求序号；只有当前作用域和draft仍匹配且是最新请求才apply。快速连续切换、名称等leaf编辑、外部权威刷新、切换Listener/Workspace、卸载均不能被旧响应覆盖。转换失败按现有可见错误通道传播，不伪造成功，不隐式改配置。
8. 证书collector仅收集Some引用；applyDraftWorkspace/prune/lease仍在UI，detach只清真正不可达且未持久化材料。保存beginCommit仅保护normalized当前Listener引用，成功后commit，失败保留原有cleanup合同。

### 必须保持的转换行为

- 同模式无操作；HTTP↔Socket采用目标Rust default，公共ID/name/port等保持原切换行为。
- Relay→LocalResponder清上游及Server TLS，保留App TLS和已有exact package；仅首次Direct→scripted才使用推荐包或None，不升级/替换已有绑定。
- LocalResponder→Relay创建原有空host/0port，不恢复隐藏旧Server TLS，保留App TLS/processing；LocalResponder→Direct同时回Relay。
- App/Server TLS独立；开启时使用已有默认（待选identity=None，verify_hostname=true，trust/client identity=None），关闭只清对应侧。HTTP路由映射保持既有request_target/fixed_server/local_server行为。
- 复制仅新ID、副本名、enabled=false，保留其他配置；不完整草稿也可复制。

### 小任务、所有权与依赖

| 小任务 | 所有者 | 依赖 | 验收 |
| --- | --- | --- | --- |
| B1 Rust Draft DTO/纯转换/校验/IPC/bindings与回归 | backend_fixes | 方案审查APPROVE | Option/strict往返、53项转换矩阵、CAS优先、valid/normalized约束、无新保存路径 |
| F1 前端迁移/异步竞态/lease回归 | frontend_fixes | B1合同和bindings稳定后才开始 | UI无领域默认/转换执行，invalid不保存，旧响应不覆盖、lease/冲突保护 |
| I1 主线集成/文档/独立整体审查 | 主Agent/独立Agent | B1和F1均终态收集并集成 | 新P0/P1/in-scope P2清零；父任务合同不回退 |

当前批次不并行改未稳定共享接口。Agent不得撤销父任务改动，不修改自己之外的共享文件；主Agent持锁管理元数据。

### 测试与可复用证据

- 修改前13个源码/既有测试快照已保存；既有Socket转换53/53 PASS，日志baseline-transition-tests.log。
- Rust：defaults parity、完整strict→Draft→strict exact roundtrip、copy身份、模式矩阵/TLS双侧/idempotent/exact package、缺字段字段错误、malformed typed input拒绝、stale revision优先、invalid=None/valid=Some、保存唯一事务回归。
- Frontend：invalid不调用save，valid走旧save；rapid transition/leaf编辑/外部刷新/切换作用域的迟到响应；父任务草稿冲突、证书detach/commit/unmount；HTTP action表单保持。
- 受影响Rust application/domain tests、command与bindings、前端typecheck/lint/test、architecture、最终build；源码尺寸不得新增债务。实现完成后独立整体审查。
- [本任务证据](../../../testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/README.md)，parent关系见metadata；旧快照/日志不覆盖，新RED/GREEN顺序追加。

### 方案审查记录

独立architect首次结论REQUEST CHANGES：5→3 API、CAS优先、专用normalized Option、transition防陈旧、lease可达性、文档门禁。以上全部已写入本节，等待修订方案复核。尚未修改生产源码。

## 实现就绪与进入时间

- 2026-09-05 15:23:24 +08:00：独立architect复核修订方案APPROVE，无剩余实现前阻断；原NOT_READY已由完整DTO/API/转换/错误/并发/测试合同解除。实现就绪PASS，进入实现时间为2026-09-05 15:23:24 +08:00。
- 所有既有行为有源码/测试依据，无新增待确认产品语义；B1先执行TDD与Rust/bindings，F1等待合同稳定。

## B1交接与F1进入记录

- 2026-09-05 15:57:14 +08:00：B1全部验证与证据完成并进入终态后启动F1；公开签名冻结，未在共享合同未稳定时并行迁移UI。
- B1：Application Draft 12/12、既有listener_validation 2/2、命令/绑定1/1、双次release绑定fresh/deterministic、Application/Tauri Clippy -D warnings、fmt/diff/新增文件500行门禁均PASS。见[后端证据](../../../testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/backend/README.md)。domain、原new/save、前端均未被B1改动。
- 绑定：listenerDraftCopy(source)、listenerDraftTransition(draft, transition, recommendedPackage|null)、listenerDraftValidate(workspaceId, expectedWorkspaceRevision, draft, certificateReferences)；generated `ListenerDraftTransition`以type区分data_plane/http_response_mode/socket_topology/processing/app_tls/server_tls。错误协议组合明确返回LISTENER_DRAFT_TRANSITION_NOT_APPLICABLE，不隐式切协议或成功。
- F1拥有Listener模块源码/测试和新增协调hook，不修改Rust/generated；如需跨feature共享文件修改先报告。主Agent同时在冻结Rust源运行application/domain/infrastructure完整库测试，前端编辑不修改这些被测对象。

### 异步迁移补充门禁（保持既有同步操作顺序）

1. 不能对不同控件的连续意图简单“最后响应获胜”：App TLS开→Server TLS开，最终须两侧开启；Direct→Scripted推荐P→LocalResponder推荐Q，最终须保留exact P。
2. 同一批结构意图在Rust返回中间Draft上按用户顺序组合，中间结果不覆盖UI，只把有效的最终结果写回。叶编辑、外部权威、切换Listener/Workspace/卸载令整批失效，旧队列尾部不再发请求；新批可独立开始，旧批迟到不覆盖。
3. 失效epoch在事件入口/统一state setter同步推进，不能仅依赖effect同步ref。测试包括同tick叶编辑后Promise resolve、A→B→A选择和旧批晚于新批返回。
4. 在途转换不能让save/start/TLS probe提交旧模式；单独约束提交动作而不锁整个editor，快速模式选择和叶编辑仍可操作。已有导入/保存/运行锁也不得让旧转换重新落地。
5. 失败按现有可见错误通道结束本批，无默认成功或兜底继续。证书lease仍只在有效最终草稿的可达性变化时处理。
6. HTTP tls.enabled为普通bool叶字段；客户端认证模式/信任选择仍是既有强类型字段映射，空哨兵改null，保留原first eligible trust选择，不额外增加认证intent或新策略。

### 文档影响

更新现有modules的Application草稿所有权，以及data-flow的专用校验、3API/唯一保存路径、意图组合与证书lease边界。最终前端实现和验证后核对操作说明；历史父证据保留，新增文档快照归本任务。

## 集成发现：归档资源被当前源码工具扫描

- 2026-09-05 16:11:39 +08:00：F1 typecheck发现TASK001/002的resources源码快照被纳入当前项目，部分相对依赖缺失导致Cannot find module。当前已验证tsconfig include **/*.ts、**/*.tsx但只exclude node_modules；ESLint也未排除资源快照。根因是当次新增源码归档与工具全目录发现边界冲突，不是Listener生产代码错误。
- 修复边界/所有权：主Agent仅修改tsconfig.json和eslint.config.mjs，排除docs/testing/evidence/**/resources/**；不改历史快照，不排除src、实际测试/脚本或整个docs。归档资源不是活动源码，显式生产import仍按TypeScript依赖规则检查。
- 方案：改快照扩展名会改变已链接历史资源路径；排除所有docs范围过大。采用最小资源目录排除，不改变产品或公共配置合同，属于本任务证据/验证所需集成修复。
- 验证：先用真实TypeScript解析配置和ESLint.isPathIgnored复现资源被发现，再修改并确认归档资源被排除、当前src和实际配置/脚本仍包含；最终全量typecheck/lint/build在F1源码冻结后执行。禁止仅过滤错误后把门禁标PASS。

- 2026-09-05 16:23:52 +08:00：归档发现边界已完成RED→GREEN。真实TypeScript解析和ESLint.isPathIgnored证明原快照被扫描；窄排除后4项比较通过，src、活动测试及vitest.config继续检查。见[配置证据](../../../testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/integrated/config-boundary/README.md)。未改历史资源。最终typecheck/lint/build仍待F1冻结后验收。


## I1整体审查发现与修复边界

- 2026-09-05 16:45:03 +08:00：B1/F1全部终态、结果收集、最终Next构建/架构/快照一致性检查完成后，独立architect给出REQUEST CHANGES：无P0/P1，1个in-scope P2。
- 当前已验证：Application ServerTls intent仅检查Socket，不检查Relay；set_server_tls遇LocalResponder直接return，外层仍Ok。直接调用新IPC会把不适用操作报告成功，违反新Draft命令已批准的NOT_APPLICABLE合同。UI隐藏/队列拦截不能替代接口边界。
- 最小复现：LocalResponder Draft分别发送ServerTls enabled=true/false，预期LISTENER_DRAFT_TRANSITION_NOT_APPLICABLE；现返回Ok。根因是旧内部no-op辅助函数被新公共intent直接复用，漏掉拓扑准入。
- 最小/最优方案：仅修Application transition和Rust requirement回归；在ServerTls分支严格匹配Relay，错误组合返回既有typed error。可把私有helper收窄为Relay参数以消除内部静默return，无新API/默认/兼容/前端变化。既有Relay双侧TLS行为不变。
- 修复就绪PASS：已确认根因、输入输出和既有错误合同，无新增产品选择。B1负责该2文件和独立新review-fix证据；主线保留旧快照，定向测试/命令/Clippy/fmt后复审，不重复未受影响前端构建。


## 最终完成与验收（本节为当前结论）

- 2026-09-05 16:59:02 +08:00：B1、F1、I1全部完成，任务状态已完成。原初始阶段NOT_READY/待实现记录保留为历史，不是当前状态。
- 结果：PASS_WITH_KNOWN_UNRELATED_SOURCE_SIZE_FAILURE_AND_NATIVE_UI_NOT_RUN。独立整体审查先REQUEST CHANGES（唯一P2），最小修复后复审APPROVE；本轮P0/P1/in-scope P2=0。
- B1：Application Draft与严格领域分离，只新增copy/transition/validate三命令，保留原new/save与111旧命令声明；CAS先于字段校验，invalid无normalized，错误组合typed error。最终ServerTls只接受Relay，helper不再静默忽略LocalResponder。
- F1：结构默认/转换收敛Rust，UI保留叶编辑、串行意图协调、同步epoch失效和证书lease。保留父任务逐Listener冲突、局部放弃、权威revision、迟到响应和其他草稿。提交/TLS probe在途门禁、旧平面/旧Server控件门禁均有回归。
- 验证窗口：初始Rust三库969/969；正式全前端536/536，随后受影响72/72与控件39/39；最终P2修复后Draft13/13、旧validation2/2、command1/1、严格Clippy/fmt/diff通过。不得把不同窗口合并为虚构的一次全量执行。
- 集成：最终Next生产构建、类型、lint、架构边界通过；58个最终源/配置/文档资源逐hash与当前源一致，P2两文件使用明确的新快照覆盖映射，旧证据保留。前端/公开合同不受P2纯Rust适用性修复影响，未重复无关构建。
- 尺寸门禁：仍FAIL，剩6个原有无关超限；Listener主页面491行，相关主测试拆到500行以内，未新增尺寸债务。未顺带清理原#13。
- 修改文件：Application models/listener_draft及facade/listeners/draft、既有listeners验证helper、Tauri listener命令/registry/generated；Listener组件、workspace草稿/持久化/证书/transition协调及相应测试。完整逐文件来源与最终快照见下方证据，不用阶段tracked diff替代新增文件清单。
- 文档同步：modules、data-flow、user-operation-guide；明确Application编辑态/唯一保存事务、异步切换和不完整草稿行为。tsconfig/eslint仅排除证据resources目录，真实配置API RED/GREEN证明当前源码/活动测试继续检查。历史资源未改写。
- [总证据与全部用例](../../../testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/README.md)；[后端](../../../testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/backend/README.md)、[前端](../../../testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/frontend/README.md)、[集成](../../../testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/integrated/README.md)、[P2修复](../../../testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/backend-review-fix/README.md)、[独立审查](../../../testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/integrated/independent-review.md)。各用例包含实际命令、输入/预期/输出、原资源/源码快照、来源及复测入口；N/A项逐项说明。
- 限制：native desktop UI/真实设备NOT_RUN；使用jsdom关键交互/可访问性状态断言，未声称原生点击验收。CI/push/commit/安装/发布均NOT_RUN。无新增依赖、产品默认、Schema/Wasm/Display限制改动。
- 父任务#9由本任务关闭；#3恢复方式选择与父任务真机验证不属于本任务完成范围。

- 2026-09-05 17:01:03 +08:00：归档后最终一致性检查发现总证据metadata.validation仍为最初设计阶段说明，已更正当前验证摘要和更新时间；更正前原始JSON完整保留为integrated/metadata-before-final-label-correction.json。源码、测试输出、历史资源和完成时间未改变，任务仍已完成；仅修正当次交付记录的残留标签。

## 本地提交授权与范围

- 2026-09-05 21:04:09 +08:00：用户明确要求“提交”。本次提交包含TASK-20260905-001与TASK-20260905-002已集成并审查通过的修复、当前文档及对应完整测试证据；两任务共享Listener编辑合同和索引，按当前已验证整体创建一次本地提交，避免拆出未经独立验证的中间状态。
- 提交前复核当前源码与最终验证快照、暂存范围、diff空白和证据完整性；源码未变化，不重复未受影响的业务测试。提交信息正文同时标明两个任务ID，可由git log按任务ID定位提交。
- 先前“未提交”的记录反映当次测试/归档时点；本次新增用户提交授权，不改写历史测试结果。Task001仍待真实Android设备验收，Task002完成状态保持；不推送、不触发CI、不安装/发布。

- 2026-09-05 21:07:54 +08:00：用户追加“不要有hash证据啥的”。提交范围相应收窄为源码、测试代码和当前项目说明；自动证据、日志、APK、包含其引用的本地任务档案及配套任务/证据索引不纳入提交，原文件保留本地。上一条“提交包含完整证据”的计划由本条替代；不新增换行属性文件，不清洗或删除原始材料，不推送。

- 2026-09-05 21:08:56 +08:00：本地提交已完成并复核，包含95份源码/测试/当前文档；生成档案、日志和APK未纳入，暂存区已清空，相关本地材料保留。未推送、未触发CI。
