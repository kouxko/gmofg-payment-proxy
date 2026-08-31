# phase14-final-schema100

- 任务：`TASK-20260829-002`
- 用例：`phase14-final-schema100`
- 父用例：`phase13-builtin-zip-replacement`
- 目的：固化产品 1.00 的最终 Schema100，在空库与开发期 `RecreateCurrent` 重建中只创建统一外部软件包注册表；删除旧 `protocol_packages` / `protocol_package_files` 表、查询与测试，同时保留 Phase2 的开发期 reset policy/marker 供 Phase17 删除。
- 被测状态：分支 `codex/task-20260829-002`，基线 HEAD `453474854a169678b5d01191f824395eba9aebe8`；工作树含本阶段修改及不属于本阶段的用户 `docs/README.md` 修改，后者未读取、编辑或纳入验证差异。
- 执行环境：macOS arm64，Asia/Shanghai；Rust/Cargo workspace 与 Node/pnpm 版本沿用当前仓库 checkpoint 环境。

## 合同与实际资源

- Phase2 双启动 fixture：`src-tauri/crates/host/src/tests/phase2_database_startup.rs`。
- ZIP 三个 entry 来源：`test-support/fixtures/task-20260829-002/phase-4/package-contract/` 下 `http-manifest.json`、`protocol.js`、`display.js`；当次实际字节已同名快照到 `resources/`，`cmp` 全部 PASS，其中两个 JS 均为 10 bytes 且以换行结束。测试在内存中生成真实 ZIP，经公开 Application `prepare` / `commit` 写入统一注册表。
- 最终包存储：`external_protocol_packages`，持久化 Manifest、规范化指纹、本地 ZIP、enabled、首次/最近连接、远端地址和最近稳定错误；在线连接与 RPC payload 仍不进入 SQLite。
- Workspace `json` 继续作为递归 Document、统一 RuleDefinition、Listener 与 revision/lifecycle 的聚合持久化边界。

## RED

1. `pnpm test:task-20260829-002:phase14`：exit 1；checker 命中 `schema.rs`、`external_packages.rs`、`environment_configuration_baseline.rs`、`workspaces.rs` 四类旧表/查询残留。
2. `cargo test ... sqlite::sqlite_tests::phase14_schema100`：2/2 FAIL；最终 Schema 仍包含旧表，外部包 enabled 变化未进入 environment package inventory。
3. 受影响全目标第一次为 Infrastructure 501/502；陈旧 listener test 未装配统一 external provider，却期待旧 `PROTOCOL_PACKAGE_NOT_FOUND`。装配真实 `ExternalPackageRegistryAdapter` 后，统一注册表精确返回 `EXTERNAL_PACKAGE_NOT_FOUND`。

## GREEN / 实际结果

| 检查 | 结果 |
| --- | --- |
| Phase14 checker mutation + current source | PASS；Node 3/3，旧表在生产 SQLite 与旧测试中均为零，已删除仍固定 Schema19/旧表/四阶段规则的 `e2e_proxy_rules.py` 与其测试，并由 mutation 锁定不得恢复 |
| Phase14 Cargo | PASS；2/2，最终表集合；inventory 对 registration JSON/fingerprint、local archive、enabled、first/last connection、remote address、recent error 三元组逐项敏感 |
| SQLite affected | PASS；78/78 |
| Phase2 Host 双启动 | PASS；3/3。第一次写入 Workspace、disabled Listener、完整统一 RuleDefinition 和真实本地 ZIP；SQLite readback 逐字段证明 Manifest/identity、32-byte fingerprint、byte-exact local archive、enabled、first/last connection、remote/recent error；Preserve 第二启动验证不可变字段精确保留及稳定错误时间推进，RecreateCurrent 第二启动验证 row 不存在 |
| Host all targets/features | PASS；33/33 |
| Infrastructure all targets/features | PASS；502/502 unit 及全部 integration targets |
| 静态门 | PASS；fmt、workspace strict Clippy、typecheck、lint、architecture、source-size、generated bindings freshness/determinism、`git diff --check` |
| 唯一全仓 checkpoint `session1649` | PASS，exit 0；前端 64 files / 545 tests，Rust workspace all-target/all-feature 全绿 |

`session1649` 的统一执行通道在完成时未保存 filesystem stdout/stderr transcript，因此未伪造“原始日志”；`outputs/session1649-capture-status.json` 保存当时已捕获的命令、exit、门结果与缺失原因。Review repair 按边界没有重跑 full workspace；fresh targeted/affected/static 原始结果摘要见 `outputs/review-repair-targeted.txt`。

## N/A / NOT_RUN

- 原始 HTTP/Socket Frame、Decode/Encode、Server/App 字节：N/A；本阶段只改变持久化 Schema 与基线观察，数据面由 Phase10/11 证据覆盖。
- UI 截图与人工交互：N/A；UI 未修改。
- 真实 macOS Release App/DMG 重启、系统权限弹窗、Windows runner/MSI/NSIS：`NOT_RUN`，分别属于后续 Phase17/18 或需要人工/远程环境；不阻塞本阶段代码工作。
- push、远程 CI、Release、部署：`NOT_RUN`，未授权。
- Phase2 的 `RecreateCurrent` branch、marker 与 release blocker：按 Phase14 边界保留，Phase17 才删除。

## 结果

`VERIFIED / APPROVED / CODE CHECKPOINT READY / FULL CHECKPOINT ARTIFACT PARTIAL`。独立 Reviewer 与 Verifier 均通过，P0/P1/P2=`0/0/0`，`code_checkpoint_ready=true`。唯一 full checkpoint `session1649` 的 exit 0 与门结果保留，但原始 stdout/stderr 未落盘且不可恢复，因此 `full_checkpoint_artifact_complete=false`；该缺口未被伪造或用重跑覆盖。
