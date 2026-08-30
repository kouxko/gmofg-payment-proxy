# 测试证据索引

本文按执行日期倒序记录可复用测试证据及其派生关系。原证据目录保持不可变；新需求通过父任务 ID、
父用例 ID 和父证据稳定路径建立关系。

## 2026-08-31

| 任务 ID | 用例 ID | 用途 | 状态 | 父任务 | 父用例 | 父证据 | 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TASK-20260829-002 | phase11-socket-shared-rpc-pipeline | 验证Socket统一shared `/packages` Frame/Decode/Display/Encode、consumedBytes gate、unchanged原字节、changed Encode、joint actor lifecycle rollback/commit、typed failure与Socket/HTTP capability隔离；review repair统一production装配及HTTP+Socket RuleRepository projection/commit/reset，并将Socket NthHit投影给actor、Document留在joint gate；真实Relay按ProxyToUpstream→ProxyToApp观察前序修改，Nth miss提交advance、Encode失败不消费counter、两方向各单次提交；checker22/22、Domain87/87、external runtime5/5与静态门PASS；最终Reviewer/Verifier P0/P1/P2=0；Infrastructure full 600/602的相关stale断言已修，剩余Android deadline、唯一checkpoint session24690的既有non-loopback环境阻塞与人工NOT_RUN继续保留 | VERIFIED / APPROVED / CHECKPOINT READY | TASK-20260829-002 | phase10-http-shared-rpc-pipeline | [父证据](2026-08-31/TASK-20260829-002/phase10-http-shared-rpc-pipeline/README.md) | [证据](2026-08-31/TASK-20260829-002/phase11-socket-shared-rpc-pipeline/README.md) |
| TASK-20260829-002 | phase10-http-shared-rpc-pipeline | 验证HTTP Body统一shared `/packages` RPC、strict UTF-8/Shift-JIS与Content-Encoding gate、authoritative wire bytes、unchanged 0 Encode RPC、changed Encode、typed remote failure贯穿Exchange/Proxy/capture、stable top-level external code、Display fail-open typed observation、production prepare_async双向capability与joint rollback及legacy仅cfg(test)；checker19/19、focused6/6、affected Application460/460/Exchange24/24/Runtime227/227/Infrastructure643/643及静态门PASS；最终Reviewer/Verifier P0/P1/P2=0，唯一checkpoint session3469的既有non-loopback环境阻塞与人工NOT_RUN保留 | VERIFIED / APPROVED / CHECKPOINT READY | TASK-20260829-002 | phase9-local-sidecar-lifecycle | [父证据](2026-08-30/TASK-20260829-002/phase9-local-sidecar-lifecycle/README.md) | [证据](2026-08-31/TASK-20260829-002/phase10-http-shared-rpc-pipeline/README.md) |

## 2026-08-30

