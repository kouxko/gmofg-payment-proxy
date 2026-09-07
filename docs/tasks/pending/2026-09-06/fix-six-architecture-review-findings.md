# 修复六项架构与逻辑审查发现

## 任务信息

- 任务 ID：TASK-20260906-001
- 状态：待确认
- 任务日期：2026-09-06
- 创建时间：2026-09-06 10:48:57 +08:00
- 开始时间：2026-09-06 10:48:57 +08:00
- 最后更新时间：2026-09-06 15:55:13 +08:00
- 完成时间：未完成
- 创建路径：docs/tasks/pending/2026-09-06/fix-six-architecture-review-findings.md
- 归档路径：docs/tasks/completed/<实际完成日期>/fix-six-architecture-review-findings.md
- 优先级：高；涉及运行中 Android 配置、并发提交、Socket 内存与错误合同。
- 关键词：Android、LAN、apply、Rules、竞态、Socket、资源预算、EOF、外部包观测

## 背景、目标与范围

用户先要求审查现有架构与逻辑，后明确要求“处理一下”。本任务修复已报告的 2 项 P1 和 4 项 P2，补充可重复回归和独立整体审查。基线为当前 HEAD 5fd8bfbb；本次开始时已有文档索引及 2026-09-05/06 任务与证据改动，全部保留。

范围：Android LAN 热协调的活动配置身份；Socket 诊断预分配和读取块约束；规则创建重复提交与迟到保存；Exchange EOF 元数据与 Socket 外部错误观测。

不在范围：未证实的 Android 多控制端 epoch 问题、其他功能改造、新依赖、数据库迁移、真实外部业务、远程 CI 和发布；本地代码提交在后续明确授权后进入范围。本地修复不能代替 Android 真机/TUN 或发布验收。

## 需求确认记录与就绪门禁

- 用户“处理一下”确认执行上一轮六项问题的修复及回归验证。
- 五项行为修复以当前需求/代码合同为准：保存不等于应用；同一次保存仅提交一次；迟到结果不覆盖新选择；失败保留真实阶段/方向；两种协议保持相同外部错误关联字段。
- Socket 诊断预算按当前事件数与逻辑字节预算执行，不增加任意默认上限。
- 尚未确认：Socket 单次读取缓冲绝对上限。已通过结构化问题提出 1 MiB 推荐值与 16 MiB 选项；用户答复前不修改该上限或静默裁剪配置。
- 五项行为修复及诊断队列分配：输入、输出、现状、预期、根因及可判定回归均明确；就绪检查 PASS，进入实施时间 2026-09-06 10:48:57 +08:00。
- 读取块上限分支：待确认，仅继续调查/构造非危险测试；不阻塞其他独立分支。

## 现象、证据与根因

| 项目 | 当前已验证现象 / 复现 | 预期依据 | 已确认根因与影响 |
| --- | --- | --- | --- |
| P1 Android | 运行中 Profile 仅保存新目标应用/弱网参数，LAN 地址改变后 status 路径以最新保存 Profile 构建 apply_active/apply；静态调用链已核对，尚无真机复现 | requirements.md 363-374：仅显式 start/apply 应用 Profile；现存活动 LAN 只允许刷新端点 | endpoint_reconciliation 未比较活动 Profile 指纹，持久草稿进入隐式应用路径 |
| P1 Socket 内存 | 有效上游下 u32::MAX 读取块/事件容量均通过校验；with_capacity 和 vec! 在预算/取消检查前直接预分配；未执行危险巨额分配 | SocketRuntimeLimits 显式资源合同；诊断队列同时受数量和字节预算限制 | 诊断容量直接变预分配；读取块无已确认上限，资源耗尽可使进程退出 |
| P2 重复创建 | 临时副本两次 Save 交错 materialize，期望一次 rule_id:null 请求，实际两次；原测试失败日志已保留 | 一次用户保存事务只创建一条规则 | 子组件锁未覆盖 materialize，父回调捕获旧 pending |
| P2 迟到保存 | Save A 未返回时选择 B，B 已显示后 A 响应覆盖编辑器；新增断言失败 | 已存在选择代次保护，旧异步结果不得覆盖当前选择 | save 写回未绑定选择代次 |
| P2 EOF 元数据 | Server 应答前 EOF，返回 Error 的 code/stage/direction 均为空；新增 Read 断言失败 | exchange-pipeline.md 的强类型错误元数据合同 | Exchange::fail 仅日志记录已知 read/downstream，未放入返回错误 |
| P2 外部错误观测 | Socket 外部包失败的 method/request_id 等未进入 ExchangeObservation；同 HTTP 断言失败 | HTTP 已有 typed external failure 投影与 UI 解析合同 | failed_with_context 的 Socket 分支漏投影 external_* 字段 |

