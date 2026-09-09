# TASK-20260905-001：按已确认合同修复整体审查问题

- 任务 ID：`TASK-20260905-001`
- 状态：已完成
- 任务日期：`2026-09-05`
- 创建时间：`2026-09-05 14:00:06 +08:00`
- 开始时间：`2026-09-05 14:00:06 +08:00`
- 最后更新时间：2026-09-08 12:03:49 +08:00
- 完成时间：2026-09-08 12:03:49 +08:00
- 创建路径：`docs/tasks/pending/2026-09-05/fix-reviewed-architecture-contracts.md`
- 归档路径：docs/tasks/completed/2026-09-08/fix-reviewed-architecture-contracts.md
- 关键词：架构审查、Android、路由、控制租约、generation、Bootstrap、HTTP Header、Listener 草稿、合同回归
- 任务优先级：高
- 优先级理由：涉及协议、跨模块并发、生命周期和 UI 状态一致性，执行完整流程。

## 背景、目标与范围

用户要求审查整体架构并检查修复建议与已完成任务的冲突，随后确认执行。目标是在保留后续已确认合同的基础上修复仍成立的问题。

范围：原审查 #1 心跳饥饿、#2 generation 回调隔离、#3 受控恢复、#4 后端 Bootstrap cursor、#5 请求 Header、#6 Listener 草稿与权威状态、#7 前端 Bootstrap 刷新、#8 严格目标路由、#9 Listener 业务默认/转换边界、#12 当前活动文档。第 #9 先核实既有 factory 可复用边界，保留已批准的 HTTP 表单映射。
不在范围：#10 可选观测 facade 整理、#11 可选远程解析性能优化、#13 既有尺寸债务；不调整 Display 1 MiB/8192、远程 RPC 限制、Wasm 能力、Schema、固定 Root、历史档案、发布或远程 CI。

## 需求确认记录与变更

- `2026-09-05 14:00:06 +08:00`：用户对两项推荐回复“确定 执行”：#8 只有明确命中配置的域名、IP/CIDR 或 DNS 快照目标才代理，未知 IP 不按唯一端口补偿；#3 开启失联保护时等待有效桌面控制后再恢复 VPN，不建立无 owner 的 TUN。
- 原合同变化：端口补偿的历史用例按旧合同保留为差异证据；当前自动恢复不能先建立 TUN 再等随机 epoch 租约到期。开关关闭的 device-only 自动恢复、5 秒失联关闭、透明路由由桌面重新 prepare、其他设备与 Listener 隔离继续有效。
- 整体范围按用户执行审查建议的上下文采用上述缺陷修复范围，已发送可选范围澄清；若用户收窄，未依赖部分保持不实现。
- 未确认事项：已确认两项的产品行为无未决；其他分片仅在源码与既有合同足以锁定行为时进入实现。

## 需求就绪与根因

- 问题/目标/范围：PASS；不新增未确认默认、重试、降级或兼容路径。
- 当前已验证：HEAD `c7a70d3e`、工作区初始干净；routing.rs 的 unique_domain_route_for_port 对未知 IP 仅按唯一端口代理；恢复入口生成 unmanaged 随机 epoch；其余发现已在前序只读审查核对源码/任务并有前端最小复现。
- 预期：既有严格路由、5 秒失联保护、epoch 隔离、旧响应不能覆盖新事件、草稿保留、raw 请求 Header 与 chunked 响应合同同时成立。
- 复现：路由输入单域名端口+未知 IP，旧行为命中代理；恢复输入默认保护开启的保存 activation，旧行为启动后无可续租 owner；其他分片先建立失败用例。
- 根因：路由补偿放大了匹配范围；恢复入口缺少受控恢复准入；其余候选原因由各分片先回归证明再实施。
- 推断/未知：真实 Android 开机、USB 断开和新 APK 数据面尚未执行；不能用 JVM 或 Rust 单元替代真机结论。
- 需求就绪：路由 PASS；恢复方案在只读调用链核对后记录并进入实现。其余分片分别完成根因和就绪记录。

## 最小改动、最优设计与小任务