| 任务 ID | 用例 ID | 用途 | 状态 | 父任务 | 父用例 | 父证据 | 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TASK-20260829-002 | phase9-local-sidecar-lifecycle | 验证真实本地 Boa Sidecar process、package 主动统一 `/packages` 注册、strict ZIP importer不执行Boa、local-vs-remote enable/manual restart、disabled local restart fail-closed且不调用port、exact串行process ownership、Supervisor唯一错误owner、10秒注册、无retry/replay、disconnect/delete/shutdown kill+reap、duplicate no-takeover 与 listener enabled+online gate；checker15/15、runtime20/20、supervisor4/4、Application lifecycle8/8、IPC2/2、Infrastructure591/591、Application460/460、前端84/84及静态门PASS；保留既有non-loopback MCP环境超时与真实bundle/permissions NOT_RUN | VERIFIED / APPROVED / CHECKPOINT READY | TASK-20260829-002 | phase8-boa-sidecar-runtime | [父证据](2026-08-30/TASK-20260829-002/phase8-boa-sidecar-runtime/README.md) | [证据](2026-08-30/TASK-20260829-002/phase9-local-sidecar-lifecycle/README.md) |
| TASK-20260829-002 | phase8-boa-sidecar-runtime | 验证单 Boa Context 串行、Boa default features、静态/nested/cyclic package-relative ESM、dynamic import Promise settle、固定八 export 预检缓存、HTTP string、Socket Uint8Array/canonical Base64及 compile-only generic sidecar marker；包含 baseline、ordinary Array、dynamic Promise、Host-binding checker 四组 RED、checker21/21、Cargo12/12、affected/static 与唯一十门 exit0 | VERIFIED / APPROVED / CHECKPOINT READY | TASK-20260829-002 | phase7-package-runtime | [父证据](2026-08-30/TASK-20260829-002/phase7-package-runtime/README.md) | [证据](2026-08-30/TASK-20260829-002/phase8-boa-sidecar-runtime/README.md) |
| TASK-20260829-002 | phase7-package-runtime | 验证严格根 ZIP/shared Manifest active importer、actual ZIP byte accounting、package 主动无 id 注册、固定 typed methods、stable Domain code 贯穿真实 Socket diagnostic、canonical Base64/FrameResult、production WebSocket wire ceiling、取消/顺序 RPC/raw-vs-wire 边界及旧动态策略删除；最终 cross-phase gate 保证 Phase4 legacy allowlist 永久为空、generated SHA 与 fresh bindings 一致且 Phase7 聚合真实执行 Phase4 checker；包含 Phase6 干净基线真实 RED、全部 review findings/repairs、affected full 与完整十门 PASS | VERIFIED / APPROVED / CHECKPOINT READY | TASK-20260829-002 | phase6-rule-chain-transaction | [父证据](2026-08-30/TASK-20260829-002/phase6-rule-chain-transaction/README.md) | [证据](2026-08-30/TASK-20260829-002/phase7-package-runtime/README.md) |
| TASK-20260829-002 | phase6-rule-chain-transaction | 验证 terminal-scoped Nth lifecycle、save/runtime stats 分离、validated Application 唯一私有事务、完整 AppError、working HTTP+Document 可见性、pending terminal、共享 delta 校验、actor 全失败 checkpoint 回滚、冲突不重试/不消费、caller abort actor 所有权、generated/TS parity 与完整十门；保留初版 Reviewer/Verifier 和第二轮 Reviewer findings、repair 及最终零 finding 复验 | VERIFIED / APPROVED / CHECKPOINT READY | TASK-20260829-002 | phase5-unified-rule-domain | [父证据](2026-08-30/TASK-20260829-002/phase5-unified-rule-domain/README.md) | [证据](2026-08-30/TASK-20260829-002/phase6-rule-chain-transaction/README.md) |
| TASK-20260829-002 | phase5-unified-rule-domain | 验证递归非空 AND/OR 条件树、严格类型谓词、统一有序动作、Document 严格修改、terminal 停止、priority+rule_id 排序、working-state 可见、独立复制、两阶段新保存与 generated/TS parity；保留初始 review findings、修复 RED/GREEN、焦点 flake 与最终十门复跑 | VERIFIED / APPROVED / CHECKPOINT READY | TASK-20260829-002 | phase4-package-contract | [父证据](2026-08-30/TASK-20260829-002/phase4-package-contract/README.md) | [证据](2026-08-30/TASK-20260829-002/phase5-unified-rule-domain/README.md) |
| TASK-20260829-002 | phase4-package-contract | 验证唯一 API1 Manifest/RPC/FrameResult/stable-code crate、Rust/generated/TS/MCP parity、Cargo 实际测试发现、精确 Phase7 allowlist 与七组证据 SHA/字节一致；保留历史 132/133 ALF timeout 与短名 0-test 非证据，记录 firewall permitted 后精确 1/1 和最终十门 PASS | VERIFIED / APPROVED / CHECKPOINT READY | TASK-20260829-002 | phase3-recursive-document-contract | [父证据](2026-08-30/TASK-20260829-002/phase3-recursive-document-contract/README.md) | [证据](2026-08-30/TASK-20260829-002/phase4-package-contract/README.md) |
| TASK-20260829-002 | phase3-recursive-document-contract | 验证无 identity recursive Document/Schema、RFC6901、即时 Rust/generated/前端消费者、旧合同零残留及 Nuvei 派生 fixture；保留初版 Verifier FAILED、最终 scalar-text P2、完整修复与独立 fresh 十门 PASS | VERIFIED / APPROVED / CHECKPOINT READY | TASK-20260829-002 | phase2-development-database-recreate | [父证据](2026-08-30/TASK-20260829-002/phase2-development-database-recreate/README.md) | [证据](2026-08-30/TASK-20260829-002/phase3-recursive-document-contract/README.md) |
| TASK-20260829-002 | phase2-development-database-recreate | 验证显式开发期 Schema100 重建、Host 默认与 Release Preserve、WAL/回滚/FK/双启动合同及 package/Tauri 双层发布阻断门禁；保留 Verifier FAILED、build 绕过 P1、修复复跑与最终 delta APPROVE | APPROVED_WITH_RELEASE_BLOCKER | TASK-20260829-002 | phase1-green-contract-baseline | [父证据](2026-08-29/TASK-20260829-002/phase1-green-contract-baseline/README.md) | [证据](2026-08-30/TASK-20260829-002/phase2-development-database-recreate/README.md) |

