# TASK-20260908-001：修复 Document 整数 JSON 序列化

- 任务 ID：TASK-20260908-001
- 状态：已完成
- 任务日期：2026-09-08
- 创建时间：2026-09-08 14:52:07 +08:00
- 开始时间：2026-09-08 14:52:07 +08:00（只读调查与规划）
- 最后更新时间：2026-09-08 16:03:20 +08:00
- 完成时间：2026-09-08 16:03:20 +08:00
- 创建路径：`docs/tasks/pending/2026-09-08/fix-document-integer-json-serialization.md`
- 归档路径：`docs/tasks/completed/2026-09-08/fix-document-integer-json-serialization.md`
- 任务优先级：高（公共数字序列化、协议和配置兼容，完整流程已执行）
- 关键词：DocumentNumber、f64、整数、小数、Serialize、HTTP、Wasm
- 需求就绪：已确认的普通整数规范化范围通过；负零保留当前既有输出，不引入新行为，见执行追加。
- 修改文件：生产变更仅 `src-tauri/crates/domain/src/document/model.rs`；测试、fixture、用户/架构说明、任务与证据详见完成记录。
- 当前验收/测试/CI：本地功能、静态检查、Release绑定生成及独立审查APPROVE；真实设备/原生UI/远端CI未运行。
- 完成总结：统一修复整数JSON序列化，保留小数、字符串、负零及数值校验；验证与证据已完成。下方提案和开放问题记录为历史，最终合同以执行追加和完成记录为准。

# Document 整数 JSON 序列化修复提案

## 用户目标与授权

用户： “这个不行呢 需要修改 你先给我一个修复方案呢”。当前只交付修复提案，不进入实现。原回答仅是机制解释，用户现在要求规划修复。

## 当前证据与根因

- src-tauri/crates/domain/src/document/model.rs:10、38、185、402：DocumentNumber(f64)，整数解析转 f64，派生 Serialize 统一输出浮点数。
- src-tauri/crates/domain/src/document/tests/recursive.rs:13：原文数组 1 被测试明确断言为 1.0。
- src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs:221：Document 未变化时原文保留，变化时完整重新编码。
- src-tauri/crates/package-runtime/src/wasm_component.rs:362：Wasm JSON 桥直接 serde_json::to_string(document)，不经过 Document::to_json。
- examples/protocol-packages/gmofg_cps/src/codec.rs:10：等值保留原 body，变化走 serde_json 重编码。
- 运行环境：本地工作区源码；用户截图是旧回答，无原始交易报文/运行版本。实际设备复现未知，不宣称本轮实测。
- 候选原因：输入错误无证据；只在 UI 格式化不能解释上述重编码；领域表示与派生序列化组合有直接源码/现有测试支持。

## 原则、决策驱动与方案比较（RALPLAN-DR）

原则：数值及 JSON 类型保持；统一合同单一实现；保留既有校验与未修改原文分支；不承诺词法保真；不新增依赖或兜底。
驱动：消除重编码新增 .0；覆盖 HTTP/Wasm/嵌套值；控制迁移和兼容影响。

A（推荐提案）：保留 f64 数值模型，为 DocumentNumber 实现统一 Serialize。对已有验证范围内、没有小数部分的数值调用整数序列化，其余调用浮点序列化。不使用 epsilon、不取整非整数、不正则替换 JSON 文本。去掉派生 Serialize，保留类型/比较/Deserialize/安全整数及有限值合同。无符号/符号转换须基于现有安全范围证明。原本 2.0 在重编码时也会输出 2，这是明确行为变更，须用户确认。
优点：单一出口，覆盖 Document::to_json、serde_json::to_value/to_string、规则配置和 Wasm 桥；无需 Integer/Float 公共类型迁移。缺点：是数值规范化而非原始数字写法保留，序列化配置/观测输出也变化。
B：整数/浮点分别存储，必要时保留数字原始词法。优点：可区分原输入 2 与 2.0；若扩展完整词法可保留 2.00。缺点：涉及反序列化、比较、规则编辑器 JS number、IPC/Wasm、Schema/持久化全链路；单改 Rust enum 不能保证原文保真。只有用户要求上述保真时再制定此方案，不能与 A 混称。
只改 to_json：不覆盖 Wasm serde_json 直接序列化，排除。只改 UI 或正则删 .0：不修实际模型出口/可能破坏字符串，排除。

## 推荐验收合同（待用户确认）

