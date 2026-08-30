# phase10-http-shared-rpc-pipeline

- 任务：`TASK-20260829-002`
- 用例：`phase10-http-shared-rpc-pipeline`
- 状态：`VERIFIED / APPROVED / CHECKPOINT READY`
- 执行时间：`2026-08-31 01:15:00 +08:00` 至 `2026-08-31 03:12:48 +08:00`
- 父用例：[phase9-local-sidecar-lifecycle](../../../2026-08-30/TASK-20260829-002/phase9-local-sidecar-lifecycle/README.md)

## 目的与结果

Phase 10 将 HTTP Body 协议包从旧 in-process runtime 切换到与 Socket 相同的 package 主动连接
`/packages` typed RPC。Listener start 从统一在线 registry 冻结 exact HTTP package binding；HTTP
Decode、Display 与 changed Document Encode 均调用该 binding，production 编译中旧 HTTP executor 和
legacy joint variant 只保留为 `cfg(test)`，没有第二条生产运行路径。

Exchange `HttpContext` 新增 authoritative `wire_body`。UTF-8、Shift-JIS 及确认的 alias 通过严格 codec
处理；未知/缺失 charset、非法字节、不可表示文本和非 identity `Content-Encoding` fail-closed。
Document 未变化时不调用 Encode RPC并转发原始 wire bytes；变化时使用同一原始输入、working Document
和原 codec 编码。现有 joint actor 继续在 encode 成功后才提交 HTTP/Document/control/lifecycle，冲突不
重试；Display 仍由 Exchange reader 的 observation-only fail-open 边界处理，Decode/Rules/Encode 失败则
终止 Exchange。未新增 timeout、queue、Busy、retry、replay 或 recovery。

`wire_body` 是 Rust Exchange 内部上下文，不是 generated/public DTO；因此 minimal TS adapter 无需新增
字段。bindings freshness/determinism 与 `tsc --noEmit` 已证明 generated TypeScript 和现有消费者未漂移。
Phase 11 Socket pipeline、Phase 12 legacy 删除及 Phase 15 完整 UI 未提前实现。

Review repair 后，JSON-RPC remote failure 的 package、direction、stage、method、request id、numeric
code、stable code、message 与 data shape 从 HTTP Decode/Display/Encode 的唯一错误 owner 贯穿
Exchange、Proxy/child-task aggregate 与 capture typed event，不再折叠成 String/Internal。失败仍在 joint
actor commit 前回滚，不发布 prepared message 或 lifecycle delta。production-shape 用例从 fake shared
provider 经 `prepare_async` 建立双向 capability，并覆盖 unchanged 0 Encode；changed 与 typed failure
rollback 由同一 focused suite 覆盖。旧 executor 文件与 module declaration 均由 checker 限制为
`cfg(test)`。

最终 review repair 进一步把 remote failure 的顶层 Proxy code 固定为
`EXTERNAL_PACKAGE_CALL_FAILED`，保留 package `data.code` stable code，禁止再映射为
`INTERNAL_ERROR`。Display 的 fail-open 不变，但 typed failure 会进入 capture observation；endpoint
转换也保留 `external_package_call`，因此 Encode 失败可到达 observation/child-task/capture。production
用例现真实经 provider→`prepare_async`→双向 capability→joint actor，分别证明 changed Encode 成功
提交一次，以及 typed Encode 失败时 message/lifecycle 不变且零提交；测试已拆分到 Clippy 行数限制内。
后续纯结构 repair 将 production-shape fixture 与断言移入职责子模块，主测试文件 386 行、子模块
264 行，均低于 500 行门禁；六条 Cargo 测试名与行为保持不变。

## TDD 与复测

- Cargo RED：Phase10 fixture 首先因 `HttpContext.wire_body` 和 strict helper 缺失产生 `E0432/E0560`；
  checker 初始报告 11 项缺失。GREEN 后 focused 4/4 PASS。
- checker：canonical + 18 negative mutations，19/19 PASS；覆盖 wire owner、shared provider、HTTP kind、
  charset/content-encoding、禁止 legacy runtime/retry、unchanged gate、Encode RPC、stable top-level code、
  Display observation、endpoint typed conversion、production actor rollback 与 Cargo test discovery。
- Phase10 focused：6/6 PASS，覆盖 UTF-8/Shift-JIS、unknown charset/non-identity encoding、production
  `prepare_async` 双向 capability、unchanged 0 Encode RPC、changed Encode RPC及Decode/Display/Encode typed failure。
- legacy HTTP regression focused：12/12 PASS；确认 Phase10 original-byte gate 未改变旧测试专用 executor
  的 encode/非 UTF-8 拒绝行为。
- affected full：Application 460/460、Exchange 24/24、Runtime 180/180 + integration 47/47、Infrastructure
  597/597 + integration 46/46 PASS。
- bindings fresh/deterministic、TypeScript typecheck、architecture、source-size、lint、fmt、affected strict
  Clippy 与 `git diff --check` 均 PASS；前端完整 64 files/545 tests PASS。
- 本阶段唯一完整 checkpoint 为 session `3469`。Phase1、bindings、architecture、source-size、lint、
  typecheck、frontend 545/545、fmt 与 workspace strict Clippy 全部 PASS；最终 workspace tests 在首个
  Tauri lib 129/130 后，既有 non-loopback MCP HTTP exchange 用例等待 10 秒超时。命令使用 `&&`，
  因此该点之后的剩余 workspace targets 均为 `NOT_RUN`，不得描述为 PASS。

复测命令：

```text
pnpm test:task-20260829-002:phase10
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-exchange --all-targets --all-features
cargo test --manifest-path src-tauri/crates/proxy/Cargo.toml --all-targets --all-features
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --all-targets --all-features
pnpm check:bindings
pnpm typecheck
pnpm scan:architecture
pnpm scan:source-size
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-exchange -p intercept-proxy-runtime -p intercept-proxy-infrastructure --all-targets --all-features -- -D warnings
git diff --check
pnpm check:task-20260829-002:checkpoint
```

non-loopback 网络用例按用户离席指示记为环境阻塞，不修改其 timeout/retry，也不重跑完整 checkpoint。
真实 macOS `.app` bundle、系统权限/防火墙弹窗与签名后 E2E 需要人工环境，保持 `NOT_RUN`。独立
Reviewer 已 `APPROVE`，最终 Verifier 为 `VERIFIED / APPROVED / CHECKPOINT READY`，P0/P1/P2 均为
0、`checkpoint_ready=true`。唯一 checkpoint session `3469` 的环境阻塞与后续 targets `NOT_RUN`
继续作为历史执行事实保留；未提交、未 push、未触发 CI/Release。