## 2026-08-29

| 任务 ID | 用例 ID | 用途 | 状态 | 父任务 | 父用例 | 父证据 | 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TASK-20260829-002 | phase1-green-contract-baseline | 验证 Phase 1 当前合同 inventory、bindings freshness/determinism、完整十门禁及独立复验；保留首次 ADB deadline 偶发失败和成功复跑记录 | PASS | 无 | 无 | 无 | [证据](2026-08-29/TASK-20260829-002/phase1-green-contract-baseline/README.md) |
| TASK-20260829-001 | DB-SCHEMA-100-001 | 验证版本 100 正式兼容基线、pre-1.00 原子重建、并发/FK/WAL/异常 marker 边界及真实 App 启动 | PASS | TASK-20260828-005 | UNIFIED-RULE-CONTRACT-001 | [父证据](2026-08-28/TASK-20260828-005/UNIFIED-RULE-CONTRACT-001/README.md) | [证据](2026-08-29/TASK-20260829-001/DB-SCHEMA-100-001/README.md) |

## 2026-08-28

| 任务 ID | 用例 ID | 用途 | 状态 | 父任务 | 父用例 | 父证据 | 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TASK-20260828-006 | WORKSPACE-LIVE-REFRESH-001 | 验证外部 Workspace 提交后列表与详情刷新、名称草稿合并、刷新失败陈旧标记及重启后真实 App 状态 | PASS_WITH_NOT_RUN | TASK-20260828-003 | RUNNING-APP-FINDINGS-REGRESSION-001 | [父证据](2026-08-28/TASK-20260828-003/RUNNING-APP-FINDINGS-REGRESSION-001/README.md) | [证据](2026-08-28/TASK-20260828-006/WORKSPACE-LIVE-REFRESH-001/README.md) |
| TASK-20260828-005 | UNIFIED-RULE-CONTRACT-001 | 验证单一 RuleDefinition、统一阶段、单一持久化集合、联合 HTTP 运行时和 Socket 能力隔离 | PASS | 无 | 无 | 无 | [证据](2026-08-28/TASK-20260828-005/UNIFIED-RULE-CONTRACT-001/README.md) |
| TASK-20260828-004 | ANDROID-CONTROL-LEASE-001 | 验证默认开启的逐设备控制租约、5 秒 generation 看门狗、取消清理和多设备隔离 | PASS_WITH_NOT_RUN | 无 | 无 | 无 | [证据](2026-08-28/TASK-20260828-004/ANDROID-CONTROL-LEASE-001/README.md) |
| TASK-20260828-003 | RUNNING-APP-FINDINGS-REGRESSION-001 | 验证 Mock 托管 Header 修复、Environment commit 事件与 Workspace 统一刷新链路 | PASS_WITH_NOT_RUN | TASK-20260828-001 | RUNNING-APP-REPLAY-001 | [父证据](2026-08-28/TASK-20260828-001/RUNNING-APP-REPLAY-001/README.md) | [证据](2026-08-28/TASK-20260828-003/RUNNING-APP-FINDINGS-REGRESSION-001/README.md) |
| TASK-20260828-002 | SOCKET-CONNECTION-STATUS-001 | 验证 Socket Exchange 只表达连接生命周期并保留异常错误 | PASS | 无 | 无 | 无 | [证据](2026-08-28/TASK-20260828-002/SOCKET-CONNECTION-STATUS-001/README.md) |
| TASK-20260828-001 | RUNNING-APP-REPLAY-001 | 在当前运行 Release App 中用本机 HTTP/TCP 模拟 Server 重放真实 Proxy、抓包、Exchange、日志、规则能力和恢复 | FAILED_WITH_NOT_RUN | TASK-20260827-003 | FINAL-REPLAY-001 | [父证据](2026-08-27/TASK-20260827-003/FINAL-REPLAY-001/README.md) | [证据](2026-08-28/TASK-20260828-001/RUNNING-APP-REPLAY-001/README.md) |