- 规则输入 [0,1,2]，重编码输出 [0,1,2]，不新增 .0。
- 2.0 在重编码时输出 2；2.5 保留 2.5；字符串 "2.0" 保持字符串原值。
- 修改 result 时未修改的整数 count:2 仍为 2；对象/数组任意嵌套同样成立。
- 未修改 Document 仍保持原始 body 字节，含原文 2.0、空白和键顺序。
- JSON 仍为 number，比较/匹配规则数值结果不变；安全整数边界与非法数值拒绝不放宽。
- 本提案不实现数字词法保真、高精度数值、全报文格式保真、未约定错误行为或第三方包任意输出纠正。
- 开放决策：是否接受重编码时将原本 2.0 规范化为 2；负零序列化策略须明确并覆盖，不能沿用 CPS 历史排除作为全局默许；科学计数法不承诺原词法保留。

## 实施步骤与职责

S1 主 Agent：确认以上输出合同，完成需求就绪后登记进入实现时间；优先级建议高（公共序列化、协议边界及配置兼容）。
S2 单一实现 owner：domain/model.rs 自定义 Serialize；先补精确字符串/Value 类型回归，保留整数安全范围与失败测试；同步活动预期中原先有意断言的 .0，不覆盖历史证据。
S3 集成验证：package-runtime Wasm HTTP/Socket Encode 与 Display 输入；plain HTTP 上下行；应用规则保存/重载/导出导入与旧 2.0 配置；CPS Encode→Finalize 实际发送报文与已有 Java oracle。只修改确有失败证据的调用方，不扩大协议包业务逻辑。
S4 文档、证据、审查：解释整数数值规范化及原文保留边界，稳定状态后独立整体审查，证据归档和日期关闭；未验证设备/外部环境如实记录。

## 测试计划与失败预演

1. 单元：0、正负整数、2.0、2.5、最小非零小数、接近整数的可表示小数、安全整数正负边界；NaN/Infinity/超安全整数继续失败；负零待确认后固定预期。覆盖 parse_json 及直接 Deserialize/构造，再走三类序列化入口。
2. 兼容：旧规则 JSON 含 2.0 加载成功，保存/重新加载值和规则行为一致；配置 envelope/版本不变；生成 TS 仍为 number，bindings 确定性、lint/typecheck/fmt/相关 strict Clippy。
3. 集成：内置 HTTP 双向更改字段后 wire bytes 精确对比；无规则、不命中、同值 Set 原字节不变；json_pretty/CPS 经实际 Wasm Host 验证，Socket 复用现有组件夹具检查 Document 桥和最终 frame。
4. 端到端：CPS 本地 loopback 记录 Decode/Rules/Encode/Finalize/Sent/App 收到的正文与签名；UI 输入数组、保存重开、捕获查看类型与最终正文；真实支付系统不调用。
5. 预演风险：只改 to_json 导致 Wasm 漏修→直接 serde_json 与实际 Host 测试；粗暴取整误伤小数/负零→边界断言；配置与验签受序列化影响→旧配置 round-trip 与 Java oracle/wire 检查。
6. 复测命令从根目录使用 cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-domain document；package-runtime/infrastructure/application 按现有实际测试名定向运行。正式证据在独立 TASK 用例目录，包含实际输入、资源快照、命令、输出和 replay；本轮仅计划，不运行功能测试。
7. 原证据引用：TASK-20260907-004 / cps-checksum / docs/testing/evidence/2026-09-07/TASK-20260907-004/cps-checksum/；实施前核对实际资源清单、复用 Java oracle/签名期望并建立新 derived_from；不改父证据。

## ADR（提案，尚未采用）

Decision：推荐 A，以整数数值规范化修复自动新增 .0。Drivers：单一出口、跨边界一致、较低迁移风险。Alternatives：B 为保真目标的真实替代；局部 to_json/UI 修复不足。Why：当前问题可由统一 Serialize 覆盖。Consequences：重编码会把 2.0 输出成 2，配置 JSON/观测 JSON 也可能变化；不会补回已丢失精度/词法。Follow-ups：先确认目标，再实现和验证。

## 后续人员与执行建议

可用角色：executor、test-engineer、verifier、architect、critic。主 Agent/单一 executor 拥有 domain 模型（medium）；共享合同稳定后 test-engineer 验证独立调用方（medium）；verifier 负责最终证据（high）。任务只需一个实现 owner；需要持久目标追踪时可用 ultragoal，独立测试多时才组合 team，当前不启动。team 先完成整批测试并集成，再由 verifier 验证后关闭；ralph 仅在用户明确选单 owner 持续循环时使用。普通 App 可用原生 agents；本轮不创建 goal、不启动 CLI 工作流。

## 评审与状态

Architect：APPROVE，作为待用户确认提案；已接受补充：A 是数值规范化，原本 2.0 会输出 2，配置序列化同样受影响；负零策略必须确认。该评审不代表实现授权。

