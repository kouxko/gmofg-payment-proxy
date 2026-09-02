# 完成架构优秀化优先任务

## 任务信息

- 任务 ID：TASK-20260825-004
- 状态：已完成
- 任务日期：2026-08-25
- 创建时间：2026-08-25 16:56:09 +08:00
- 开始时间：2026-08-25 12:31:00 +08:00
- 最后更新时间：2026-08-25 23:30:42 +08:00
- 完成时间：2026-08-25 23:30:42 +08:00
- 创建路径：`docs/tasks/pending/2026-08-25/architecture-excellence-delivery.md`
- 归档路径：`docs/tasks/completed/2026-08-25/architecture-excellence-delivery.md`
- 关联提交：未提交
- 关键词：整洁架构、Rust 业务合同、Listener CIDR、SQLite executor、聚合快照、Infrastructure、确定性测试、可观测性、外部协议包

## 背景

用户完成了项目架构审查并批准按优先级执行
`.omx/plans/architecture-excellence-prioritized-review.md`。执行开始后，仓库于同日新增任务治理规范；
因此本档案在治理规范生效后立即补登记，覆盖本轮 G023-G031，保留此前验证记录，
并从 G029 起严格按本档案维护测试证据与归档。

## 目标

- 让 Rust 成为协议规则编辑业务合同的唯一事实源。
- 完整删除 Listener 客户端 CIDR 功能，默认允许所有来源 IP；保留 Android 目的路由 CIDR。
- 建立共享、串行、可测试的 SQLite 阻塞执行边界，并提供点查询和 Application 聚合快照。
- 收窄 Infrastructure 对 Host 的公开能力和具体 adapter 耦合。
- 建立可防绕过的架构门禁和确定性并发测试。
- 整合观测责任、容量、保留和关联字段；完整 payload 明确允许。
- 验证受信任外部协议包的故障隔离，不增加认证系统。

## 范围

- G023-G031 对应的 Domain、Application、Exchange、Proxy、Infrastructure、Host、Tauri、React、脚本、测试、生成绑定和相关架构文档。
- 本轮所有验证证据、整体对抗审查、文档同步和任务归档。

## 不在范围

- 外部协议包 token、HMAC、mTLS、Origin、注册身份或授权。
- 隐私、脱敏、PAN/PII 过滤或完整 payload 禁止。
- SQLite 连接池、新数据库依赖或正式 Release 1.0 schema 冻结。
- crate 大拆分、无证据的大规模重写。
- Push、远程 CI、发布或部署。

## 需求确认记录

- 外部协议包全部受信任，不增加身份认证。
- 测试、日志和报告允许保存完整 payload，不考虑隐私过滤。
- `allowed_client_cidrs` 必须完整删除，不保留可选开关；Listener 默认允许所有外部 IP。
- Android `destination_targets` 和 proxy-route CIDR 必须保留。
- SQLite 保持预发布不兼容 schema 重置；仅在用户正式宣布 Release 1.0 时冻结基线。
- Rust 拥有业务合同，React 只渲染能力并提交意图。
- 保留现有分层，不新增依赖，不做 broad rewrite。

## 未确认事项

- 无。当前实现边界已经由用户逐项确认。

## 最小改动与最优设计

| 方案 | 结论 |
| --- | --- |
| 只修 UI/局部条件 | 会保留前端业务矩阵、同步 SQLite 和宽 bundle，形成长期双事实源，不采用 |
| 在现有 crate 边界内修复权威合同、执行器、快照和接口隔离 | 保持行为和依赖图，同时消除根因，采用 |
| 拆 crate、换数据库或引入连接池 | 当前证据不足且范围过大，不采用 |

## 小任务列表