## 2026-08-27

| 任务 ID | 用例 ID | 用途 | 状态 | 父任务 | 父用例 | 父证据 | 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TASK-20260825-005 | TLS-CA-BUNDLE-FINAL-001 | 验证多 CA Bundle 解析、规范化、受保护持久化、恢复、缺失 Intermediate 的 Socket TLS 与真实后台握手 | PASS | TASK-20260825-005 | TLS-CA-BUNDLE-001 | [父证据](2026-08-25/TASK-20260825-005/TLS-CA-BUNDLE-001/README.md) | [证据](2026-08-27/TASK-20260825-005/TLS-CA-BUNDLE-FINAL-001/README.md) |
| TASK-20260827-003 | FINAL-REPLAY-001 | 重跑归档场景、本地完整门禁、隔离 App 的 MCP 指南与环境候选 create/apply/status、退出重启和端口释放 | PASS_WITH_NOT_RUN | 多个历史任务 | 见场景清单 | [场景清单](../final-replay-scenario-inventory-20260827.md) | [证据](2026-08-27/TASK-20260827-003/FINAL-REPLAY-001/README.md) |
| TASK-20260825-006 | MCP-CONFIG-APP-001 | 验证打包 App 完整资源候选预览、原子提交、当前 Workspace 持久化 envelope 与退出重启恢复 | PASS | TASK-20260825-006 | MCP-CONFIG-CONTRACT-001 | [父证据](2026-08-26/TASK-20260825-006/MCP-CONFIG-CONTRACT-001/README.md) | [证据](2026-08-27/TASK-20260825-006/MCP-CONFIG-APP-001/README.md) |

## 2026-08-26