最小：删除端口猜测，复用已有桌面控制 start/prepare 和纯状态协调器；修正现有 cursor/draft/请求过滤边界。最优：明确控制所有权、权威快照和 UI 草稿、raw 协议表示、Rust 业务构造职责。优先最小边界修复，不引入新层、依赖、双实现或全局串行。

| 分片 | 所有者 | 依赖 | 状态 | 验收 |
| --- | --- | --- | --- | --- |
| A1 Android #1/#2 | Android Agent | 真实gate/epoch、generation竞态回归 | 本地验证通过，真机未执行 | 心跳不等待业务gate；当前generation原子故障清理 |
| A3 Android #3 | 主 Agent | 恢复产品选择待答 | 待确认，未修改 | 受控自动恢复或明确start/apply后的恢复 |
| B 路由 #8 | 主 Agent | 已确认 | 本地验证通过，真机未执行 | 未知目标直连；域名/IP/CIDR/DNS命中保持 |
| C 后端 #4/#5 | 后端 Agent | 确定性RED | 验证通过 | 快照期间事件可补发；逐跳请求头清理且响应合同保持 |
| D1 前端 #6/#7 | 前端 Agent | 三方草稿及事件回归 | 验证通过 | 草稿/证书保留、局部冲突解决、旧响应保护 |
| D9 Listener #9 | 主 Agent | 独立Draft DTO/接口设计 | 设计待办，未修改 | 编辑中空字段合法性与严格领域校验分离 |
| E 文档/集成验证/审查 | 主 Agent/独立架构 Agent | 实现批次全部结束并集成验证 | 已实现批次APPROVE，整任务未完成 | 检查任务相关P0/P1/P2；不将待确认分支标完成 |

## 原任务、证据与复用

- [TASK-20260828-004](../2026-08-28/android-vpn-stop-on-adb-device-missing.md)：复用默认 5 秒、关闭开关、generation/多设备隔离与错误路径；父证据待按原任务实际路径补齐。
- [TASK-20260827-001](../2026-08-27/android-multi-device-vpn-management.md)：复用 per-serial/CAS/锁顺序，不重写原档案。
- [TASK-20260904-005](../2026-09-04/fix-http-chunked-response-forwarding.md) / HTTP-CHUNKED-RESPONSE-001：复用请求/响应 wire、404、chunked、故障注入合同。
- [TASK-20260904-003](../2026-09-04/http-action-parameter-forms.md)：保留前端强类型表单/序列化与 Rust 最终校验；尺寸 7 项是已知债务。
- [TASK-20260904-004](../2026-09-04/fix-certificate-revision-semantics.md)：集合 revision 与记录 revision 不要求相等，材料校验保持。
- 前序只读复现 `/private/tmp/proxy-architecture-review-20260905/` 仅作为线索，本任务需要归档实际用例资源和新测试结果。

## 测试与文档计划

先执行原路由/租约测试并保存，再写失败回归，实施后执行受影响 Rust/Companion JVM/前端套件、fmt/lint/typecheck/架构检查和适用构建。协议用例记录输入、raw Header/Body 和实际网络结果；状态用例记录输入、时钟、generation、预期与实际。不适用 TLS/交易/远程服务为 N/A；真机不可用时记录 NOT_RUN，不声称真机完成。
证据目标：`docs/testing/evidence/2026-09-05/TASK-20260905-001/`，各分片只写自己的实际内容目录；主 Agent 持锁更新索引。独立总体对抗审查在全部分片收集与集成验证后执行。
文档影响：Android 架构/README/需求/操作说明需要检查更新；模块图/根 README/MCP 当前 resource 需要核对；历史任务/证据/ADR 原文无需更新。

## 实施与验收记录

- `2026-09-05 14:00:06 +08:00`：完成历史、锁协议及工作区状态检查，登记高优先级任务；未触发 CI、未发布。
- 修改文件、测试结果、附加文件：实施后据实追加。
- 验收结果：未完成。
- CI、push、发布：NOT_RUN，不在授权范围。
- 完成总结：未完成。

## 继续执行与细化边界