| ID | 小任务 | 依赖 | 可并行 | 负责人 | 状态 | 验收标准 | Commit |
| --- | --- | --- | --- | --- | --- | --- | --- |
| G023 | Rust-owned protocol-rule editor contract | 无 | 否 | executor + test review | 已完成 | Rust context/draft 权威，React 无 fallback，跨层门禁通过 | 未提交 |
| G024 | 删除 Listener client CIDR | G023 | 否 | executor + boundary review | 已完成 | 相关字段/UI/准入/错误归零，Android CIDR 保留 | 未提交 |
| G025 | SQLite blocking executor | G024 | 否 | executor + architect | 已完成 | 共享 executor、async DB 段迁移、原子性与取消测试 | 未提交 |
| G026 | 点查询与 Application aggregate snapshot | G025 | 否 | executor + architect | 已完成 | PK 点查、MCP/backup 无 N+1/双读 | 未提交 |
| G027 | Infrastructure consumer narrowing | G026 | 否 | executor + architect | 已完成 | private adapters/bundle fields，Host 无具体 adapter 耦合 | 未提交 |
| G028 | 确定性测试和架构门禁 | G027 | 否 | executor + test engineer | 已完成 | 四类负向 fixture、关键并发无 sleep/yield 同步替代 | 未提交 |
| G029 | Observability contract consolidation | G028 | 否 | executor + architect | 已完成 | 责任表、容量/保留、关联字段和 deterministic overflow | 未提交 |
| G030 | Trusted external-package fault isolation | G029 | 否 | executor + verifier | 已完成 | malformed/timeout/disconnect/independence/identity/limits 证据 | 未提交 |
| G031 | Runtime ownership, async boundaries and source cleanup | G030 | 否 | executor + independent reviewers | 已完成 | Listener/Rule/TLS/SQLite/Android 生命周期明确，source-size 全绿 | 未提交 |
| FINAL | 整体清理、验证、证据、对抗审查和归档 | G030 | 否 | main + independent reviewers | 已完成 | 全部门禁通过，整体 APPROVE，任务档案与证据完整 | 未提交 |

## 文档影响分析

| 文档 | 当前判断 |
| --- | --- |
| `README.md` | 无需更新；本轮功能合同由 requirements/onboarding/architecture/MCP 文档承载 |
| `docs/README.md` | 已登记，完成时移除 |
| `docs/requirements.md` | 已随 Listener CIDR 更新，待最终复核 |
| `docs/onboarding-guide.md` | 已同步 Listener CIDR 与 G030 trusted-package/full-payload 合同，待最终复核 |
| `docs/architecture/*.md` | 已更新 Listener CIDR、数据流、模块职责、运行时观测和相关 ADR；最终链接复核通过 |
| `docs/mcp/*.md` | G026/G029 已同步；G030 external-package guide 已同步故障隔离、额度与错误合同 |
| Android 与 external-package 文档 | G024 Android 文档已同步；G030 两个外部包示例 README 已同步 |
| `docs/testing/release-validation-matrix.md` | 已允许按测试需要记录完整报文、payload 与 Document |

## 测试计划

- Rust：受影响 crate 定向测试、workspace 标准测试、严格 Clippy、Fmt。
- Frontend：定向 Vitest、完整 Vitest、TypeScript、ESLint、generated bindings。
- 架构：frontend、architecture、runtime、socket、source-size 扫描和负向 fixtures。
- 网络/协议：standalone socket relay gate；G030 外部包故障隔离矩阵。
- G029/G030 及最终验收证据保存到
  `docs/testing/evidence/2026-08-25/TASK-20260825-004/`，每项包含 README、metadata 和实际使用的资源/输出。
- 执行远程 CI：否；用户未授权。

## 对抗审查计划

- G023-G028 已执行独立专项审查并解决 blocker。
- G029、G030 分别执行专项架构/验证审查。
- 所有小任务完成后，由未参与实现的 code reviewer 与 architect 做整体对抗审查；最终必须 `APPROVE`。

## Skill 使用记录

- `oh-my-codex:ultragoal`：维护持久化分步执行和 checkpoint。
- `oh-my-codex:team`：当前环境无 tmux，按 Skill 退化为 Codex native subagents。
- `testing-tauri-apps`：G028 采用 mock IPC、Tauri command/binding 和分层测试合同。
- `test-driven-development`：行为修改先建立失败夹具/回归测试，再实现并全量验证。

## 实施记录

### 2026-08-25 16:56:09 +08:00

- 新治理规范在本轮执行中途加入；主 Agent 分配 `TASK-20260825-004` 并补登记 G023-G031。
- G023-G028 已完成且分别获得独立审查 `APPROVE`；详细证据同时记录在 `.omx/ultragoal/ledger.jsonl`。
- G029 正在进行只读盘点；任务登记完成后继续实现。
- CI：未执行；未 Push。