| 任务 ID | 用例 ID | 用途 | 状态 | 父任务 | 父用例 | 父证据 | 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TASK-20260826-004 | NTR-RHAI-002 | 在真实 Proxy 导入/启用 Rhai 包并完成三组 Nuvei Listener 双向 Exchange | NOT_RUN | TASK-20260826-003 | NUVEI-PKG-003 | [父证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-003/README.md) | [证据](2026-08-26/TASK-20260826-004/NTR-RHAI-002/README.md) |
| TASK-20260826-004 | NTR-RHAI-001 | 验证 Nuvei Tango Rhai 包的 Python parity、原文 Display、只读失败路径、已知字节数和确定性 ZIP | PASS | TASK-20260826-003 | NUVEI-PKG-003 | [父证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-003/README.md) | [证据](2026-08-26/TASK-20260826-004/NTR-RHAI-001/README.md) |
| TASK-20260826-003 | NUVEI-PKG-003 | 验证 external Document int wire 修复及真实上下行 split/decode/display/encode 逐字节保持 | PASS | TASK-20260826-003 | NUVEI-PKG-002 | [父证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-002/README.md) | [证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-003/README.md) |
| TASK-20260826-003 | NUVEI-PKG-002 | 验证 Python 外部包连接与 RPC 结构化诊断日志不泄露报文或字段内容 | PASS | TASK-20260826-003 | NUVEI-PKG-001 | [父证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-001/README.md) | [证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-002/README.md) |
| TASK-20260826-003 | NUVEI-PKG-001 | 验证 Nuvei Tango 长度前缀 JSON 的只读 Python 外部包、掩码和逐字节保持 | PASS | 无 | 无 | 无 | [证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-001/README.md) |
| TASK-20260826-002 | DOC-GOV-006 | 验证快速配置验证分流、生命周期、分层结论、清理门禁和正式任务升级合同 | PASS | 无 | 无 | 无 | [证据](2026-08-26/TASK-20260826-002/DOC-GOV-006/README.md) |
| TASK-20260826-001 | DOC-GOV-005 | 验证需求就绪、根因分析、高低优先级、风险分级测试和锁目录规则 | PASS | 无 | 无 | 无 | [证据](2026-08-26/TASK-20260826-001/DOC-GOV-005/README.md) |
| TASK-20260825-006 | MCP-CONFIG-CONTRACT-001 | 验证环境配置 v1 DTO、严格 Schema、公共 literal、fixture、终态合同和 active MCP 隔离 | PASS | 无 | 无 | 无 | [证据](2026-08-26/TASK-20260825-006/MCP-CONFIG-CONTRACT-001/README.md) |

## 2026-08-25

| 任务 ID | 用例 ID | 用途 | 状态 | 父任务 | 父用例 | 父证据 | 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TASK-20260825-007 | DOC-GOV-004 | 验证单工作区全局任务锁、所有权恢复链和任务索引一致性 | PASS | 无 | 无 | 无 | [证据](2026-08-25/TASK-20260825-007/DOC-GOV-004/README.md) |
| TASK-20260825-005 | TLS-CA-BUNDLE-001 | 准备上游多 CA PEM Bundle 实测资源和复测步骤 | PREPARED | 无 | 无 | 无 | [证据](2026-08-25/TASK-20260825-005/TLS-CA-BUNDLE-001/README.md) |
| TASK-20260825-004 | FINAL-ARCHITECTURE-VALIDATION | G023-G031 稳定树三连跑、完整本地质量门禁和最终独立审查输入 | PASS | 无 | 无 | 无 | [证据](2026-08-25/TASK-20260825-004/FINAL-ARCHITECTURE-VALIDATION/README.md) |
| TASK-20260825-004 | G030-EXTERNAL-PACKAGE-FAULT-ISOLATION | 验证受信任外部协议包的超时、畸形输入、断连、额度和跨包故障隔离 | PASS | 无 | 无 | 无 | [证据](2026-08-25/TASK-20260825-004/G030-EXTERNAL-PACKAGE-FAULT-ISOLATION/README.md) |
| TASK-20260825-004 | G029-OBSERVABILITY | 验证观测责任、容量、保留、关联字段和完整 payload 合同 | PASS | 无 | 无 | 无 | [证据](2026-08-25/TASK-20260825-004/G029-OBSERVABILITY/README.md) |
| TASK-20260825-003 | DOC-GOV-003 | 验证测试资源归档和跨任务复用规范 | PASS | TASK-20260825-001 | DOC-GOV-001 | [父证据](2026-08-25/TASK-20260825-001/DOC-GOV-001/README.md) | [证据](2026-08-25/TASK-20260825-003/DOC-GOV-003/README.md) |
| TASK-20260825-002 | DOC-GOV-002 | 验证小任务对抗审查改为可选、整体审查保持强制 | PASS | TASK-20260825-001 | DOC-GOV-001 | [父证据](2026-08-25/TASK-20260825-001/DOC-GOV-001/README.md) | [证据](2026-08-25/TASK-20260825-002/DOC-GOV-002/README.md) |
| TASK-20260825-001 | DOC-GOV-001 | 验证项目 AGENTS 治理规范 | PASS | 无 | 无 | 无 | [证据](2026-08-25/TASK-20260825-001/DOC-GOV-001/README.md) |