历史依据只作定位：2026-09-05 架构记录曾暂停的 HTTP 风险不作为本任务发现。当前源码与新测试为权威。未知：真实设备网络切换、跨平台/远程业务结果；不会把本地测试描述为真机或发布通过。

## 最小改动与最优设计

| 方案 | 分析 |
| --- | --- |
| 最小改动（采用） | 在现有 Android reconciliation 校验活动身份；规则保存持有同一异步所有权并隔离迟到写回；Socket 队列按需分配并遵守既有预算；EOF 使用已有错误分类；补全 Socket 外部字段。沿用现有端口、领域验证与测试，不增加新依赖或双路径。 |
| 最优设计 | 活动运行事实与持久草稿分开；一次保存事务持有稳定所有权；资源在分配前受明确合同约束；错误与观测在正确边界完整投影。上述局部边界修复即可满足，无需新增框架或改变协议拓扑。 |

## 小任务与所有权

| ID | 内容 / 所有权 | 依赖 / 并行 | 状态 | 验收 |
| --- | --- | --- | --- | --- |
| S1 | Android LAN：infrastructure/adapters/android_adb 与必要 application Android 文件 | 可并行；不碰 UI、Socket | 已实现并验证 | 保存 Profile 漂移不发 apply；纯 LAN 端点变化仍正确热协调；显式 Apply 保持 |
| S2 | 规则 UI：src/features/rules 的编辑、保存与回归 | 可并行；不碰后端 | 已实现并验证 | 快速重复点击只保存一次；迟到完成不覆盖新选择；错误后可继续编辑 |
| S3 | Exchange：crates/exchange 与对应 proxy EOF 映射测试 | 可并行；主 Agent | 已实现并验证 | 真实 EOF 保留 Read/Downstream 与现有分类；Socket 外部字段完整 |
| S4 | Socket 资源：proxy/socket_relay observer/io/config、domain/application 验证与必要绑定 | 诊断部分可并行；上限依赖确认；不得与 S3 共改文件 | 待确认 | 极值不触发初始巨额分配；预算/错误/取消及边界测试通过 |
| S5 | 集成、文档、证据、独立整体审查与归档 | 当前独立实现批次已集成；主 Agent 独占索引 | 已实现子集通过；整任务待 S4 | 全部回归、静态检查与独立 APPROVE；未验证层明确记录 |

所有 Agent 共享工作区，不得撤销他人修改；子 Agent 仅修改分配范围，不登记编号、不操作任务索引、不提交。共享文件/合同冲突先报告主 Agent。整批全部结束并完成集成验证后才开始整体对抗审查。

## 测试与文档计划

采用 test-driven-development 技能；先保存回归 RED 输出，再实施并保存 GREEN。最小验证为 Android fake runner 控制请求/身份测试、规则组件交互测试、Exchange/Socket 集成与诊断内存预算测试；完成后运行 Rust workspace 全量、前端全量、类型、零警告 lint、fmt、strict Clippy、架构和源文件尺寸门禁。只执行受影响检查；不触发 CI。

证据计划：android-active-profile、rules-save-ownership、exchange-error-observation、socket-resource-limits、integrated-verification。每项在 docs/testing/evidence/2026-09-06/TASK-20260906-001/ 下归档 README、metadata、实际测试源码/资源、输入输出和复测入口；临时探针与正式验收区分。UI 使用真实组件测试的操作/状态/可访问名断言；原生 App 点击与 Android 真机不适用本次本地代码回归验收，记 NOT_RUN，不能替代设备/发布验收。