### 2026-08-25 17:08:36 +08:00 — G029

- 按 `test-driven-development` 先增加运行日志队列计数读取测试；RED 以六个 `E0609` 证明
  producer 的 full/disconnected/contended 计数未进入 `ApplicationLogPage`。
- 将三个进程累计丢弃计数投影到 runtime log query、MCP JSON 和 reproduction Markdown；未新增
  store、依赖、认证、隐私过滤或 payload 摘要路径。
- 增加精确 `N=3/N+1=4` Store 淘汰测试、`B=128/B+1` 队列字节拒绝测试和业务结果不变断言；
  复用既有 Exchange store 淘汰、回滚、独立 loss-control lane 测试。
- `runtime-observability.md` 增加责任表、生产容量、保留、计数和关联字段合同；明确完整 HTTP Body、
  Socket bytes、Document 与复现证据允许进入专用有界通道，diagnostics 不复制 payload 是职责隔离。
- 专项对抗审查：由主 Agent 另行安排独立 architect；本 executor 未自行审查实现。
- 证据：[G029-OBSERVABILITY](../../../testing/evidence/2026-08-25/TASK-20260825-004/G029-OBSERVABILITY/README.md)。
- CI：未执行；未 Push；未提交。

### 2026-08-25 17:29:40 +08:00 — G029 reviewer fixes

- 关闭 architect WATCH：诊断页新增 `oldest_retained_event_id` 与 `snapshot_required`，直接复用
  EventHub 全局有界历史并按 `after_event_id` 缺口判定，不新增 retention owner。
- EventHub 已有共享 `CapacityLedger` 字节预算；增加固定 `B=569/B+1=570` 及 `N=3/N+1=4`
  边界测试。另保留 runtime/Exchange queue `B=128/B+1` 丢弃且业务结果仍为
  `Ok("business-completed")` 的测试。
- Exchange page 将 producer `dropped_events` 与 consumer/store `ignored_events` 分开投影，
  同步 React 展示、Rust 生成绑定、MCP 和文档；不再用一个字段混合两个责任边界。
- 单一 MCP smoke 覆盖五个关键输出的字段合同：application log、diagnostics、Exchange
  observation、HTTP capture、reproduction report。
- ADR-005 已同步三个 `queue_dropped_*` 字段，明确与 Store `evicted_count` 分离。
- reviewer fixes 定向测试、UI、TypeScript、架构 gates 和 strict Rust gates 均通过；等待主 Agent
  安排独立 architect 复审，executor 不自行给出 `APPROVE`。

### 2026-08-25 17:40:04 +08:00 — G030 implementation

- 先盘点 115 个既有 external-package 定向测试，确认注册 30 秒 timeout、RPC timeout、disconnect pending
  cleanup、heartbeat、duplicate exact identity、stale generation、in-flight、wire/display 大小和 Listener 精确停止
  已有覆盖；生产协议与身份策略保持不变。
- 补充两个不同精确包的隔离测试：一个包 malformed JSON 或 stalled RPC 时，另一个包仍可立即完成 RPC，且各自
  `max_in_flight` 不共享。
- 补充 raw malformed WebSocket frame 隔离测试，以及真实 ExternalPackageServer + ListenerRuntime 路径的非法
  `consumed_bytes > buffer.len()` 测试；后者只关闭当前业务连接，包保持 online、Listener 保持运行并继续处理下一连接。
- 修复两个实测 gap：malformed WebSocket transport 不再与正常 Close/EOF 混为 `Disconnected`，改为稳定
  `EXTERNAL_PACKAGE_TRANSPORT_ERROR`；注册 30 秒 deadline 现在从初始 request 写出前开始，并覆盖阻塞的初始写、
  heartbeat flush/send 和响应等待。
- 增加 registry + exact-listener cleanup + actor 组合测试：包 A 离线只停止引用 A 的 Listener，包 B 的 Listener
  不受影响且包 B RPC 同时继续完成。
- 连接接纳上限仍为 256；额度满时 fail-fast，并新增稳定诊断码
  `EXTERNAL_PACKAGE_CONNECTION_LIMIT_REACHED`。测试证明拒绝可查询、失败握手和正常任务退出都会释放 permit。