- 2026-09-05 14:36:06 +08:00：用户暂停后要求继续；保留暂停时修改，重验后续测试，未提交/推送/CI。
- #8 已进入实现：旧路由8/8 PASS归档；新单元和真实SOCKS5回归在旧实现RED，分别证明未知IP被匹配以及原目标返回ORIG但实际收到PROX；删除唯一端口补偿后Android Engine 48/48 PASS。最终格式化与集成后再次验证。
- #1 最小方案：仅heartbeat纯续租帧不等待同serial业务gate；它只读取owner/runtime快照，不写owner/forward/reverse状态；所有业务操作锁序不变，设备端继续拒绝旧epoch。必须通过真实adapter gate调用路径回归，不接受无关Mutex的伪并发证明。
- #2 最小方案：NativeSocketProtector绑定创建时generation，主线程执行故障回调时检查registry代次；保持当前实例即时fail-open。正在验证CAS与TUN清理之间竞态。
- #3 新发现的未确认事项：现有桌面重连只查询状态，不自动发start/apply。已询问用户“桌面重连后自动恢复”还是“保持停止直到桌面明确start/apply”。等待回答期间只读调查恢复入口，不实施该分支；不能把继续指令当作这个产品细节的答案。此前“无未决”的表述以本条补充为准。
- #4/#5：后端Agent已建立确定性RED，随后分别前移bootstrap cursor捕获和raw request Connection token清理；保留响应chunked/404/body/fault阶段和逐条证书revision语义。
- #6/#7：前端Agent按权威base+逐listener草稿、同listener冲突、证书lease提交后清理与相关事件才作废bootstrap的边界实施。
- #9 核实结论：现有Rust只有listener_new/copy，未完成Socket草稿的空ProtocolPackageRef不能通过严格领域反序列化；直接新增接受ProxyListener的转换命令会破坏编辑流程。需独立草稿DTO/接口设计，本轮不贸然替换现有前端转换；已批准HTTP表单映射保持。作为设计待办单独记录，不声称已修复。
- #12：活动modules依赖图与Application依赖表、rule_definition_save命令名、当前README/MCP工具数修正；历史任务/ADR/证据保持原文。

## 已实现批次当前结论

- 实现批次全部终态并完成结果收集后，才启动独立架构 Agent 审查；不在并行实现中提前进行总体审查。小任务不分别重复审查，由本次集成批次统一覆盖。整任务仍未完成，后续 #3 实现后仍需补充整任务审查。
- #1：根因为heartbeat等待可能耗时的同设备业务锁导致默认5秒租约饥饿；旧gate真实调用回归500ms超时，解耦纯续租帧后通过。心跳仍独占临时ADB forward，全球端口预留和精确serial+port清理不变；此前“不写forward”仅指不提交持久owner/forward/reverse状态，不表示没有临时forward。
- #2：根因为JNI故障未携带原generation，以及分离校验/清理允许替换start插入；携带generation并原子校验、发布faulted、清理后并发回归通过。状态锁期间清理的锁序由独立审查再次核对。
- #4：根因为bootstrap读取快照后才取cursor，期间事件被游标越过；在第一次await前取cursor，暂停settings读取期间发事件的回归证明可补发。
- #5：根因为只删固定逐跳字段而漏掉所有Connection声明字段和Proxy-Connection；精确raw头及真实TCP回归证明清理且保持无关头顺序、大小写、重复和空白。chunked/404/故障注入回归通过。
- #6：根因为外部workspace整体替换与本地draft未分清权威基线；逐Listener三方合并保留本地修改并识别同Listener冲突。显式按钮仅放弃当前冲突草稿，保留其他草稿及其证书lease；外部已删除则移除当前项。冲突时保存/启动/删除阻断，校验成功不解除冲突。
- #7：根因为所有观察事件都废弃在途bootstrap但部分事件不安排替换请求；只由相关快照事件作废并排队新快照，观察事件不作废，迟到响应保护保持。
- #8：旧单域名唯一端口补偿可代理无关IP；用户已确认删除该分支。旧8/8基线、新单元和真实SOCKS5 RED，以及新48/48 GREEN均归档。
- 对已完成任务保护：Display 1 MiB/8192与远程RPC 128 KiB是不同边界，本次不改；Wasm能力、固定Root与证书revision、chunked响应、5秒失联保护不回退。源码尺寸仍是原有7项失败，不扩大清理范围。