后续复用：
- 父任务 TASK-20260905-001，父用例 MANUAL-RECOVERY-001，父证据 docs/testing/evidence/2026-09-05/TASK-20260905-001/manual-recovery/；保持手动恢复与有效 LAN 热协调，新增保存配置漂移反例。
- 父任务 TASK-20260905-004，父用例 final-verification，父证据 docs/testing/evidence/2026-09-06/TASK-20260905-004/final-verification/；保持统一 Exchange、raw 特化及原有回归行为，新增四项原测试遗漏的边界。
- 上一轮临时复现为本任务起因，复制实际报告、日志和新增测试资源；初始错误 fixture 保留并标注，不算产品 RED。

文档同步：只在相关架构/需求文档补充已落地的行为/明确确认的资源合同，不重写历史任务；任务与索引写入遵守全局锁。

## 对抗审查计划

各并行小任务不提前进行单独审查，统一在全部集成且测试通过后由未实施该任务的独立 code-reviewer/architect 检查六项根因、合同、并发/资源/错误边界和证据；任务相关 P0/P1/P2 必须清零并取得 APPROVE 才归档。

## 实施、验收与完成记录

- 实施记录：S1-S3 和 S4 诊断队列已实现、回归并独立复审通过；读取块绝对上限待用户具体数值。
- 修改文件：17 个源码/测试文件、4 份需求/架构文档、任务入口及本次测试索引；明细见下方。
- 附加文件与测试结果：五组证据已登记稳定链接；各阶段实际资源、日志、当前快照、复测入口与独立报告均保留。
- CI：NOT_RUN，用户未要求。
- 未确认事项：Socket 单次读取缓冲上限；其余分支已就绪。
- 完成总结：已实现部分本地验证与独立复审通过；整体未完成，剩余读取块上限待确认，不移动 completed。

## 第一批实现与复审进展

- 2026-09-06：S1 Android Profile 指纹门禁及冻结端点更新完成，两个漂移 RED 用例修复后通过；进一步锁定热更新只改 proxy_host、保留冻结 IP/端口/Profile/epoch，聚焦与 Clippy 通过。
- S2 规则保存覆盖物化到提交全过程，晚到结果按代次隔离；保留取消能力和旧保存失败提示。新增六项回归，规则模块 43 项通过。
- S3 EOF 复用现有 READ_FAILED/Read/Downstream，真实本机 TCP 证明只发一个下游读取失败终态；Socket external_* 关联字段补齐，Exchange 27 和 Socket 63 项聚焦通过。
- S4 诊断队列改为按需增长；usize::MAX 容量加 512 字节预算可构造并正确淘汰。读取块绝对上限尚未获得具体用户选择，不进行该项实现。
- 首次稳定全量：Rust 45 组 1645 项、前端 546 项通过；随后补强取消/错误提示与冻结路由测试，最终复测 Rust 1645、前端 548 通过，源码 1352 文件前后稳定，源码快照逐字节一致。类型、零警告 lint、严格 Clippy、格式、架构、500 行和前端构建通过。首轮 Clippy 测试分号问题已修正；原失败日志保留。
- 独立 architect 对所审实现给出 CLEAR，但任务闭环仍 BLOCK：读取块上限待确认、正式证据待入库。
- 独立 code-reviewer 又复现范围内 P2：新建草稿物化期间再次 New，组件同身份复用，旧物化仍可保存。该发现重新打开 S2，以显式编辑会话身份修复，补 New/同身份替换回归。不得把之前的局部通过写成最终 APPROVE。
- 外部错误字段的观测总预算候选仍在复审确认，确认前不当成缺陷或改变既有预算。
- 用户后续询问评分并要求其他任务继续，已说明约 80/100 属于工程判断、不是覆盖率；随后“继续”维持原修复任务，并未明确选择读取缓冲上限。
- 当前完整证据保留于本机临时任务目录，待复审修复后转入正式 TASK 用例目录；未提交、未发布、未触发 CI，未执行真机/TUN/长稳或外部业务验收。


## 复审问题闭环与最终已实现批次（2026-09-06 15:30:20 +08:00）