验收补充：现有 cps_checksum_wire.rs:263/275 的 normalize_numbers 会抹掉 2/2.0 差异，保留其业务语义比较，但必须另加最终原始 body 字符串/字段数字 token 的精确断言，禁止用数值归一化断言替代本问题验收。覆盖实际测试入口 actual_cps_component_matches_java_and_signs_encoded_rule_changes 和 real_cps_wasm_resigns_modified_pay_response_on_app_wire，保存 Encode 与 Finalize 后 App 收到的原始正文。

当前：Architect→Critic 顺序评审完成，均 APPROVE（仅方案质量）。即使方案评审通过，用户未确认规范化边界之前也不进入实现。

评审变更记录：补齐配置序列化影响、负零需求门禁、CPS 原始数字 token 与 Finalize 后 wire 验收。规划完成；需求状态保持待确认。

## 用户授权与执行追加（2026-09-08 15:12:58 +08:00）

用户确认“符合预期”，要求基于最新 develop 创建 hotfix/修复问题 并处理。已 git fetch --all --prune，权威 gitlab/develop 与新分支基点均为 6764210eb057bb482b86a05cd1df23d7cdea5c5c。当前分支 hotfix/fix-document-integer-json-serialization。原方案文档原样携带，无其他未提交改动。

确认 A 整数数值规范化，2.0→2；不扩大到词法保真。高优先级，执行完整适用流程。负零的 0/保留 -0.0 选项已通过异步问题等待明确回复；这不阻止准备其他回归测试，但负零相关序列化实现须等回复。需求就绪：除该边界外全部通过，当前允许调查/构造可复现测试；整体序列化实现尚未开始。

小任务：S1 root拥有domain模型/领域回归、任务/文档/证据、集成验证；S2 native executor拥有 infrastructure HTTP/CPS 测试与 package-runtime tests，输入为统一整数输出合同，验收 raw wire/Host；S3 native executor拥有 application 配置往返测试及 MCP expected-preview fixture，验收旧浮点配置加载与规范化输出/类型不变。测试文件不重叠，共享模型只由root改；子Agent禁止修改任务元数据/提交/历史证据。所有测试修改结果完成并集成后做一次整体独立对抗审查，小任务不单独审查以免对未集成状态作结论。

任务证据计划：docs/testing/evidence/2026-09-08/TASK-20260908-001/integer-json/。复用父 TASK-20260907-004/cps-checksum 资源，新增当次输入与实际结果，父证据不可覆盖。未授权远端Push/CI/MR/发布；按照仓库规则在验证归档后创建本地提交。

### 已确认范围的独立实现（2026-09-08 15:22:31 +08:00）

用户已确认的输出矩阵包含 0/1/2/2.0/2.5/字符串及嵌套值，该子范围就绪PASS并开始实现。按照零假设规则，只执行不依赖负零新策略的部分：负零继续原有 -0.0 序列化，不自行选择新行为；待回复后完成负零边界和整体需求门禁。此安排细化上条“整体序列化实现等待”约束，允许已确认且独立部分推进，未把未回答视为批准。

## 当前合同收束与验证（2026-09-08 15:47:43 +08:00）

流程细化（主 Agent 判断，并非用户对负零的新回复）：先前把“新增负零规范化”当成整个 hotfix 的就绪门禁过宽。当前源码既有 -0/-0.0→-0.0 输出已核实，新增回归也验证了其符号保持。保留这个既有合同无需用户批准新行为，因此本次只改变已确认的普通整数数值表示，负零保留不变；不把异步问题的未回复当作 -0.0→0 的授权。若用户选择将负零改成0，再按新合同处理。原先的开放讨论记录保留供追溯。

最终实施合同：0→0、2→2、2.0→2，负整数同理；有小数部分的值保持，字符串和其他JSON类型保持；负零沿用原 -0.0；未有效修改的原文编码路径保留；有限值与安全整数拒绝规则不放宽。无需Schema/版本迁移、不新增依赖、不改第三方包业务算法。

实施结果：唯一生产代码变更为 DocumentNumber 自定义 Serialize；HTTP/Wasm/配置调用方通过统一出口生效。其余为当前测试、fixture预期及文档；相关测试module排序按rustfmt调整。活动CPS wire比较由数值归一化升级为严格Number类型相等并补原始token断言，原签名及完整正文比较要求保留；不再需要会抹除 .0 差异的 normalize_numbers 辅助代码。

验证：领域109；Application477+Package Runtime23=500；HTTP管线22；前端27；Java签名14；实际Wasm双包双方向输出和CPS最终337字节正文均PASS；类型检查、ESLint、架构、500行、fmt及受影响4 crate严格Clippy通过。bindings检查进行中，整体独立审查待执行。所有采信执行source_stable=true。首轮RED证明原缺陷；HTTP新增断言曾误要求Finalize保留缩进空格，已依据现有 finalize.rs:39 compact输出修正并重跑PASS；Clippy首次items_after_statements只需移动测试内部helper定义，已修正且严格Clippy复验PASS。