## 验证与稳定证据链接

| 用例 | 当前结果 | 证据 |
| --- | --- | --- |
| android-contracts | 本地回归、JVM 32/32、四ABI APK通过；真机NOT_RUN | [证据](../../../testing/evidence/2026-09-05/TASK-20260905-001/android-contracts/README.md) |
| backend-contracts | RED3；events8、message11、upstream7、fault4、chunked3通过；check/clippy通过 | [证据](../../../testing/evidence/2026-09-05/TASK-20260905-001/backend-contracts/README.md) |
| frontend-contracts | 第一轮全量573；最终改动后受影响216；typecheck/lint/boundary通过 | [证据](../../../testing/evidence/2026-09-05/TASK-20260905-001/frontend-contracts/README.md) |
| STRICT-ROUTING-001 | 单元及真实TCP/SOCKS5 RED/GREEN；Android Engine 48/48；真机NOT_RUN | [证据](../../../testing/evidence/2026-09-05/TASK-20260905-001/strict-routing/README.md) |
| CURRENT-DOCUMENTS-001 | 活动文档、Cargo依赖、MCP工具/资源标题一致性通过 | [证据](../../../testing/evidence/2026-09-05/TASK-20260905-001/current-documents/README.md) |
| INTEGRATED-VERIFICATION-001 | Rust四库1120/1120，最终Next构建、fmt、architecture通过；已实现范围审查APPROVE | [证据](../../../testing/evidence/2026-09-05/TASK-20260905-001/integrated-verification/README.md) |

Android、后端、前端用例metadata及README分别记录父任务/父用例/父证据路径、复用合同和本次变化；原始资源及实际APK、RED/GREEN日志、输入/预期/实际、复测命令均随本任务归档。修改源码列表和逐文件快照见集成证据metadata.json。

## 文档影响分析与同步结果

- 已更新：根README、modules模块图/Cargo依赖/命令名、MCP当前resource标题和对应测试、Android严格路由/heartbeat/generation边界、data-flow快照游标与事件刷新、用户操作说明Listener外部冲突解决。
- 不需修改：HTTP动作表单、Display限制、证书模型、Schema/Wasm合同，均保持既有实现；历史任务、历史证据和ADR保持原文。
- 待处理：#3恢复产品行为明确并实现后再同步Companion恢复说明；#9独立Draft DTO设计与接口文档未完成。
- CI/push/发布/安装：未执行。Android APK仅本机构建校验和按既有脚本更新本地打包资源；未向设备安装。

## 独立审查结果与后续设计

- 2026-09-05 15:09:07 +08:00：独立architect审查已实现范围APPROVE，新P0/P1/in-scope P2为0；见[报告](../../../testing/evidence/2026-09-05/TASK-20260905-001/integrated-verification/independent-review.md)。
- #9独立登记为 [TASK-20260905-002：Listener草稿DTO设计](../2026-09-05/listener-draft-dto-design.md)，先形成可评审方案；父任务不声称该点已修复。#3恢复产品选择和真机验证仍待补齐。

- 2026-09-05 16:23:52 +08:00：#9已在TASK-20260905-002完成源码方案与独立审查，确认属于保持已知行为的工程修复，进入实现。B1 Rust/绑定完成并冻结，F1前端迁移进行中；原“仅设计”的结论为前阶段历史，目前尚未宣称#9完成。#3产品选择继续等待，未将“继续”当作选择答案。

- 2026-09-05 16:59:02 +08:00：#9已由[TASK-20260905-002](../2026-09-05/listener-draft-dto-design.md)完成实现、验证与独立复审并归档。新增3个Application Draft命令，保持原保存事务与既有合同；唯一审查P2已RED/GREEN修复。原“仅设计/进行中”描述为阶段历史。当前可确定修复#1/#2/#4/#5/#6/#7/#8/#9/#12已完成本地验证；本父任务状态转待确认，#3仍等待桌面重连自动恢复或明确start/apply的产品选择，真机验证未完成，整任务不归档为已完成。源码尺寸经相关测试拆分现剩6项原有超限；Display 1 MiB保持。未提交/推送/CI。