- 文档明确：所有可达且 wire 正确的外部包均受信任，不增加认证/Origin/loopback/CIDR 门禁；完整 payload 允许进入
  专用有界日志与证据，diagnostics 不复制 payload 仅是责任分离。
- 按 source-size gate 拆出两个 fault-isolation 测试模块；G030 最大拥有文件为 483 行。全仓 gate 仍由四个非 G030
  累积文件阻塞（608/574/553/524 行），本分项不越界重构。
- fresh 验证：Infrastructure 124、Domain 37、Application 7、Host 1、Tauri MCP 23 全部通过；Fmt、Infrastructure
  strict Clippy、architecture/runtime/socket/frontend gates 与 `git diff --check` 通过。
- 专项对抗审查：独立 verifier 于 2026-08-25 18:13:25 +08:00 给出最终 `APPROVE`；详见下方审查记录。
- 证据：[G030-EXTERNAL-PACKAGE-FAULT-ISOLATION](../../../testing/evidence/2026-08-25/TASK-20260825-004/G030-EXTERNAL-PACKAGE-FAULT-ISOLATION/README.md)。
- CI：未执行；未 Push；未提交。

### 2026-08-25 18:13:25 +08:00 — G030 independent verification

- 独立 verifier 完成 G030 最终审查，结论：`APPROVE`。
- 审查确认 malformed transport、注册全阶段 timeout、跨包独立进展、精确 Listener 清理、连接额度和非法
  frame boundary 均有实现与确定性回归证据；trusted unauthenticated package 与完整 payload 合同未被收窄。
- 审查证据沿用
  [G030-EXTERNAL-PACKAGE-FAULT-ISOLATION](../../../testing/evidence/2026-08-25/TASK-20260825-004/G030-EXTERNAL-PACKAGE-FAULT-ISOLATION/README.md)；
  metadata 的 `PASS_WITH_UNRELATED_REPOSITORY_GATE_FAILURE` 仅指四个 source-size FINAL 清理项，不是 G030 失败。
- G030 状态转为已完成；总体任务不归档，进入 FINAL 整体清理、验证、对抗审查与归档阶段。

### 2026-08-25 23:03:10 +08:00 — G031 implementation and FINAL evidence repair

- Listener start/stop 收口为一个显式取消所有者，并用 run-token/epoch CAS 阻止旧任务发布或清除新运行时；
  Proxy/Infrastructure 的 task-scope、shutdown、disconnect 和 exact-listener cleanup 均有确定性测试。
- SQLite 阻塞访问统一进入共享串行 executor；Application 聚合快照使用点查询，Host/Android bootstrap 与状态
  转换有明确所有者，不在 async runtime 上直接执行数据库 I/O。
- Rule runtime 改为 async actor，先完成持久化 CAS 再执行动作；TLS/rule blocking bridge 有界且可取消；Document
  rule compiler 进入有界 CPU 边界并使用 generation CAS，避免旧编译结果覆盖新规则。
- 证书材料和动态 SNI 采用原子 CA/fallback snapshot，移除 panic 转换；external registry/provider 改为 async，
  保留 exact package identity、错误传播和 trusted unauthenticated 合同。
- 按责任拆分 certificate material、listener certificate resolution、Host architecture support、Socket rules test
  runtime、external relay contract tests、rule actor 和 body-codec epoch cleanup；所有手写源码不超过 500 行，
  未增加依赖、fallback、兼容双路径或业务抽象。
- 完整 `pnpm check`、Rust workspace all-target/all-feature、standalone socket relay、architecture/runtime/socket/
  frontend/source-size gates 与 `git diff --check` 全部通过；Vitest 66 files/648 tests 与 Rust workspace 各连续
  三次 PASS，六次执行前后共同 status/diff 指纹保持一致。
- 最终证据：
  [FINAL-ARCHITECTURE-VALIDATION](../../../testing/evidence/2026-08-25/TASK-20260825-004/FINAL-ARCHITECTURE-VALIDATION/README.md)。
- 整体独立 architect 给出 `APPROVE` / `CLEAR`；code reviewer 的三个 P2 档案发现已全部修复，增量复审最终
  `APPROVE`，剩余 P0/P1/P2 为零。最终 quality gate、完整 task file/status/diff、三连跑和复测入口均已归档。
- CI：未执行；未 Push；未 stage；未提交。

## 修改文件

