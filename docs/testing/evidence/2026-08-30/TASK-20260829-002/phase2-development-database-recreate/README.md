# phase2-development-database-recreate

- 任务：`TASK-20260829-002`
- 用例：`phase2-development-database-recreate`
- 目的：证明 Phase 2 的临时开发数据库重建只由 Tauri debug composition root 显式启用；Host 默认与 Release Tauri 显式保持 `Preserve`；Infrastructure 在单一异步 open 路径内用一个 `BEGIN IMMEDIATE` 事务删除全部非 SQLite 对象并重建当前 Schema100。
- 执行环境：macOS arm64，Asia/Shanghai；Node `v26.7.0`、pnpm `11.13.1`、rustc `1.97.1`、cargo `1.97.1`。
- 被测状态：分支 `codex/task-20260829-002`，HEAD `0927997175f01428aa5d31bfd8247fb7056b47d7`，HEAD tree `c25d267996d9adcbc50c13681db21b750d9c398a`。工作树包含本任务未提交修改以及不属于本阶段的用户 `docs/README.md` 修改；验证期间被测文件未发生变化。
- 实际资源：`resources/nuvei-tango-json-rhai-1.0.0.zip`，来自活动资源 `examples/protocol-packages/nuvei_tango_rhai/dist/nuvei-tango-json-rhai-1.0.0.zip`，SHA-256 `0595af171e20ae9eee21da42a8327971c99689a278cab6ffd7612ba20a4049ea`，6706 bytes。
- generated bindings：`src/generated/rust-types.ts` SHA-256 `15d730c6afae0f9011bd6539ea98f339342d4e4b22a4751bff595d893815891c`，checkpoint 前后保持一致。

## 预期

1. Host 默认 `Preserve`；Tauri debug 显式选择 `RecreateCurrent`，Tauri Release 显式选择 `Preserve`；`AppState` 与 `ProductProfile` 不参与策略选择。
2. `RecreateCurrent` 不先接受或迁移旧版本：打开连接后关闭 FK、开始 `IMMEDIATE` 事务、删除全部非 `sqlite_%` trigger/view/table、创建当前 Schema100、提交，并在成功或失败后恢复 FK。
3. `<100`、旧 layout 100、当前 Schema100 和已提交 WAL 数据均被重建；注入失败时所有旧对象回滚、FK 恢复、Host 不启动且错误传播。
4. 双启动 fixture 的第一次 Host 仅通过公开 Application/Host 能力写入唯一 Workspace、disabled Listener、Rule 及其 revision/hit_count/last_hit_at/one_shot，并通过真实 ZIP prepare/commit 写入 Package；关闭前逐字段 readback。
5. 第二次显式 `RecreateCurrent` 按唯一 identity 验证上述数据不存在，但不要求表为空，因为当前默认 Workspace 会重建；同一 helper 在默认 `Preserve` 下逐字段验证全部数据保留，供 Phase 17 反转复用。
6. Release checker 只扫描明确枚举的生产 Rust 文件，独立阻断唯一 marker 和实际临时 reset contract；只删除 marker、debug opt-in 或 policy 任一部分都不能假 PASS，只有 Phase 17 删除整个临时合同后才 PASS。
7. Package `tauri:build` 在 Android companion build 与 `tauri build` 前串行执行同一 release checker；Tauri `build.beforeBuildCommand` 也在 `pnpm build` 前执行同一 checker，覆盖通用 `pnpm tauri build` 和直接 Tauri CLI。当前两条入口均必须阻断，`beforeDevCommand`、`tauri:dev` 与普通 Cargo check 不受影响。双 gate 可接受：checker 只读且确定性，package 层避免 Android companion 副作用，配置层封闭所有 Tauri build 入口；若第一层通过，第二次执行只是相同的发布前复核。

## RED / 修正记录

