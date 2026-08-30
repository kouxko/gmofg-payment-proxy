# phase9-local-sidecar-lifecycle

- 任务：`TASK-20260829-002`
- 用例：`phase9-local-sidecar-lifecycle`
- 状态：`VERIFIED / APPROVED / CHECKPOINT READY`
- 执行时间：`2026-08-30 23:10:00 +08:00` 至 `2026-08-31 01:04:27 +08:00`
- 父用例：[phase8-boa-sidecar-runtime](../phase8-boa-sidecar-runtime/README.md)

## 目的与结果

Phase 9 已把 Phase 8 的通用 Boa runtime 接入真实本地 Sidecar 进程。Sidecar 只接受私有
`--archive` 与 `--packages-url` 参数，严格读取 ZIP 后主动连接现有 `/packages` WebSocket，发送无 id
注册通知，并在同一 Boa Context 中串行处理固定 typed RPC。Proxy 新增 exact package process owner：
启动前停止并回收旧进程、断开旧 transport，最长等待 10 秒注册；超时或启动失败保留 SQLite
`enabled=true`、记录稳定错误且不重试。app-start 只后台启动持久化的 enabled local exact versions；
disable、manual restart、disconnect/exit、delete 与 app shutdown 都收敛到 exact child kill/wait，pending
transport 失败且不 replay。Listener 继续同时要求 enabled 与 online，duplicate exact identity 不接管。

严格 ZIP preview/commit 使用同一 registry，并原子持久化 local archive；importer 只读取严格
ZIP/Manifest/resources，不加载或执行 Boa。commit 后即 enabled 并由唯一 Sidecar Boa owner 启动。
Schema 字段允许缺失的既有 HTTP Manifest 同步为 Rust/generated/前端 nullable schema，不增加第二协议、
timeout、queue、Busy、retry 或 recovery。Phase 10/11 pipeline/codec、Phase 12 legacy 删除、Phase 13
built-ins/templates、Phase 15 完整 UI 均未提前实现。

Review repair 进一步区分本地与远端生命周期：disabled local offline 可以启用并启动，但必须启用成功
后才允许 manual restart；remote offline
继续拒绝。Application port/use case、Tauri command、generated binding 与详情 UI 提供 manual restart，
exact identity 由串行 lifecycle gate 保证同一时刻最多一个进程，旧进程先 kill/wait 后再 spawn；pending
调用失败且不 replay。Supervisor 是 temp/freeze/spawn/timeout/exit 错误的唯一持久化 owner，caller 不再
用 generic transport error 覆盖。两条 Phase 8 IPC fail-closed 断言已迁移为 Phase 9 真实成功与持久状态。

## TDD 与复测

- Cargo discover/compile RED：Phase 9 真实进程测试最初因缺少 Tokio process feature exit `101`；typed
  dispatch 随后以 `E0308` 失败，修复后真实进程 2/2 PASS。
- persistence RED：local archive outcome 缺失导致 exit `101`，实现原子 install/reuse/conflict 后 PASS。
- checker RED：canonical 初次因 Cargo discovery 过滤错误失败；source-size 后续以 registry 596 行失败，
  拆出 `local_archives.rs` 后不放宽 500 行门禁并 PASS。
- Phase 9 checker：canonical + 14 negative mutations，15/15 PASS。
- package-runtime all targets：20/20 PASS；真实 process focused 2/2 PASS。
- strict importer no-evaluation 3/3、supervisor focused 4/4、Application lifecycle 8/8 PASS；覆盖
  restart kill/reap、并发 exact max-one、process error owner、timeout 保留 enabled、无 retry、shutdown 无 orphan。
- 两条完整名真实 Tauri IPC 各发现 1 项并 PASS；短名 0-test 继续明确不作为证据。
- 最终 P1 disabled-local restart guard 三层 RED→GREEN：Application 完整名 1/1、UI 2/2、真实
  Tauri IPC 1/1 PASS；Application port 调用计数保持不变，证明拒绝发生在 restart port 之前。
- 首次合法注册 enabled-state 精确用例完整名 1/1 PASS；external package affected 76/76 PASS。
- Infrastructure full 591/591、Application full 460/460、协议包前端 affected 84/84 PASS。
- bindings fresh/deterministic、typecheck、architecture、source-size、fmt、affected strict Clippy 与
  `git diff --check` 均 PASS。
- 本轮唯一完整 checkpoint 的前九门全部 PASS，前端 64 files/545 tests PASS；最终 workspace 在
  Tauri 129/130 后，既有 non-loopback MCP HTTP exchange 用例 10 秒 deadline 超时。affected Tauri full
  同样复现，因此如实标记 `GLOBAL CHECKPOINT ENVIRONMENT BLOCKED`，未修改 timeout/retry，也未重跑
  完整 checkpoint。

复测命令为：

```text
node --test scripts/check-task-20260829-002-phase9-lifecycle.test.mjs
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-package-runtime --all-targets
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --lib -q
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application --lib -q
pnpm exec vitest run src/features/protocol-packages
pnpm check:bindings
pnpm typecheck
pnpm scan:architecture
pnpm scan:source-size
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-package-runtime -p intercept-proxy-infrastructure -p intercept-proxy-application -p intercept-proxy-host --all-targets --all-features -- -D warnings
git diff --check
pnpm check:task-20260829-002:checkpoint
```

真实 macOS `.app` bundle 的 Sidecar 定位、系统权限/防火墙弹窗与签名后进程 E2E 需要人工环境，按用户
指示为 `NOT_RUN`，不阻塞后续代码工作。完整 workspace checkpoint 已按约定仅执行一次并记录上述环境
阻塞。最终独立 Reviewer 结论为 `APPROVE`，Verifier 结论为
`VERIFIED / APPROVED / CHECKPOINT READY`；P0=0、P1=0、P2=0，`checkpoint_ready=true`。既有
non-loopback MCP HTTP 环境超时继续作为历史阻塞保留，真实 macOS bundle/权限验证保持 `NOT_RUN`；
未提交、未 push、未触发 CI/Release。
