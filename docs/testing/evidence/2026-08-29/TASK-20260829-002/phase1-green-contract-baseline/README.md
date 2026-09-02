# phase1-green-contract-baseline

- 任务：`TASK-20260829-002`
- 用例：`phase1-green-contract-baseline`
- 目的：证明 G042 / Phase 1 的当前合同 inventory、可编译测试入口、generated bindings freshness/determinism 和完整十门禁在当前分支可重复判断为 GREEN，并保存独立 Verifier 的完整结论。
- 执行环境：macOS arm64，Asia/Shanghai；Node `v26.7.0`、pnpm `11.13.1`、rustc `1.97.1`、cargo `1.97.1`。
- 被测状态：分支 `codex/task-20260829-002`，HEAD `8aa787bffd99c6302c284b373e4f805e4d553a49`，HEAD tree `9dc6af46d03dff05512439738fe368a21a94a249`。验证时工作树包含本任务未提交文件及用户的 `docs/README.md` 修改；测试期间未修改产品代码、Cargo 或 generated bindings。
- 实际输入：`inputs/current-contract-inventory.json`，来源为活动 fixture `test-support/fixtures/task-20260829-002/phase-1/current-contract-inventory.json` 的当次快照。
- bindings 快照：`src/generated/rust-types.ts` SHA-256 `15d730c6afae0f9011bd6539ea98f339342d4e4b22a4751bff595d893815891c`。

## 预期

1. Bindings Node tests 5/5，包含输出删除后 generator 抛错时恢复 checked-in bytes 并传播原错误。
2. Phase 1 baseline tests 4/4，inventory 与 package checkpoint 精确一致，命令或路径漂移 fail-closed。
3. `pnpm check:bindings` 连续两次真实 Release 生成，checked-in、first、second 完全一致，执行后文件 SHA 不变。
4. 完整 checkpoint 严格执行十条命令：Phase 1 tests、bindings、architecture、source-size、lint、typecheck、完整前端测试、Rust fmt、Rust clippy、Rust workspace all-target/all-feature tests。
5. `git diff --check` 通过；独立 Verifier 无 P0/P1/P2。

## 步骤与结果

复测命令完整保存在 `replay/commands.txt`，结构化结果保存在 `outputs/verification-summary.json`。

| 检查 | 实际结果 |
| --- | --- |
| `pnpm test:bindings-check` | PASS，5/5 |
| `pnpm test:task-20260829-002:phase1` | PASS，4/4 |
| `pnpm check:bindings` | PASS，两次真实 Release 生成一致；恢复后 SHA-256 不变 |
| architecture / source-size / lint / typecheck | PASS |
| `pnpm test` | PASS，61 files / 531 tests |
| Rust fmt / strict workspace clippy | PASS |
| Rust workspace all-target/all-feature | 最终完整复跑 PASS，0 failed、0 ignored |
| `git diff --check` | PASS |
| 独立 Verifier | `VERIFIED`，P0=0、P1=0、P2=0 |

## 首次完整复验中的既有 ADB deadline 偶发失败

该事件不得从证据中省略：独立 Verifier 首次执行
`pnpm check:task-20260829-002:checkpoint` 时，前九项均 PASS，最后 Rust workspace 中唯一失败为
`adapters::android_adb::tests::forward_control::cancelled_stalled_response_removes_owned_forward_without_blocking_other_serial`。
失败位置为 `crates/infrastructure/src/adapters/android_adb/tests/forward_control.rs:114:5`，panic 文本为
`device A response must hit the outer deadline`；该次 Infrastructure 结果为 `647 passed; 1 failed; 0 ignored`。

Verifier 随后使用 `--exact` 定向连续运行该测试三次，每次均 `1 passed; 0 failed; 647 filtered`；之后再次执行完整 checkpoint，十门禁全部 PASS、exit 0。Phase 1 未修改该既有 ADB 测试或产品实现。本证据把首次失败、定向 3/3 和完整复跑 PASS 分开记录，不把首次失败改写为成功。

## N/A

- 协议原始报文、Frame、Decode/Encode：N/A；Phase 1 只建立当前合同清单与静态/自动化门禁，不执行协议交换。
- UI 截图与人工交互：N/A；本阶段不改变 UI，前端由现有 Vitest 531 项验证。
- 真实设备、真实外部服务、生产数据：N/A；不属于 Phase 1 合同基线范围。
- CI、push、Release、部署：N/A；未获授权且未执行。
- rollback commit：N/A；由主 Agent 在 G042 checkpoint 收口时单独创建，本证据不提交代码。

## 结果

`PASS`。G042 已由独立 Verifier 标记 `VERIFIED`，P0/P1/P2 均为 0；该结论只覆盖 Phase 1，Phase 2 至 Phase 18 仍未执行。