## #3 用户确认与实施就绪

- 2026-09-05 17:19:36 +08:00：用户明确选择“等待手动启动”。原“重连自动恢复/等待手动”的未决项解除；用户没有要求停止未达到5秒超时且仍有效的运行实例，也没有要求关闭有效LAN热协调。
- 新合同：受保护VPN因失联、重启或恢复入口进入停止状态后，重连/轮询/端点刷新不自行启动数据面；等待用户明确start/apply。已有Owner保留CleanupRequired和epoch用于CAS，用户可点“应用修改”重新准备；无Owner时点“启动”。不自动删除Owner。
- 既有合同保留：默认开启及5秒失联关闭、同serial/epoch/generation隔离、关闭保护时的device-only自动恢复、透明路由必须桌面重新prepare、有效Active+匹配active_runtime的LAN热协调、原#1/#2和Task002修复及Display1MiB。
- 当前事实/根因：Android的startActivationIntent和restartSavedProfile直接生成新generation并配置随机unmanaged租约，保护开启仍会建TUN。LAN端点协调除健康快速返回外，对CleanupRequired/Faulted等或无active_runtime的持久Owner也会发送apply；独立endpoints刷新可绕过Stopped status短路。
- 复现/预期：protected saved activation → 不生成恢复generation、不enqueue/建TUN、不伪造owner租约；unprotected device-only → 继续原恢复；随后明确desktop start→有效epoch/续租。LAN恢复Owner/缺runtime证明→无隐式apply且端点不健康；Active+匹配runtime+LAN变化→保留热apply。真实设备暂无证据，仅在实际检测可用时执行授权内验证，不能用JVM代替真机。
- 方案比较：分别在Job/Service/UI加bool判断会重复规则并漏入口；采用Android既有控制租约/启动协调边界统一拒绝受保护无owner恢复，复用现有generation与失败回滚，薄入口消费明确的等待结果。LAN仅在已有有效运行归属下保留协调，不整体删除热更新，不新增协议字段/Schema/默认/兜底。
- 就绪PASS：范围、用户选择、输入/输出、错误及等待行为、最小复现和验收明确，进入实现时间为本记录时间。等待手动是正常准入拒绝，不计为启动失败；保存activation格式/用户Profile字段不迁移。
- 并行所有权：backend_fixes仅Android Kotlin生产/测试；主Agent仅Rust endpoint_reconciliation及相关测试、当前文档/元数据/集成。共享协议不改；两实现终态收集并集成后才启动整体对抗审查，不单独提前审查。前端仅事实核对，无生产变更预期。
- TDD：沿用已读test-driven-development技能，先失败回归，保护/关闭保护/手动启动/旧代回调与LAN正反例；Android JVM/仪器测试编译与release APK，Rust相关adapter/application回归、fmt/Clippy、架构与静态检查按影响执行。后续构建只在需要证明新源码时运行，不重复未变前端套件。
- 文档影响：android-companion README、requirements 8.5、Android架构/操作说明需更新为手动恢复；历史任务/ADR/旧测试资源保留；新证据manual-recovery引用原android-contracts及TASK-20260828-004/ANDROID-CONTROL-LEASE-001，不覆盖旧APK或日志。

- 2026-09-05 17:27:01 +08:00：LAN正向保持回归发现同一协调分支直接构造profile/proxy_runtime，漏掉既有control_lease；Companion startOrApply要求保护开启时携带owner_epoch与5000ms，因此不能用宽松fake声称热协调正常。为兑现本次“保留有效LAN热协调及当前租约”的门禁，复用已有lease::control_request_payload附当前owner epoch，不新增协议或默认。真实本地控制帧回归将按Companion要求拒绝漏lease的apply；恢复态拒绝、手动新epoch、热协调同epoch三者分别验证。

