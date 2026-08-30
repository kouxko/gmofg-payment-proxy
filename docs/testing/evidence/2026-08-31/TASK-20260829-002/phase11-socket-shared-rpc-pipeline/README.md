# phase11-socket-shared-rpc-pipeline

- 任务：`TASK-20260829-002`
- 用例：`phase11-socket-shared-rpc-pipeline`
- 状态：`VERIFIED / APPROVED / CHECKPOINT READY`
- 执行时间：`2026-08-31 03:20:00 +08:00` 至 `2026-08-31 05:23:13 +08:00`
- 父用例：[phase10-http-shared-rpc-pipeline](../phase10-http-shared-rpc-pipeline/README.md)

## 目的与结果

Phase 11 将外部 Socket package 的 Frame、Decode、Display、Rules、Encode 接入统一 package 主动
`/packages` typed RPC 和 Phase 6 actor transaction。Listener plan 只装配带共享 `PipelinePorts` 的
production factory；Decode 保存当前 Frame 的原始字节和 Document，Rules 把两段方向 Program 作为
一个 joint evaluation 交给 actor，actor 在 Encode 成功后才提交 lifecycle。Encode 失败恢复 actor
checkpoint，不提交 one-shot/hit count；成功只提交一次。

Document unchanged 时直接返回原始 Socket bytes，0 Encode RPC；changed 时以原始输入的 canonical
Base64 调用 exact direction Encode。Frame `consumedBytes` 继续由 Exchange 拒绝 zero/oversize；Display
仍是 observation-only fail-open。typed Encode failure 保留 stage/request_id/stable code。Socket generated
adapter 不包含 HTTP-only headers/method/status/url。未新增 timeout、queue、Busy、retry、replay或恢复。
Phase 12 legacy 删除、Phase 15 UI 与后续阶段未提前实施。

## TDD 与复测

- RED：首个 Infrastructure compile exit 101；先后暴露 2 个 Proxy contract error，随后 8 个 joint
  capability/plan/Send 错误。architecture RED 分别发现 HTTP 子模块持有 Socket joint 职责和
  Proxy→Domain crate 反向依赖；迁移到协议中立 `joint_document` 并用 UUID/ProxyResult gate 后关闭。
- focused：production unchanged、changed、typed Encode failure 3/3 PASS；actor lifecycle rollback/commit
  1/1 PASS；external relay affected 8/8 PASS；Exchange Socket 3/3 PASS。
- checker：canonical + 13 negative mutations，14/14 PASS；覆盖 consumedBytes、Display observation、共享
  pipeline 注入、joint actor/Encode、原字节/canonical Base64、禁止 retry、Cargo discovery、typed failure、
  lifecycle rollback 和 generated Socket/HTTP capability separation。
- static：fresh compile、bindings freshness/determinism、architecture、source-size、fmt、affected strict
  Clippy 与 `git diff --check` PASS。
- review repair：真实 production bundle 与 E2E fixture 统一经 `ListenerRuntimePipelineAssembly` 装配共享
  `RuntimePipelineAdapter`；Socket handler 使用 Workspace runtime epoch 访问同一 actor。`RuleRepositoryAdapter`
  的 snapshot/order/signature/commit/reset 已统一覆盖 HTTP + Socket，首次 prepare 的零变化 reset 不再写库。
  真实 SQLite + production pipeline + 两阶段 Socket Program 证明 typed Encode 失败为 0 lifecycle commit 且
  revision/state 不变，随后成功只写一次并将两个 one-shot 各命中一次。external runtime 5/5、stale
  LocalResponder direction contract 1/1、checker canonical + 19 mutations 20/20 PASS。
- repair affected full：Infrastructure 602 项中 600 PASS；本轮暴露的 LocalResponder stale assertion 已迁移并
  精确复测 PASS；另一个未修改 Android ADB outer-deadline 并发测试失败，记录为环境/时序阻塞且未重跑
  full。后续 architecture、source-size、fmt、三受影响 crate strict Clippy、checker 与 diff 均 fresh PASS。
- second review repair：Socket `ConditionTree` 只把 actor-owned `NthHit` 投影到共享 RuleEngine，Document
  条件继续由 joint Program gate，未恢复或放开 `AppToProxy`/`UpstreamToProxy` 保存能力。权威 TASK
  仅允许 `ProxyToUpstream`/`ProxyToApp` 两个写出阶段；真实 Relay 先在上行用 NthHit(2) 修改 `[a,b]`
  为 `[x,b]`，真实 upstream echo 后，下行规则观察 `x` 再改为 `[x,y]`。首次 Nth miss 只提交 counter
  advance；第二次 Encode 失败保持 SQLite revision/lifecycle 且不消费 actor counter；重试仍命中。
  upstream echo 暂停点证明上行只提交一次，最终下行只追加一次提交。fresh exact 1/1、Domain 87/87、
  external runtime 5/5、checker canonical + 21 mutations 22/22、architecture/source-size/fmt、Domain+
  Infrastructure strict Clippy 与 diff PASS。
- 唯一完整 checkpoint session `24690`：Phase1、bindings、architecture、source-size、lint、typecheck、
  frontend 64 files/545 tests、fmt、workspace strict Clippy 全部 PASS；workspace tests 在首个 Tauri lib
  129/130 后仅既有 non-loopback MCP HTTP exchange 10 秒 deadline 超时，后续 workspace targets 因
  `&&` 停止均为 `NOT_RUN`。按用户指示不修改 timeout/retry，也不重跑 checkpoint。

复测命令：

```text
pnpm test:task-20260829-002:phase11
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure socket_encode_failure_rolls_back_lifecycle_before_successful_commit
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-exchange socket
pnpm scan:architecture
pnpm scan:source-size
pnpm check:bindings
cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-domain -p intercept-proxy-runtime -p intercept-proxy-infrastructure --all-targets --all-features -- -D warnings
pnpm check:task-20260829-002:checkpoint
```

真实 macOS `.app` bundle、系统权限/防火墙弹窗与签名后 E2E 需要人工环境，保持 `NOT_RUN`。未重跑完整
workspace checkpoint；session `24690` 仍是唯一完整 checkpoint。远程 CI、push、Release 均未执行。
最终独立 Reviewer 结论为 `APPROVE`，Verifier 结论为
`VERIFIED / APPROVED / CHECKPOINT READY`；P0=0、P1=0、P2=0，`checkpoint_ready=true`。
唯一 checkpoint 的既有 non-loopback 环境阻塞、Android deadline 环境时序阻塞以及人工 bundle/
permissions `NOT_RUN` 继续作为执行事实保留，不影响最终代码与合同 verdict。
