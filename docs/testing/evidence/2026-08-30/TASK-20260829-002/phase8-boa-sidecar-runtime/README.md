# phase8-boa-sidecar-runtime

- 任务：`TASK-20260829-002`
- 用例：`phase8-boa-sidecar-runtime`
- 状态：`VERIFIED / APPROVED / CHECKPOINT READY`
- 执行时间：`2026-08-30 21:25:00 +08:00` 至 `2026-08-30 22:57:40 +08:00`
- 父用例：[phase7-package-runtime](../phase7-package-runtime/README.md)

## 目的与结果

Phase 8 已实现可编译的通用 Rust Sidecar 边界和单个 Boa `Context`：package-relative 小写 `.js`
模块在虚拟 `/package` 根内解析，入口及静态依赖正常 load/link/evaluate，合法 `dynamic import()` 由同一
Boa loader 在 Hook 调用时加载并按 Boa cache 只求值一次；固定八个 callable export 在注册预检时缓存，
后续调用通过 `&mut self` 串行进入同一 Context。HTTP hook 只收发 JSON 字符串；Socket hook 将 canonical
Base64 解码成 JavaScript `Uint8Array`，并严格要求 encode 返回 `Uint8Array` 后再编码为 canonical Base64。
HTTP package 不被要求实现 Socket frame exports，普通 JavaScript Array 不能冒充 Socket bytes。

Boa `0.22.0` 原生 default features 已启用，不人为限制 Boa 自身能力。Proxy 没有额外发明 Boa 本身没有的
Node/fs/process Host bindings，也未增加 Rhai 或其他执行回退。Hook 返回 Boa Promise 时持续驱动 jobs 直到
fulfilled/rejected；不设置 timeout、queue、Busy 或中断，永久 pending 按已确认合同持续占用该 Context。
Sidecar binary 仅提供通用可编译 marker；Phase 9 的进程参数、启动和注册生命周期均未实现，Tauri
`externalBin` 未修改。

真实 TDD 先在精确 HEAD `03144593ca929379fdb848516c35fcd92743106c` 的 detached 临时 worktree 中，
只重放新增 Phase 8 test 和编译所需依赖声明：缺少 Sidecar binary 与 `LocalSidecarRuntime`，Cargo exit
`101`。其后 ordinary Array encode 用例在修改前真实失败（0 passed / 1 failed），证明旧实现错误接受
非 `Uint8Array`。review repair 另真实取得 dynamic import Hook Promise 被错误转成空 Document 的 0/1 RED；
修复后 lazy module 两次 Hook 只求值一次。Phase 8 当前 runtime 9/9 + review 3/3，共 12/12 通过。

最终 P2 checker repair 先以三类 mutation 取得 18 passed / 3 failed RED：Proxy 侧
`register_global_*`、`NativeFunction` 和 custom `HostHooks` 均可绕过；随后对去除注释/字符串后的 Rust
结构 token 精确扫描，禁止 Proxy 注入非 Boa Host binding，但不检查 Boa 自身 globals/default features。
checker mutation/正控现为 21/21，通过两个真实 Cargo target discovery 确认 Phase 8 12 个测试；package-runtime
all-targets 同时保持 Phase 7 ZIP 6/6。Phase 7 聚合、Phase 4 contract、package-contract 13/13、
protocol-scripting 160/160、Infrastructure 585/585 与相关 transport/R07 suites 全部 fresh PASS。
bindings fresh/deterministic、architecture、source-size、lint、typecheck、fmt、package-runtime strict
Clippy 和 `git diff --check` 均 PASS。repair 后唯一完整十门 session `47419` 终态 exit `0`：前端
63 files/543 tests、workspace all-target/all-feature 全部零失败；Tauri 130、Application 458、
Infrastructure 585、Android 47 均通过。更早 PID `18568`/`26450` 未保存 exit 的观察记录被本轮真实
exit0 取代，不再作为当前验收结果。

## RED、结果与复测

- 精确 HEAD 编译 RED：[phase7-baseline-red.stderr.txt](outputs/phase7-baseline-red.stderr.txt)
- ordinary Array 严格类型 RED：[non-uint8array-red.stderr.txt](outputs/non-uint8array-red.stderr.txt)
- dynamic import Promise RED：[dynamic-import-promise-red.stderr.txt](outputs/dynamic-import-promise-red.stderr.txt)
- Host binding checker RED：[checker-host-binding-red.stderr.txt](outputs/checker-host-binding-red.stderr.txt)
- 结构化结果：[verification-summary.json](outputs/verification-summary.json)
- 复测命令：[commands.txt](replay/commands.txt)

真实 Phase 9 Sidecar 进程启动、参数、注册和生命周期，Tauri
`externalBin`/bundle，Phase 10/11 pipeline，Phase 12 legacy 删除，Phase 13 built-ins/templates，Phase 15
UI，以及 CI、push、Release、Windows/macOS bundle/E2E 均为 `N/A / NOT_RUN`。最终独立 Reviewer
结论为 `APPROVE`，Verifier 结论为 `VERIFIED / APPROVED / CHECKPOINT READY`；P0=0、P1=0、P2=0、
`blockers=[]`、`checkpoint_ready=true`。全部需求变更、历史 findings、RED/GREEN 与 Phase9+ `NOT_RUN`
边界继续保留。
