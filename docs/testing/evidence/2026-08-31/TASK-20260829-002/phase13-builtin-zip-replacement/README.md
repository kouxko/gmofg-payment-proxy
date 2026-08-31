# phase13-builtin-zip-replacement

- 任务：`TASK-20260829-002`
- 用例：`phase13-builtin-zip-replacement`
- 状态：`VERIFIED / APPROVED / CODE CHECKPOINT READY / GLOBAL CHECKPOINT ENVIRONMENT BLOCKED`
- 执行时间：`2026-08-31 06:44:00 +08:00` 至 `2026-08-31 09:52:13 +08:00`
- 父用例：[phase12-legacy-stage-deletion](../phase12-legacy-stage-deletion/README.md)

## 目的与结果

Phase 13 将内置 ISO8583 Socket 包、作者模板、MCP resource 和 production seed 收敛为严格根 ZIP：
`manifest.json`、`protocol.js`、`display.js`。旧 `manifest.toml`、`document.toml`、`protocol.rhai`、
`display.rhai`、Rhai library、`protocol-scripting` crate 及其 production/runtime/persistence owner 已删除；
不保留 alias、迁移、fallback 或双执行路径。旧数据依合同 fail-closed，由用户人工清理。

production bundle 仅把 strict ZIP 安装为 enabled/offline 的本地外部包，随后仍由统一 Sidecar 主动连接
`/packages` 并注册；未恢复 App 内嵌执行 owner。活动 architecture/runtime/dependency/coverage checkers、
Phase 4/7/10/11 predecessor contracts、generated bindings 和用户文档已同步到当前合同；历史 evidence
snapshot 未修改。

## TDD 与复测

- RED：Cargo 实际发现 2 个 Phase13 built-in tests，初始 2/2 失败（严格 ZIP 文件缺失且 legacy 文件仍在）；
  Phase13 checker 初始发现 22 个 legacy owner/resource/dependency 残留。
- GREEN：Phase13 Cargo 2/2；checker canonical、mutations 与 discovery controls 18/18。checker 额外锁定
  `Internal` source、旧 store/compiler/portability ports、Application store field、lookup merge、bundle stub、
  repository adapter stub 全部不得回加，并要求 built-in test 实际执行 Display export。
- production 回归：两个旧短名 `--exact` 命令实际均发现 0 tests，已明确判为无效证据；改用完整测试名后
  production seed 1/1、Sidecar online gate 1/1 PASS。Cargo/checker discovery 对 0-test 结果 fail-closed。
  strict ZIP 只含三个权威根文件，enabled/offline 后由 Sidecar online gate 接管。
- affected：Phase1 4/4；Phase4 canonical；Phase7 checker22/package runtime22/transport7/external runtime5/
  ceiling1/diagnostic1；Phase8 checker21；Phase9 checker15/supervisor4/external registry77；Phase10
  checker19/Cargo6；Phase11 checker22/Cargo7；Phase12 aggregate25/Cargo2，均 PASS。
- static：fresh workspace all-target compile 0 errors/0 warnings；bindings fresh/deterministic；architecture、
  runtime/dependency/coverage、source-size、lint、typecheck、fmt、strict Clippy 和 `git diff --check` PASS；
  frontend Vitest 64 files / 545 tests PASS。
- review repair affected：Application 455/455 + integration 49/49、Infrastructure protocol-package 7/7 + archive
  1/1、前端相关 11 files / 161 tests、typecheck、lint、architecture、source-size、fmt、strict Clippy、diff 均
  fresh PASS；`ProtocolPackageSourceViewModel` 与 UI 仅保留统一 External 路径，内置身份由精确 package id
  判定，不再由第二 source variant 表示。
- final delta repair：统一执行 `pnpm bindings` 与 `pnpm check:bindings`，fresh/deterministic 生成的
  `ProtocolPackageSourceViewModel` 仅含 External union，注释明确 JavaScript ZIP；活动 Phase4 inventory 的
  generated SHA 同步到当前导出，历史 Phase4 evidence snapshot 不改。Phase4 gate 先真实拒绝旧 SHA，修复后
  checker 23/23、Cargo 13/13、TS 7/7 PASS。Application test helper 只提升到 lifecycle tests 模块树可见，
  all-target compile、full 504/504 与 strict Clippy PASS，未放宽 production API。
- 唯一完整 checkpoint session `7784`：前 9 门 PASS；第 10 门 workspace tests 在顶层 lib 发现 130 项，
  其中 2 项失败。其一为 stale production 断言，错误地期望 enabled/offline 的内置包在 Sidecar online 前
  可 preview；断言修为 `EXTERNAL_PACKAGE_OFFLINE` fail-closed 后 exact 1/1 PASS，排除唯一环境项的 root
  lib 129/129 PASS。其二为既有
  `production_bind_is_reachable_on_current_platform_interfaces_without_false_availability` non-loopback MCP
  HTTP deadline `Elapsed(())`，记录为 `ENVIRONMENT BLOCKED`。Cargo 在该 target exit 101，后续 workspace
  targets 未运行；依指令不重跑完整 checkpoint，不把 global checkpoint 记为 PASS。

复测命令：

```text
pnpm test:task-20260829-002:phase13
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-package-runtime --test phase13_builtin_package --all-features
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure adapters::environment_apply_revision16_integration::internal_package_baseline::phase13_seed_projects_the_enabled_builtin_before_sidecar_start --all-features -- --exact
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy mcp::tests::g036_behavior_contract::application_lifecycle::production_apply::production_full_resource_candidate_requires_builtin_sidecar_online --all-features -- --exact
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy --lib --all-features -- --skip production_bind_is_reachable_on_current_platform_interfaces_without_false_availability
pnpm check:bindings
pnpm scan:architecture
pnpm scan:source-size
pnpm lint
pnpm typecheck
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features -- -D warnings
git diff --check
```

真实 macOS `.app` bundle、签名、系统权限/防火墙弹窗及人工清理旧本地数据保持 `NOT_RUN`；远程 CI、
push、Release、Windows bundle E2E 和 Phase14+ 保持 `NOT_RUN`。

最终结论：独立 Reviewer `APPROVE`、Verifier `VERIFIED`，P0/P1/P2 均为 0；Phase13 为
`VERIFIED / APPROVED / CODE CHECKPOINT READY`，`code_checkpoint_ready=true`。唯一 session `7784`
的 non-loopback 环境阻塞、后续 workspace targets 与人工项 `NOT_RUN` 均保留，故
`global_checkpoint_complete=false`，不得写为全局 PASS。
