# phase6-rule-chain-transaction

- 任务：`TASK-20260829-002`
- 用例：`phase6-rule-chain-transaction`
- 状态：`VERIFIED / APPROVED / CHECKPOINT READY`
- 执行时间：`2026-08-30 12:20:00 +08:00` 至 `2026-08-30 15:38:58 +08:00`
- 父用例：[phase5-unified-rule-domain](../phase5-unified-rule-domain/README.md)

## 目的与结果

Phase 6 将可编辑 `one_shot` 提升到 RuleDefinition 顶层，并把 server-owned
`hit_count/last_hit_at/revision/Nth counters` 留在只读 runtime lifecycle，
并把 `NthHit` 提升为 common condition leaf；Socket 未获得 HTTP 能力。Application 新增唯一
`RuleChainTransaction`，私有持有 working HTTP message、Document、trace、pending control 与
lifecycle deltas。前序 HTTP/Document mutation 对后序条件可见；整条规则链只 encode 一次、
delta commit 一次，commit 成功前不发布 message、Document、trace 或 terminal control。

Infrastructure repository port 只接收 lifecycle delta；actor 删除旧 revision-conflict retry，
一次事务只尝试一次 commit。冲突时 joint 原消息保持不变，NthHit/one-shot 不消费；caller 在
actor-owned commit 开始后 abort 不会取消或重放 actor state machine。

真实 TDD RED 包括 Domain 缺少 lifecycle owner、Application 缺少 transaction、Infrastructure
旧 retry 测试方向错误，以及 checker 文件缺失。全量 Application 首轮另真实发现 common NthHit
`count=0` 未校验（457 PASS / 2 FAIL），在 Domain 唯一 owner 修复后原失败精确 2/2 与全量
503/503 通过。source-size 首轮发现 `unified_rule.rs` 580 行，职责拆分后为 492 行，独立 lifecycle
module 98 行；未放宽 500 行门禁。strict Clippy 的三项格式 finding 均修复后 fresh PASS。

初版独立 Reviewer 结论为 `REQUEST CHANGES`（P1=4、P2=1），独立 Verifier 结论为
`FAILED`（P1=2）。反例证明初版错误地用 `hit_count` 近似 Nth attempt、缺少 terminal
IP/证书 identity 隔离，公开 program/lifecycle tuple 可错配，普通 save wire 可伪造 runtime
统计，HTTP condition 会降级并丢失完整 `AppError`，repository delta subtraction/校验不够
fail-closed。每项均先加入永久 RED 或 checker mutation，再做最小修复；初版结论保留而未改写。

Repair 后，Nth attempt 由 `(rule_id, terminal IP, certificate fingerprint)` 独立持有；成功
no-match 会原子提交 Nth advance，任意执行/编码/校验/取消/冲突失败均不消费。私有 validated
plan 在任何 port 前拒绝 rule/lifecycle mismatch 与重复 ID；repository 先完整校验 duplicate、
zero、oversized、decrease、wrong-id 与 revision，再一次性提交。HTTP condition 原样传播全部
`AppError` 字段。copy 创建新 rule identity、revision=`INITIAL` 并重置 server-owned stats，只
复制已确认的可编辑 metadata、conditions/actions 与 `one_shot` 配置。

随后独立复审再次给出 `REQUEST CHANGES`（P1=2）：Infrastructure adapter 未镜像 Domain 的
`disable_one_shot && !has_hit` 约束，crafted Nth-only delta 可错误禁用 one-shot；actor 在 Nth/runtime
delta 校验失败返回前未恢复 engine checkpoint。两项均先用公开 adapter/actor 回归得到真实 RED，
再把 `RuleLifecycleDelta::validate_against` 设为统一校验 owner，并确保 prepare、Nth validation、
runtime validation 与 commit 任一失败都恢复 checkpoint，且不发布 message/control/trace/lifecycle。

最终 Phase6 checker mutation/正控 28/28，Cargo 实际发现 Domain 9、Application 11、
Infrastructure 8。Domain 128/128、Application 508/508、Infrastructure 695/695、Host 33/33、
Tauri 133/133、bindings deterministic、TypeScript focused 21/21 与全部静态门均通过。
完整十门单进程 checkpoint exit 0，前端 63 files / 543 tests，Rust workspace 全目标/全特性
0 failed、0 ignored。最终独立 Reviewer 结论为 `APPROVE`，Verifier 结论为
`VERIFIED / APPROVED / CHECKPOINT READY`，P0/P1/P2=0、`blockers=[]`；初版 findings 与新增
2 个 P1 均已关闭。Phase 7+、完整 Phase10/11 pipeline/codec 切换、Phase12 legacy 删除、
Phase15 UI、CI 与 Release 仍为 `NOT_RUN`。

## 可复测资源

- 活动 fixture：`test-support/fixtures/task-20260829-002/phase-6/rule-chain-transaction/contract-inventory.json`
- 当次输入快照：[inputs/contract-inventory.json](inputs/contract-inventory.json)
- Checker 输出：[outputs/checker-tests.txt](outputs/checker-tests.txt)
- 结构化结果：[outputs/verification-summary.json](outputs/verification-summary.json)
- 复测命令：[replay/commands.txt](replay/commands.txt)

协议原始报文、真实 Frame/Decode/Encode/Server/App 字节与 UI 截图为 `N/A`：Phase 6 验证
transaction/lifecycle 原子合同并接入现有 HTTP+Document joint bridge，不切换 Phase10/11 完整
新 pipeline/codec，也不实施 Phase15 完整编辑器。