- 2026-09-05 17:34:32 +08:00：真实status回归进一步确认Active owner收到Stopped仍被mark_owner_reconnected提前返回为Active，保留的active_runtime会绕过端点门禁。主Agent修复范围补入owner.rs：当前epoch已停止/故障时转CleanupRequired，保留旧owner供显式apply CAS；已有清理/停止失败状态不改。RED三项真实失败已保存，未通过修改测试前提掩盖该状态错误。

- 2026-09-05 17:35:36 +08:00：方案细化：仅检查桌面Active缓存不能封住设备在自动LAN apply到达前租约过期的竞态。为落实已确认的等待手动合同，受保护自动热apply在现有payload增加require_active_lease=true；缺省false仅保留原显式start/apply语义。Companion在同一控制协调锁内验证当前epoch/代次/未过期租约后才enqueue，拒绝不生成新代/租约。先前“不新增协议字段”的方案约束由本条替代：不新增operation/Profile/Schema，只增加内部自动调用前置条件。Android Agent扩充原子准入回归，主Agent发送标志；两端集成后统一审查。

- 2026-09-05 17:39:36 +08:00：协议安全核对确认旧Companion v2会忽略未知payload字段，故可忽略布尔不足以保证旧端也不自动重启。最终采用独立apply_active operation承载自动受保护热更新的原子有效租约前置条件；旧端未知operation直接拒绝，不回退。既有start/apply、v2信封、Profile/Schema均保持；前条payload flag方案废弃。该工程修正保证用户手动恢复决定，未增加自动恢复行为。两端按最终operation统一回归。

## #3 最终实施、集成审查与剩余验收

- 2026-09-05 17:59:05 +08:00：用户“等待手动启动”的产品决定已全部落实到本地代码和当前文档，未确认产品事项为0。#3小任务本地完成；父任务因无连接Android设备而保持已阻塞，不转completed、不填写完成时间。
- 最终设计：protected saved recovery集中返回Waiting；unprotected device-only保留恢复；Active观察Stopped/Faulted转CleanupRequired；LAN自动协调要求Active+匹配runtime；受保护热更新用独立apply_active，在设备协调器/registry原子验证活租约和运行代次，旧端拒绝未知operation且无普通apply回退。显式start/apply保留新epoch准备事务。不再建立unmanaged恢复租约。
- 已完成任务保护：5秒/default-on、serial/epoch/generation、JNI隔离、严格路由、bootstrap/header/Listener Draft及Display1MiB均保持；#9已由Task002归档。原阶段“#3待选择/#9未完成”记录只作为历史，当前以本节为准。
- 实现批次均终态、收集和集成后执行独立整体审查。未单独提前审查并行子项；统一审查覆盖高风险协议/并发边界。最终APPROVE，无P0/P1/in-scope P2，见[审查报告](../../../testing/evidence/2026-09-05/TASK-20260905-001/manual-recovery/independent-review.md)。审查后仅去除Companion README“计划中”的过时用语，静态检查通过，未再改代码。
- 验证：Application432/432、Infrastructure474/474，共906；最终控制帧专项5/5；Android JVM45/45；Clippy/fmt/架构/文档检查PASS；Android instrumentation编译PASS，执行NOT_RUN。最终release APK签名、alignment、4ABI、stage通过，SHA256 ced81ae37ef0415f38dfe5d8d0d830a359bb6999ebc86a2b6a46536e03f3e1df。源码尺寸仍为6项既有超限，无新增。
- 用例与资源：[MANUAL-RECOVERY-001](../../../testing/evidence/2026-09-05/TASK-20260905-001/manual-recovery/README.md)下android为第一阶段不可变证据，android-atomic-hot-apply保存最终Android/APK，desktop保存真实RED、中间失败、最终GREEN和10个本地控制Frame，post-review-docs保存审查后措辞修正。父任务/用例/原资源关联见各metadata；23份当前源/文档快照映射见verified-current-sources.json。未覆盖历史APK/旧任务证据。
- 修改文件：Android control coordinator/manager/watchdog/protocol/server、Service/Control/Job/registry及恢复/协议/仪器测试；Rust application android白名单/帧测试，android_adb owner/endpoint/test入口与恢复回归；四份当前说明。逐文件权威清单为上述source mapping。
- 文档同步：需要更新并已完成android-companion README、requirements 8.5、Android架构、用户操作说明；docs README/证据索引和本任务同步。根README/新人指南/其他架构/ADR/MCP/外部包/示例/脚本说明/内嵌资源无需更新：未发现与#3冲突的恢复承诺，未改其接口或用法；历史档案保持原文。测试矩阵/结果由本用例及证据总索引统一登记，无另建重复活动资源。
- 真实设备、LocalSocket/TUN、真实业务链路仍NOT_RUN：adb无设备；低层PASS不替代设备验收。后续只需具备设备后按归档仪器测试/准备步骤补验，不再需要确认恢复产品选项。
- CI、push、发布、安装、Git提交：未执行。当前本地工程分支已完成；父任务剩余门禁为真实设备验收，未声称整体交付已完成。

