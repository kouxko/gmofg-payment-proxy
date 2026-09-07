# Phase17 Release Schema100 preservation 证据

## 结果

`VERIFIED / APPROVED / CODE CHECKPOINT READY`。Phase17 已物理删除开发期 pre-100
recreate/reset policy、marker 与启动分支；Host 与 Tauri 只保留 Schema100 preserve/fail-closed
路径。真实 `cargo test --release --exact` 双启动 fixture 证明 Workspace、统一递归规则及 lifecycle、
外部包 registry/archive/manifest/enabled/online/recent error 状态跨两次真实 Host 启动保持。

Reviewer `APPROVE`、Verifier `VERIFIED`，P0/P1/P2=`0/0/0`，`code_checkpoint_ready=true`。

## 被测状态与范围

- baseline HEAD：`c23f7927214363c053b1769d009a9c8706436e6a`
- 执行时间：`2026-08-31 16:56:17 +08:00`
- 用户已取消 hash 验收；本证据不生成或声明 source/worktree hash。
- 精确文件范围见 [resources/file-scope.txt](resources/file-scope.txt)。
- `docs/README.md` 中未属于 Phase17 的用户任务入口及 `docs/tasks/pending/2026-08-31/` 均不在范围。

## RED

- Node release checker 初始为 `6/8 PASS`、`2 FAIL`：生产源码仍含临时 marker 与 reset policy 合同。
- Infrastructure exact `preserve_only_startup_rejects_pre_schema100_without_modifying_it` 初始为
  `0/1 PASS`：Schema99 被接受并重建，而非 fail-closed。
- 一个 Host 短名命令实际发现 `0 tests`，明确不作为证据；后续聚合入口全部使用完整名称与
  `-- --exact`，Release fixture 同时强制 `--release`。

## GREEN

- `pnpm test:task-20260829-002:phase17`（session81727，exit `0`）：Node `10/10`、Infrastructure
  fail-closed exact `1/1`、Host pre-100 no-mutation exact `1/1`、Release optimized 双启动 exact
  `1/1`，没有 0-test。
- 独立 Release 证明（session83719，exit `0`）：
  `cargo test --release --manifest-path src-tauri/Cargo.toml -p intercept-proxy-host --lib tests::phase2_database_startup::release_startup_preserves_schema100_state_across_two_real_host_starts -- --exact`
  输出 `Finished release profile [optimized]`，`1 passed; 0 failed`。
- affected：Infrastructure `501/501 PASS`；Host `10/10 PASS`。
- final repair：Node checker/mutations `12/12 PASS`；Infrastructure all-target/all-feature check、
  architecture、source-size、lint、typecheck、Rust fmt、strict Clippy 与 diff check 全部 PASS。

结构化结果见 [outputs/verification-summary.json](outputs/verification-summary.json)。原始 session stdout
未单独落盘，不能恢复的 raw output 不伪造。

## BLOCKED / PARTIAL / NOT_RUN

- Application full（session1189）：`BLOCKED / PARTIAL`，人工终止后 exit `130`。已观察到既有
  `limits_red::rejects_android_weak_network_json_larger_than_two_hundred_fifty_six_kibibytes` 失败，
  以及两个 validation cancellation/deadline tests 持续挂起；raw stdout unavailable。其后的
  Application completion 为 `NOT_RUN`，不得用 targeted/其他 crate PASS 替代。
- Phase17 full workspace checkpoint：`N/A`；权威 NDR-JS-12 将 overall full-layer acceptance 分配给
  后续阶段，本阶段未机械重跑。
- 人工 UI、真实 App、外部网络、CI、push、Release 发布：`NOT_RUN`。

## 复测

```bash
pnpm test:task-20260829-002:phase17
node scripts/check-task-20260829-002-phase2-release-blocker.mjs
pnpm scan:architecture
pnpm scan:source-size
pnpm lint
pnpm typecheck
pnpm check:rust:fmt
git diff --check
```