- 初始 RED 把 debug/release 合并到 `cfg!(debug_assertions)`，并把重建放在 Host 默认路径；独立测试预检判定边界错误后立即撤回，未作为 checkpoint。
- 正确分层 RED：`cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --lib sqlite::core::tests::recreate_current -- --nocapture` 编译失败，exit 101，三个 `E0425` 明确指出 `recreate_current_schema` / `recreate_current_schema_with` 尚不存在。
- 初次加入完整 Host fixture 后，`pnpm scan:source-size` 报 `src-tauri/crates/host/src/tests.rs: 696 行`；fixture 拆到专用测试模块后复跑通过，未放宽门禁。
- 独立 Verifier 第一次复验结论为 `FAILED`：原 checker 只计数 marker，删除 marker/日志但保留 `RecreateCurrent` 会假 PASS；reviewer 同时发现 checker 未接入 `tauri:build`、成功日志发生在 tracing subscriber 安装前、absent fixture 只断言 `is_err()`。本轮分别用生产源码 reset-contract 扫描、build 前置 gate、删除无效日志及公开 list/read identity 断言修复；状态回到 `RECHECK PENDING`，未把第一次失败隐藏为成功。

## 实际结果

完整命令见 `replay/commands.txt`，结构化摘要见 `outputs/verification-summary.json`，输入合同见 `inputs/phase2-contract-snapshot.json`。

| 检查 | 实际结果 |
| --- | --- |
| `pnpm test:task-20260829-002:phase2` | PASS；Node 8/8、Infrastructure core 6/6、Host policy 3/3 |
| 受影响 Host/Infrastructure 全目标全特性 | PASS；Host unit 12/12、Host integration/architecture 全绿、Infrastructure unit 651/651 及其 integration tests 全绿 |
| `cargo check --release ... -p intercept-proxy --lib` | PASS；Release composition 编译通过 |
| targeted strict Clippy | PASS；Infrastructure、Host、Tauri all-target/all-feature，`-D warnings` |
| `pnpm check:task-20260829-002:phase2-release-ready` | `NOT_RELEASE_READY`，exit 1；独立报告 1 个 marker 和 32 个临时 reset contract 引用；这是 Phase 2 预期结果，不是 GREEN gate |
| `pnpm tauri:build` | 预期 exit 1；在运行 Android companion build 或 `tauri build` 前由 package 层 checker 阻断 |
| `pnpm tauri build` | 预期 exit 1；Tauri 明确运行 `beforeBuildCommand`，在 `pnpm build` 与打包前由同一 checker 阻断 |
| Phase 1 十门禁 checkpoint | PASS，exit 0；前端 61 files / 531 tests，Rust workspace all-target/all-feature 0 failed |
| `git diff --check` | PASS |

## N/A

- 原始协议 Frame、Decode/Rules/Encode、Server/App 逐字节报文：N/A；本阶段只改变启动数据库策略，不执行代理数据面。
- UI 截图和人工交互：N/A；UI 未修改，Tauri composition 由静态门禁、debug test build 与 Release compile 验证。
- 真实打包 Release App 两次启动：N/A；Phase 2 的临时 marker 明确阻止 Release readiness，真实 Release 持久化验收属于 Phase 17。
- 真实设备和远程服务：N/A；数据库启动合同不依赖这些环境。
- CI、push、Release、部署、提交：N/A；未获授权且未执行。

## 结果

`APPROVE / CHECKPOINT READY`。Verifier 首次 FAILED 及复审新增 build 绕过 P1 后均已修复；最终独立 delta 复审为 `APPROVE`，P0/P1/P2=0。Fresh 证据包含两条 build 入口预期阻断、Node 8/8 与 `git diff --check` PASS，G043 可创建 rollback checkpoint。当前源码按设计仍为 `NOT_RELEASE_READY`，只有 Phase 17 删除 Tauri debug `RecreateCurrent` opt-in、唯一 marker 与完整临时 startup policy/runtime branch，让同一 release checker、package alias 和 Tauri `beforeBuildCommand` 转为 PASS，并复用同一双启动 helper 证明 `Preserve` 后才能解除。