- G029 runtime contract：`src-tauri/src/runtime_logs/{model.rs,runtime_log_counters.rs,store.rs,tracing_bridge.rs,tests.rs}`、
  `src-tauri/src/runtime_logs/exchange_ui_layer/tests/queue.rs`。
- G029 MCP/report projection：`src-tauri/src/reproduction_report.rs`、`src-tauri/src/commands/e2e_tests/mod.rs`。
- G029 responsibility wording：`src-tauri/crates/application/src/models/diagnostics.rs`、
  `src-tauri/crates/application/src/events/diagnostics.rs`、
  `src-tauri/crates/application/src/facade/{diagnostics.rs,diagnostic_report.rs}`、
  `src-tauri/crates/application/src/requirements_tests/diagnostics.rs`、
  `src-tauri/crates/application/src/models/exchange_observation.rs`、
  `src-tauri/crates/infrastructure/src/adapters/exchange_observation{,/tests}.rs`、
  `src-tauri/crates/infrastructure/src/error.rs`。
- G029 UI/bindings：`src/features/capture/{exchange-observation-list.tsx,exchange-observation-list.test.tsx,exchange-observation-test-fixture.ts}`、
  `src/features/diagnostics/{diagnostic-logs-view.tsx,diagnostic-logs-view.test.tsx}`、
  `src/generated/rust-types.ts`。
- G029 文档：`docs/architecture/runtime-observability.md`、`docs/mcp/diagnostic-architecture.md`、
  `docs/mcp/external-package-integration-guide.md`、`docs/mcp/tool-reference.md`、
  `docs/architecture/decisions/ADR-005-runtime-evidence-and-reproduction-report.md`、
  `docs/architecture/modules.md`。
- G030 runtime：`src-tauri/crates/infrastructure/src/adapters/external_package_server.rs`、
  `src-tauri/crates/infrastructure/src/adapters/external_packages/actor/{registration.rs,runtime.rs}`。
- G030 tests：`src-tauri/crates/infrastructure/src/adapters/external_package_server/{tests.rs,tests/fault_isolation.rs}`、
  `src-tauri/crates/infrastructure/src/adapters/external_packages/{tests.rs,tests/fault_isolation.rs,tests/backpressure.rs,tests/coverage/transport.rs}`、
  `src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/{external_package_runtime.rs,external_package_runtime/support.rs}`。
- G030 docs：`docs/mcp/external-package-integration-guide.md`、`docs/onboarding-guide.md`、
  `docs/testing/release-validation-matrix.md`、`examples/external-packages/{au_eftex,iso8583-deno}/README.md`。
- G023-G028：Rust-owned protocol-rule contract、Listener CIDR 删除、SQLite executor、点查询与 Application
  snapshot、Infrastructure private adapter/窄 bundle/Host 边界，以及 deterministic architecture fixtures。
- G031 runtime：`src-tauri/crates/infrastructure/src/adapters/listener_runtime/{lifecycle.rs,start.rs,document_rule_compiler.rs}`、
  `src-tauri/crates/infrastructure/src/adapters/pipeline/{rule_runtime.rs,rule_runtime/actor.rs}`、
  `src-tauri/crates/proxy/src/listener/{supervisor.rs,task_scope.rs}`、动态 SNI/TLS、Android owner、Host bootstrap 与 async provider 路径。
- G031 cleanup：certificate material、listener certificate resolution、Host architecture support、Socket rules test runtime、
  external relay contract tests、rule actor 和 body-codec epoch cleanup 的责任拆分文件。
- FINAL 档案：`docs/testing/evidence/2026-08-25/TASK-20260825-004/FINAL-ARCHITECTURE-VALIDATION/`、
  `docs/testing/evidence/README.md` 和本任务文档。

## 附加文件

- `.omx/plans/architecture-excellence-prioritized-review.md`
- `.omx/ultragoal/goals.json`
- `.omx/ultragoal/ledger.jsonl`
- `docs/testing/evidence/2026-08-25/TASK-20260825-004/G029-OBSERVABILITY/`
- `docs/testing/evidence/2026-08-25/TASK-20260825-004/G030-EXTERNAL-PACKAGE-FAULT-ISOLATION/`
- `docs/testing/evidence/2026-08-25/TASK-20260825-004/FINAL-ARCHITECTURE-VALIDATION/`