两项发现均属于原六项修复的边界补足，沿用原用户授权，不增加新产品合同。进入该增量实施前，最小复现、既有期望及根因已明确，先 RED 后 GREEN；读取块上限分支仍未进入实现。

| 复审发现 | 事实、根因和正确边界 | 方案与当前验收 |
| --- | --- | --- |
| 同身份草稿替换 | New 已打开时再次 New，以及抓包 A→B 同 rule_id:null/Listener/Stage，旧组件未卸载，旧物化完成借用父的新代次保存；有效 red-final 两条真实 RulesView 测试失败 | 单一 editorSession 的 state/ref 同时驱动 React key 和异步所有权；复用子组件卸载代次，无新兼容路径。45 项规则测试通过，独立额外 same-act 交错也未发保存 |
| 外部错误文本预算 | 原 guard 只计 Context 和 error.message；新增 external 字段可能使6MiB Context（12MiB hex）加5MiB remote文本仍发 failed，超过已有16MiB；RED两项失败 | 提取 failure.rs 保持文件职责和500行门禁；三条失败路径共用 checked-add UTF-8总字节计算，覆盖全部动态字段。恰好16MiB允许、多1字节发既有 observation_lost；不改业务错误、默认值或预算 |

候选原因已排除：没有通过禁用取消、吞掉后端错误、变更规则身份字段、放大预算或默默裁剪文本绕过问题。HTTP 原有相同预算遗漏在复用同一 guard 时一并消除；没有新增不同协议的双实现。

## 当前验收与未完成边界

| 原发现 | 当前结论 |
| --- | --- |
| Android 保存配置隐式 apply | 本地 PASS；目标应用和100%丢包保存漂移不发送控制应用；有效host变更保持冻结路由和epoch；真机/TUN NOT_RUN |
| Socket 资源耗尽 | 诊断队列按需分配 PASS；读取块绝对上限仍待确认，原P1资源分支未清零 |
| 规则重复保存 | PASS；物化到保存完成持有同一次所有权，失败释放 |
| 迟到保存覆盖 | PASS；新会话隔离保存/复制/删除返回，同身份替换取消旧物化，保留失败提示 |
| EOF 元数据 | PASS；核心和真实本机TCP都证明 READ_FAILED/Read/Downstream，只有一个下游读失败终态 |
| Socket 外部失败观测 | PASS；字段完整；三种失败投影共用16MiB预算，临界值与超限路径通过 |

最新稳定全量：Rust45组1647通过、0失败/忽略；前端73文件550通过；fmt、严格workspace Clippy、typecheck、零警告lint、架构/依赖/owned-task/socket/front-end边界、500行、Next构建、diff检查全部通过。1354源码/配置文件测试前后无变化；当前主快照1385文件逐字节一致。架构文档检查额外120项读取/链接/目录输入完整归档，解压到新目录单独重放也PASS。

独立 code-reviewer 复审：已实现子集 APPROVE，新增P0/P1/P2为0/0/0；architect：已实现子集 CLEAR。两位均确认整体仍BLOCK，不能以局部通过关闭任务。全部实施者完成且主 Agent 集成验证后才开始这两次复审；未并行审查未完成的共享实现。

## 已登记测试与附加文件

| 用例 ID | 结果 | 稳定证据 |
| --- | --- | --- |
| android-active-profile | LOCAL_PASS_DEVICE_NOT_RUN | [README](../../../testing/evidence/2026-09-06/TASK-20260906-001/android-active-profile/README.md) |
| rules-save-ownership | COMPONENT_PASS_NATIVE_UI_NOT_RUN | [README](../../../testing/evidence/2026-09-06/TASK-20260906-001/rules-save-ownership/README.md) |
| exchange-error-observation | LOCAL_UNIT_AND_LOOPBACK_TCP_PASS | [README](../../../testing/evidence/2026-09-06/TASK-20260906-001/exchange-error-observation/README.md) |
| socket-resource-limits | OBSERVER_PASS_READ_CHUNK_CAP_PENDING | [README](../../../testing/evidence/2026-09-06/TASK-20260906-001/socket-resource-limits/README.md) |
| integrated-verification | CHECKS_PASS_IMPLEMENTED_SUBSET_APPROVED_READ_CHUNK_CAP_PENDING | [README](../../../testing/evidence/2026-09-06/TASK-20260906-001/integrated-verification/README.md) |

