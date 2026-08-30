# phase12-legacy-stage-deletion

- 任务：`TASK-20260829-002`
- 用例：`phase12-legacy-stage-deletion`
- 状态：`VERIFIED / APPROVED / CODE CHECKPOINT READY / GLOBAL CHECKPOINT INCOMPLETE`
- 执行时间：`2026-08-31 05:23:13 +08:00` 至 `2026-08-31 06:43:40 +08:00`
- 父用例：[phase11-socket-shared-rpc-pipeline](../phase11-socket-shared-rpc-pipeline/README.md)

## 目的与结果

Phase 12 在 Phase 10/11 两条 production pipeline 已绿后删除旧四阶段消息规则合同。Domain、Application、
Infrastructure、Tauri/generated types 与即时前端消费者只保留 `ProxyToUpstream` 和 `ProxyToApp`；旧
`AppToProxy`、`UpstreamToProxy` wire 数据通过 Serde 边界明确 fail-closed，不增加 alias、迁移、fallback
或双执行路径。HTTP field/operator 直接由统一 `Condition::Http` owner 持有，旧 owner allowlist 为空。

阶段 checker 扫描 Rust/TS/generated/runtime factory 和活动 current-state inventory，mutation 覆盖旧 enum、
旧 wire、四阶段 factory、UI/generated 回加、旧/改名 HTTP condition owner、旧 owner allowlist 和 stale Phase1
inventory。Phase1 活动 inventory 已同步为两权威写出阶段；历史 evidence snapshot 未修改。

## TDD 与复测

- RED：Cargo wire contract 首次 1/2 失败，证明旧阶段仍能反序列化；checker 首次发现 45 个活动旧阶段/
  owner/factory 残留。删除后 Cargo 2/2、checker canonical + 9 negative mutations 10/10 PASS。
- focused：Phase1 active inventory 4/4、Phase12自身checker 10/10 + Cargo 2/2、前端规则相关 focused、bindings
  6/6 与 deterministic generation PASS。
- affected：Domain 与 Application affected run exit 0；Infrastructure `external_package_runtime` 5/5、
  `http_protocol_pipeline` 12/12 PASS。
- static：architecture、source-size、lint（零 warning）、typecheck、fmt、affected strict Clippy 和
  `git diff --check` PASS。
- cross-phase repair：fresh Phase5 checker 先因 Vitest expected 4/actual 5 与 generated golden 仍引用
  `RuleAction` 真实失败。活动 Phase5 inventory 更新为实际 Vitest 5 项，checker 强制 Phase12 legacy owner
  allowlist 为空且任何 owner 回加失败，generated golden 精确锁定 `Condition::Http` 的 field/operator 与
  `UnifiedAction::Http(HttpAction)`；Phase12 aggregate 真实纳入 Phase5 mutation/checker。修复后 Phase5
  15/15（Cargo discovery 9、Vitest discovery 5），Phase12 combined Node 25/25、Cargo 2/2，Domain 87/87
  及全部 integration、Vitest 5/5、Phase1 4/4、静态门均 PASS。Phase5 历史 evidence snapshot 未修改。
- mutation fixture repair：Phase12 mutation 改为具名路径，`protocol legacy enum`、`legacy UI copy` 与
  `generated legacy wire` 分别真实修改 protocol enum、UI model 和 generated bindings 文件，并逐项
  fail-closed；同步删除 `restore` 仍接受 legacy stages until Phase12 的过时注释。复跑 combined 25/25、
  Cargo 2/2、fmt/source-size/diff PASS。
- 唯一完整 checkpoint session `91671`：第 1 门 Phase1 baseline 3/4，因活动 inventory 仍要求已删除的
  `app_to_proxy`、`AppToProxy`、`UpstreamToProxy` fragments 而 exit 1；后 9 门因 `&&` 均为 `NOT_RUN`。
  随后只修活动 current-state inventory 及 checker mutation，Phase1 4/4、Phase12 10/10+2/2 和全部静态门
  PASS；按指令不重跑完整 checkpoint。最终 Reviewer `APPROVE`、Verifier `VERIFIED`，P0/P1/P2=0，
  因此代码检查点为 ready；但 session `91671` 未形成完整全局 checkpoint，必须保持
  `global_checkpoint_complete=false`，不得把全局门禁写成 PASS。

复测命令：

```text
pnpm test:task-20260829-002:phase1
pnpm test:task-20260829-002:phase12
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --all-features external_package_runtime -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --all-features http_protocol_pipeline -- --test-threads=1
pnpm check:bindings
pnpm scan:architecture
pnpm scan:source-size
pnpm lint
pnpm typecheck
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
git diff --check
```

真实 macOS `.app` bundle、系统权限/防火墙弹窗与签名后 E2E 需要人工环境，保持 `NOT_RUN`。既有
non-loopback/Android 环境敏感测试在 session `91671` 未执行，不能声明本轮 PASS 或失败。远程 CI、push、
Release 均未执行。

最终结论：`code_checkpoint_ready=true`、`global_checkpoint_complete=false`。Phase 13+、真实 macOS
bundle/权限弹窗、远程 CI、push、Release 与 Windows E2E 继续保持 `NOT_RUN`。