## 本地提交授权与范围

- 2026-09-05 21:04:09 +08:00：用户明确要求“提交”。本次提交包含TASK-20260905-001与TASK-20260905-002已集成并审查通过的修复、当前文档及对应完整测试证据；两任务共享Listener编辑合同和索引，按当前已验证整体创建一次本地提交，避免拆出未经独立验证的中间状态。
- 提交前复核当前源码与最终验证快照、暂存范围、diff空白和证据完整性；源码未变化，不重复未受影响的业务测试。提交信息正文同时标明两个任务ID，可由git log按任务ID定位提交。
- 先前“未提交”的记录反映当次测试/归档时点；本次新增用户提交授权，不改写历史测试结果。Task001仍待真实Android设备验收，Task002完成状态保持；不推送、不触发CI、不安装/发布。

- 2026-09-05 21:07:54 +08:00：用户追加“不要有hash证据啥的”。提交范围相应收窄为源码、测试代码和当前项目说明；自动证据、日志、APK、包含其引用的本地任务档案及配套任务/证据索引不纳入提交，原文件保留本地。上一条“提交包含完整证据”的计划由本条替代；不新增换行属性文件，不清洗或删除原始材料，不推送。

- 2026-09-05 21:08:56 +08:00：本地提交已完成并复核，包含95份源码/测试/当前文档；生成档案、日志和APK未纳入，暂存区已清空，相关本地材料保留。未推送、未触发CI。


## 新分支续办（2026-09-07 18:43:33 +08:00）

- 用户要求：切换到新分支处理当前6项待办。已从 develop 创建 codex/complete-pending-tasks-20260907，起点 caa36022；保留原有 gmofg_cps 与 http-finalize 未跟踪目录，不纳入本次工作。
- 执行范围：原任务已确认的本地修复、证据补齐、验证和状态收尾；本轮不从历史记录推导新的 push/CI/tag/Release 授权。
- 需求就绪复核与步骤：原无设备条件已变化：只读 adb 检测到 A920MAX 2740072778 和 IM28 3330000478。先核对当前运行状态及归档真机入口，再执行可隔离的验收；不能因在线就判定验收通过。
- 优先级仍为高，适用受影响验证与独立整体审查；任务级完成门禁未满足则保留未完成状态。
- 测试证据：新增执行使用 docs/testing/evidence/2026-09-07/TASK-20260905-001/ 下独立用例，原证据不覆盖。


## 真机门禁当前状态（2026-09-07 19:21:32 +08:00）

- 原“adb无设备”条件已变化：A920MAX 2740072778（API29）与IM28 3330000478（API31）在线，均为armeabi-v7a且符合Companion平台要求。当前只读输出：[physical-device-readiness](../../../testing/evidence/2026-09-07/TASK-20260905-001/physical-device-readiness/README.md)。
- A920安装Companion，IM28未安装；两台均运行业务Launcher。SavedRecoveryAdmission仪器用例本身不建真实TUN，不能替代LocalSocket/TUN/失联/业务验收；其运行入口仍涉及APK安装/替换/卸载。现有完整门禁带模拟器约束，不直接用于物理机。
- 已向用户请求专用设备及允许短暂断网的维护窗口，尚未获得选择。未安装/卸载、启停VPN、改端口映射、关闭业务App或发送交易；父任务仍已阻塞。后续还需按原验收合同明确Profile/Listener/目标应用与受控请求响应，补齐前不归档。