整体验证资源含当前完整源码/配置/活动fixtures快照、各阶段实际测试资源、历史RED及设置错误的原始日志、执行JSON、环境、输入/输出、逐字段/字节比较、独立审查和可直接执行的replay入口。290个发布文件另附逐文件SHA/字节清单；该清单自身与后来追加的归档核验记录不自引用。Android使用fake runner，动态UUID/epoch只在实际测试中逐字段比较，不伪造控制请求或空帧证据。

## 修改文件与简化

- `src-tauri/crates/exchange/src/exchange.rs`
- `src-tauri/crates/exchange/src/observation.rs`
- `src-tauri/crates/exchange/src/observation/failure.rs`
- `src-tauri/crates/exchange/src/observation/tests.rs`
- `src-tauri/crates/exchange/src/tests/exchange.rs`
- `src-tauri/crates/infrastructure/src/adapters/android_adb/endpoint_reconciliation.rs`
- `src-tauri/crates/infrastructure/src/adapters/android_adb/reverse/preparation.rs`
- `src-tauri/crates/infrastructure/src/adapters/android_adb/tests/manual_recovery.rs`
- `src-tauri/crates/proxy/src/socket_relay/observer.rs`
- `src-tauri/crates/proxy/src/socket_relay/observer/tests.rs`
- `src-tauri/crates/proxy/src/socket_relay/tests/scripted_relay.rs`
- `src/features/rules/rule-creation-editor.tsx`
- `src/features/rules/rule-definition-editor.tsx`
- `src/features/rules/rule-single-pair-editor.tsx`
- `src/features/rules/rules-view-session-races.test.tsx`
- `src/features/rules/rules-view.test.tsx`
- `src/features/rules/rules-view.tsx`

简化：EOF复用既有类型化失败投影并删除重复helper；错误投影集中到failure模块；不再把诊断保留数量当作预分配请求；UI以一个显式会话身份同时管理组件替换与迟到返回。未添加依赖、重试、静默兜底、迁移或Schema修改。

## 文档影响与同步结果

| 文档范围 | 判断 | 已执行或无需更新依据 |
| --- | --- | --- |
| docs/requirements.md | 需要更新，已同步 | Android保存与应用、冻结host热更新合同 |
| docs/architecture/android-vpn-transparent-routing.md | 需要更新，已同步 | 活动身份、指纹和冻结端点 |
| docs/architecture/rules-and-protocol-packages.md | 需要更新，已同步 | 保存所有权、New/Cancel/capture会话、迟到返回 |
| docs/architecture/exchange-pipeline.md | 需要更新，已同步 | EOF强类型语义、external字段和共同观测预算 |
| docs/README.md、任务和测试索引 | 需要更新，已同步 | 保持唯一pending入口，登记五组证据；不写完成索引 |
| 根README、新人指南、用户操作说明 | 无需更新 | 无新操作/API/默认行为，现有公开入口与操作合同保持；架构检查通过 |
| ADR、其他架构、MCP、外部包、示例、脚本、内嵌资源 | 无需更新 | 无协议、Schema、MCP工具、包接口或部署方式变更；500行与边界门禁通过 |
| 旧测试矩阵、验证结果、历史任务 | 无需改写 | 历史证据保持不可变；新回归与本次结果由上述派生用例记录 |

技能记录：test-driven-development（/Users/codin/.agents/skills/test-driven-development/SKILL.md，无独立版本号）用于RED/GREEN；oh-my-codex code-review（插件0.20.2）用于分级独立复审；rust-router及并发/领域技能用于既有Rust边界判断。技能不替代本仓库登记、证据、锁、需求门禁和归档方式。未执行独立小任务提前审查，原因是按整批稳定实现统一集成复审；两轮已实现子集审查均在批次集成后进行。

## 当前停止条件和后续就绪项