证据：[integer-json](../../../testing/evidence/2026-09-08/TASK-20260908-001/integer-json/README.md)，含原始字节/hex/chunks/Java期望、配置输入、实际Wasm和源码快照、命令日志及复测入口。真实设备/外部支付/原生UI未运行；前端仅jsdom。未Push/未触发CI/未创建MR。

## 完成记录（2026-09-08 16:03:20 +08:00）

- 需求就绪与ADR：已采用方案A；实际进入已确认范围实现时间为2026-09-08 15:22:31 +08:00。普通整数数值输出无 `.0`，原2.0输出2；负零维持既有 -0.0，不新增负零政策。没有新增未确认需求。原方案条目的“待确认/未实施”只描述当时状态。
- 小任务S1/S2/S3：全部完成、集成并验证。并行成员均终态后执行整体审查；未单独重复小任务审查，因为共同序列化合同需集成后判断。主Agent整体验证与未参与实现的独立code-reviewer审查均通过，最终APPROVE，范围内P0/P1/P2为0。详见[审查](../../../testing/evidence/2026-09-08/TASK-20260908-001/integer-json/outputs/review.md)。
- 最小与最优：只替换DocumentNumber的序列化出口，复用现有构造校验和全部调用方；无需公共类型、Schema、协议包算法或依赖迁移，无新增兼容/兜底路径。
- 验收：领域109、Application477、Package Runtime23、HTTP管线22、前端27、Java14均通过。最终8项应用配置测试复验通过。实际HTTP双方向、两种Wasm包双方向、ISO8583 Socket桥、CPS Finalize后原始正文与签名通过；未变更/同值规则保留原文。领域RED保留证明原缺陷。
- 静态和构建：严格Clippy（4个受影响crate全部target/feature）、fmt、typecheck、ESLint、架构、文件行数门禁通过；Release export-bindings编译成功，连续生成结果一致且当前TS绑定未变化。未执行完整安装包构建。
- 证据：唯一正式用例 [integer-json](../../../testing/evidence/2026-09-08/TASK-20260908-001/integer-json/README.md)，所有测试命令/输入/输出/资源/复测入口均在该目录。50份源码和17份测试资源快照经SHA核验；父任务Java源码/JAR按稳定链接复用且原件未变。原始HTTP/bin/hex/chunks按字节归档。
- 先前失败：首轮新增HTTP断言错误地要求compact Finalize输出空格，修正测试后22项通过；测试helper排序及原模块排序经Clippy/fmt修正并复验。失败日志完整保留，不计为验收PASS。
- CI和交付：本地hotfix基于已fetch的gitlab/develop `6764210eb057bb482b86a05cd1df23d7cdea5c5c`；归档后按仓库规则创建范围内本地提交。未Push、MR、远端CI、发布或部署。
- 剩余验证边界：真实设备、生产支付/TLS和原生桌面UI未执行；前端27项是jsdom测试，不替代设备或业务验收。

### 最终修改与文档影响

| 范围 | 文件与处理 |
| --- | --- |
| 生产模型 | `src-tauri/crates/domain/src/document/model.rs`：统一Serialize |
| 领域回归 | `src-tauri/crates/domain/src/document/tests/{mod,recursive,number_serialization}.rs`、`src-tauri/crates/domain/tests/phase5_unified_rule_domain.rs` |
| 配置兼容 | `src-tauri/crates/application/tests/environment_configuration_document_contract.rs`、`src-tauri/crates/application/src/requirements_tests/workspace_configuration/application_backup_import/unified_portability.rs`、MCP `environment_configuration_candidate_v1/expected-preview.json` |
| HTTP/Wasm/Socket测试 | infrastructure `phase10_http_pipeline.rs`及其中`plain_json_rules.rs`、`cps_checksum_wire.rs`；package-runtime `http_document_number_wire.rs`、`repository_components.rs` |
| 需要更新，已同步 | `docs/architecture/rules-and-protocol-packages.md`、`docs/user-operation-guide.md`：数字规范化、负零和原文保留边界；任务/完成/证据索引 |
| 无需更新 | 根README、新人指南、需求、ADR目录、MCP指南、Android文档、示例README、脚本说明和内嵌资源：操作/API/架构职责/包ABI/设备流程未改变；本任务保存决策和测试矩阵，现有示例原文保留说明仍有效 |
| 无需删除/合并 | 没有被替代的实现、依赖或权威文档；历史证据不改写 |

Skill记录：规划使用oh-my-codex ralplan/plan，执行参考rust-router/m05-type-driven；来源为当时已安装的SKILL.md。规划评审替代重复方案审查，原生Agent负责独立测试与整体审查；项目任务登记、锁、证据、审查、归档和本地提交门禁全部保留。未启动OMX CLI team/ralph运行时。