## 验收结果

- 总体结果：PASS
- 整体对抗审查：code reviewer `APPROVE`；architect `APPROVE/CLEAR`；剩余发现为零
- 已完成分项：G023-G031
- 当前分项：无；FINAL 已完成
- 未开始分项：无

## 测试结果

- G023-G028 的定向和全量结果已记录在 Ultragoal ledger；当前稳定树的最终复测结果已汇总并链接到
  [FINAL-ARCHITECTURE-VALIDATION](../../../testing/evidence/2026-08-25/TASK-20260825-004/FINAL-ARCHITECTURE-VALIDATION/README.md)。
- G029：runtime/reproduction 38 tests、Exchange observation 8 tests、Application diagnostics 14 tests、
  MCP 23 tests、前端 4 tests 和 TypeScript 通过；架构文档/边界/runtime gates、workspace
  all-target/all-feature strict Clippy、Fmt 和 `git diff --check` 全部通过。完整命令与输入见 G029 evidence。
- G030：Infrastructure external-package 124、Domain 37、Application 7、Host 1、Tauri MCP 23 全部通过；
  Infrastructure all-target strict Clippy、Fmt、architecture/runtime/socket/frontend gates 与 `git diff --check`
  通过。全仓 source-size gate 仅剩四个非 G030 累积超限文件；完整输入、输出、diff 与复测命令见 G030 evidence。
- FINAL 当前稳定树：`pnpm check` 全量通过，包含 bindings、ESLint、TypeScript、66 files/648 tests、production
  build、architecture/runtime/socket/frontend/source-size、bundle branding、Rust Fmt、workspace all-target/all-feature
  strict Clippy `-D warnings`、Windows Rust check 和 workspace tests；source-size 已 GREEN。
- Vitest 在 22:56:32-22:58:47 连续三次通过，每次 66 files/648 tests；Rust workspace
  `--all-targets --all-features -q` 在 22:58:47-23:00:03 连续三次通过。六次执行前后 task status SHA-256
  `de794492bf29fcc66cf846bc293fad1b2efe757277b0fd00b5ed86c4a684b538`、task diff SHA-256
  `1e2f8c5916c1092414e2f46a714f60ce1a0e311a92d4e787cd6b9f8b212cc9d3` 均保持不变。
- 六次稳定执行的 `tested_base_head` 为 `e93148bfd7533e6c18ae6ded3af88c0e2a06c2d7`；之后 HEAD 推进到
  `7d0e80df58e9dea710bb95438288233996f7edab` 的 TASK-006 提交仅修改任务外 `AGENTS.md`，未纳入本任务 diff。
- standalone socket relay Fmt 与 10/10 tests、workspace all-target/all-feature 非 quiet 复测、`git diff --check`
  均通过。完整输入、文件清单、状态、diff、实际结果和复测命令见 FINAL evidence。

## CI 情况

- 未触发远程 CI、发布或部署。

## 完成总结

- G023-G031 已按批准优先级全部完成：Rust 成为规则编辑业务合同唯一事实源；Listener 客户端 CIDR 完整删除且
  Android 目的 CIDR 保留；SQLite 统一进入共享 executor 并提供点查询/聚合快照；Infrastructure/Host 边界收窄；
  Listener、Rule actor、TLS、Document compiler、动态 SNI 和 Android 生命周期所有权明确；受信任无认证外部包具备
  故障隔离；完整 payload 观测保持有界；预发布 SQLite reset 合同未冻结为 Release 1.0。
- 行为回归、三轮 Vitest、三轮 Rust workspace、`pnpm check`、all-target/all-feature Rust、standalone Socket、
  architecture/runtime/socket/frontend/source-size、bindings 与 diff gate 全部 PASS。
- `ai-slop-cleaner`、独立 code reviewer 和独立 architect 最终分别为 `passed`、`APPROVE`、`APPROVE/CLEAR`；
  十项架构不变量在 FINAL `quality-gate.json` 中逐项通过。
- 相关源码、文档、测试资源、完整 split diff、复测命令、元数据和索引均已同步；任务归档到完成目录。
- 关联提交保持“未提交”：G031 既定目标明确要求 without committing/pushing，本任务关闭采用该明确例外；未 Push、
  未触发远程 CI、发布或部署。