所有不依赖读取块数值的实现、修复、测试、文档与证据登记已经处理；唯一实施阻塞是读取块上限合同。建议1MiB，另曾提供16MiB选项，超限按已有配置错误返回而非静默裁剪；这仍是建议，未视作用户确认。用户两次“继续”不替代具体数值。

收到明确上限后：领域单一常量，应用/领域验证共用，Exchange重导出同一常量供Proxy配置边界消费；先补max/max+1/u32::MAX无分配RED，再实现、集成并对整个任务最终独立审查。没有新依赖或重复数字来源。完成后方可持锁关闭并移动completed。

CI、提交、推送、安装/发布：NOT_RUN；当前工作区保留全部原有无关文档/任务改动。Android真机、跨平台构建、长稳、真实业务未验收；工程评分不是覆盖率或100%无缺陷证明。


## 入库后核验（2026-09-06 15:33:27 +08:00）

实时架构文档检查、git diff --check 和正式证据目录的replay校验再次通过。290份发布资源逐文件比对一致；五用例、五证据索引行、一个pending入口、零完成入口及父子链接一致。追加核验记录见 [归档一致性](../../../testing/evidence/2026-09-06/TASK-20260906-001/integrated-verification/outputs/archive-verification/consistency.json)。测试后的源码和合同文档未变化；只更新任务元数据，不重复未变化的全量业务测试。

## 主 Agent 当前评分

整体约85/100，是基于本轮源码、回归和审查的工程判断，非覆盖率或正确性证明。与修复前约80/100分开记录，不将未完成风险抹平。

| 维度 | 当前分数 | 主要依据与限制 |
| --- | --- | --- |
| 架构职责 | 88 | 既有依赖方向和协议边界保留，局部所有权修复有效 |
| 逻辑正确性 | 86 | 六项确认边界大部修复，真实回归与EOF字节证据支持 |
| 并发与生命周期 | 84 | 保存锁覆盖物化，编辑会话及冻结运行事实已验证；缺长稳 |
| 资源安全 | 76 | 队列和观测预算已修，读取块绝对上限P1仍开放 |
| 错误与诊断 | 90 | EOF与external字段完整，超预算可观测且不改业务错误 |
| 测试保障 | 86 | Rust1647/前端550及门禁通过；不等于真机、跨平台和业务验收 |
| 可维护性 | 85 | 复用已有类型/预算/工具，删除重复helper；无新增依赖 |

未单独给安全审计、跨平台和真机稳定性打分：本轮没有完成这些层级的全面验收。


## 本地提交授权变更（2026-09-06 15:52:56 +08:00）

- 原要求：先完成本地修复，用户随后表示暂不提交；读取块上限未确认。
- 新要求：用户明确“提交代码 后续在确认”，授权提交已经验证的代码，上限留待后续确认。
- 影响：仅交付边界从不提交改为本地Commit；不改变已有协议/数值/超限行为，读取块分支继续待确认，整体任务不关闭。
- 提交文件：17个源码/测试文件和4个当前需求/架构文档；沿用既有只提交已审查代码及同步文档的范围，任务/证据材料保留本地。无关任务、索引和其他脏文件不纳入。
- 就绪：PASS；已实现子集通过独立APPROVE/CLEAR、Rust1647/前端550与全部门禁，本次再次对1385个源码/合同快照做字节核对，未变更源码。没有新行为，已有验收项继续有效。
- 本地提交结果：待下方记录；不Push、不触发CI。


## 本地提交结果（2026-09-06 15:55:13 +08:00）

- Commit：`fa4e154f1861ce3ac032f5f87475b6fcd88c497e`；标题：`fix: guard runtime ownership and failure observations`。
- 文件：21个，与精确暂存清单及已验证快照逐字节相同；包含failure.rs和规则会话回归两个新文件。
- 提交前：暂存区为空；仅本次清单进入暂存，cached diff检查通过。提交后：暂存区为空，提交路径集合恰好21项，无源码遗留修改；原任务/证据和索引修改仍保留。
- 验证沿用稳定源码上的Rust1647/前端550与严格静态/构建门禁；本轮快照再核对及staged内容检查通过，未重复未变化源码的全量测试。
- 推送、CI、发布：NOT_RUN。读取块绝对上限后续确认，当前任务状态继续为待确认。
