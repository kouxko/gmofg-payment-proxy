# phase7-package-runtime

- 任务：`TASK-20260829-002`
- 用例：`phase7-package-runtime`
- 状态：`VERIFIED / APPROVED / CHECKPOINT READY`
- 执行时间：`2026-08-30 15:53:00 +08:00` 至 `2026-08-30 21:21:22 +08:00`
- 父用例：[phase6-rule-chain-transaction](../phase6-rule-chain-transaction/README.md)

## 目的与结果

Phase 7 已把活动 package 导入和 `/packages` WebSocket 切换到共享强类型合同：严格 ZIP 只接受根
`manifest.json`、`protocol.js`、`display.js` 与 package-relative 小写 `.js` 模块；Manifest 唯一 owner
为 `intercept-proxy-package-contract`。archive/entry/file/total/ratio/path-depth 产品限制全部复用现有合同，
非法 archive 或 Manifest 映射既有 `ProtocolPackageInvalid`。严格 JavaScript ZIP 已真实进入
`ProtocolPackageImportAdapter`，并在尚未实现的 Phase 8 JavaScript 编译边界稳定 fail-closed；旧
`manifest.toml`、Rhai、wrapper 与旧 prepare 路径不再被活动导入调用。

活动 transport 由 package 主动发送无 `id` 的 `package.register` notification，Proxy 不回复；hook
方法和结果均为固定 typed DTO。canonical Base64、`FrameResult` buffer 校验、raw logical frame 与
encoded wire budget 的 B/B+1 边界已分别验证。registration deadline、heartbeat、wire-size、shutdown
和 WebSocket backpressure 保留；未加入 hook timeout、max-in-flight、Busy、retry 或 replay。预注册静默
peer 的取消由连接 future 自身拥有且 join；顺序 RPC 不保留永久 completed-ID 集合，duplicate reply
继续 fail-closed。`PackageRpcError.data` 的 stable Domain code 已通过 typed Exchange context 进入真实
`SocketProcessingFailure`、terminal observer 与可查询 `external_package_call.stable_code`，numeric
JSON-RPC code 独立保留。ZIP entry 使用 `take(file_limit + 1)` 读取实际字节，拒绝 declared/actual
不一致并以 actual 计算 file/total/ratio。production WebSocket 底层 ceiling 使用
registration/RPC/display 三者最大值，各阶段继续执行自己的严格限额；真实 `/packages` 测试证明
registration < response <= RPC 可接收且 RPC+1 fail closed。

checker 真实扫描 active source、Cargo dependency 与唯一 DTO owner，并以 23 个 canonical/mutation/正控
覆盖注册方向、第二 owner、动态 method、旧 Hook timeout/Busy/retry、Base64/FrameResult、宽 allowlist、
Cargo discovery 与注释字符串正控。Cargo 真实发现 ZIP 6/6、transport 7/7；活动 importer/registry E2E
4/4，production WebSocket ceiling 与 stable-code diagnostic 各 1/1。两轮 review finding 均已 TDD 修复，旧 Tauri/E2E/Host 成功夹具已迁为严格 JS ZIP + Phase 8
fail-closed 断言，真实 IPC/Application 覆盖保留。

第三轮完整十门终态 exit `0`：generated bindings fresh/deterministic、architecture、source-size、lint、
typecheck、前端 63 files/543 tests、fmt、workspace strict Clippy、workspace all-target/all-features 全部
PASS；其中 Tauri 130、Application 458、Host 12、Infrastructure 585、Phase 7 transport 7、archive 6
均为零失败。此前 non-loopback MCP 环境失败本轮已通过，不再记录 blocker。

最终跨阶段修复删除活动 Phase 4 inventory 中 18 条已完成迁移的
`phase7_legacy_wire_allowlist`，并把该列表永久约束为空；精确 reallow mutation 与 stale generated
SHA mutation 均 fail-closed。活动 inventory 记录的 generated SHA-256 已按 fresh deterministic
bindings 字节复算为 `413e42788f02a616b18141bf9e7bbcc5217f775fc636b53a3f2d4bdd3b144123`，
且 Phase 7 聚合脚本真实串行执行 Phase 4 contract checker。Phase 4 mutation/正控 23/23、Phase 4
checker、Phase 7 聚合、bindings/static gates 均 fresh PASS；最终完整十门唯一 session `51772`
exit `0`，前端 63 files/543 tests、workspace all-target/all-features 零失败/零忽略。Phase 4 历史证据
快照保持不变；本用例保存本轮活动 inventory 的逐字节快照。

## RED 与可复测资源

在精确 Phase 6 SHA `56becb38decb5fc836d8274f65cc0a10b0761260` 的临时干净 detached
worktree 中，仅复制当前 Phase 7 transport test、checker 和 inventory 后真实重放：Cargo 因缺少
`PackageTransport*` 与 package-contract 以 `101` 失败；checker 因缺少 active
`package_transport.rs` 以 `1` 失败。原始输出：

- [Cargo RED](outputs/phase6-baseline-transport-red.stderr.txt)
- [checker RED](outputs/phase6-baseline-checker-red.stderr.txt)
- [重放环境](outputs/phase6-baseline-red.stdout.txt)
- 活动 fixture：`test-support/fixtures/task-20260829-002/phase-7/package-runtime/inventory.json`
- 当次 byte-identical 输入快照：[inputs/inventory.json](inputs/inventory.json)
- 跨阶段活动 Phase 4 inventory 快照：[inputs/phase4-active-inventory.json](inputs/phase4-active-inventory.json)
- 结构化结果：[outputs/verification-summary.json](outputs/verification-summary.json)
- 复测命令：[replay/commands.txt](replay/commands.txt)

真实外部 package 进程、Phase 8 Boa/Sidecar/ESM、Phase 9 registry lifecycle/duplicate state、Phase 10/11
完整 HTTP/Socket pipeline、Phase 12 全量 legacy 删除、Phase 13 built-in/template conversion、Phase 15 UI、
CI、push 与 Release 为 `N/A / NOT_RUN`；这些均不属于 Phase 7 范围。当前本地实现和十门为 GREEN，
最终独立 Reviewer 结论为 `APPROVE`，独立 Verifier 结论为
`VERIFIED / APPROVED / CHECKPOINT READY`；P0=0、P1=0、P2=0、`blockers=[]`。
全部历史 findings、repair、RED/GREEN 与 `NOT_RUN` 边界保留，Phase 7 现可创建 rollback checkpoint。