## 真机操作授权更新（2026-09-08 11:06:41 +08:00）

- 用户第2项“可以”确认使用A920并允许安装测试APK、启停VPN和短暂断网。此前维护授权阻塞解除，不重复询问。主Agent先复用既有受控测试应用/服务/fixture；若没有权威目标配置，仅对缺失的目标应用/服务参数提问。业务交易不由设备测试授权推导。
- 用户第4项“手动验收合格”记录为人工验收声明；不能改写此前自动化NOT_RUN或伪造工具/签名证据。当前专门Android自动回归按第2项继续准备。

## A920受控验收就绪（2026-09-08 11:19:42 +08:00）

- 原目标203.0.113.48:16127/16627、D48模拟响应与target/isolation probe沿用现有emulator-proxy-gate fixture；仅将模拟器transport替换为物理A920 ADB reverse，v2帧/UUID/lease遵循当前已确认生产合同。旧v1 helper不直接使用。
- 本轮验证系统已有VPN授权、同签名仪器测试（目标包不卸载）、真实LocalSocket/TUN/JNI双端口映射、非目标UID隔离、停止heartbeat超过5秒退出、过期apply_active拒绝、轮询不重启、显式apply新epoch恢复、最终stop与自有端口映射清理。实际业务链路未包含，不能用模拟D48替代真实GMO服务。
- 就绪PASS；高优先级保持。主Agent拥有设备安装/运行和元数据，子Agent仅编写独立证据replay脚本，既有测试/生产代码不改变。最终测试结束后统一独立审查。
- [A920证据](../../../testing/evidence/2026-09-08/TASK-20260905-001/a920-controlled-recovery/README.md)。

## A920最终受控运行结果（2026-09-08 11:54:50 +08:00）

- 准入instrumentation2/2 PASS；真实LocalSocket v2/TUN/JNI双端口两轮4条收发逐字节PASS，非目标UID隔离PASS，5秒失联停止/旧apply_active拒绝/轮询不复起/明确apply新epoch恢复/最终stop PASS。原APK与4项原配置已恢复并读回，测试包/映射/TUN清理通过。
- 初次备份位置及两次ADB harness失败均保留；最终attempt3脚本使用targetContext备份、无-W显式wake、shell -T读取真实exit/stderr，不修改产品绕过失败。完整资源/日志/帧/复测见新增用例。
- 未发送真实GMO业务，未声称桌面UI或LAN热协调端到端；这些与既有本地合同边界分开。等待独立任务级完成审查后归档。

## 最终验收与归档（2026-09-08 12:03:49 +08:00）

- 状态已完成：既有本地实现/审查及原父任务唯一剩余的真实Android设备验收均已闭合。此前“已阻塞/真机NOT_RUN”仅为历史阶段，不改写旧证据。
- A920当前受控验收：准入2/2 PASS；双端口两轮4条request/response逐字节PASS；目标UID隔离、真实LocalSocket/TUN/JNI、5.765秒失联停止、旧apply_active拒绝、status不重启、明确apply新epoch恢复及最终stop PASS。22对控制Frame完整解析一致，88份当前输入逐字节稳定。
- 原APK完整字节与4项原配置已恢复验证，新装测试包、备份文件、forward/reverse、TUN与Service清理通过。没有将受控D48响应声明为真实GMO交易；外部业务和桌面UI/LAN实机仍N/A，原任务未将其设为本次补验门禁。
- 独立任务级审查frontend_followup_review：APPROVE，P0/P1/P2=0；任务/证据链接、资源、帧/字节、恢复及清理、复测脚本均通过。所有确认实现小项及文档/归档项已完成；旧表中的待确认/未执行是历史记录，以本节为准。
- 本轮Android生产代码未修改；仅使用与9/5父用例相同的已验证release补真机证据。未Git提交、push、触发新CI或发布。
