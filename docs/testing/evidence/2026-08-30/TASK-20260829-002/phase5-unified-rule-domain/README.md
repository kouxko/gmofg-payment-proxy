# phase5-unified-rule-domain

- 任务：`TASK-20260829-002`
- 用例：`phase5-unified-rule-domain`
- 状态：`VERIFIED / APPROVED / CHECKPOINT READY`
- 执行时间：`2026-08-30 08:50:00 +08:00` 至 `2026-08-30 11:57:30 +08:00`
- 父用例：[phase4-package-contract](../phase4-package-contract/README.md)

## 目的与结果

Phase 5 建立唯一统一规则领域模型：非空递归 AND/OR 条件树、严格类型谓词、一个非空有序统一动作列表、Document 严格修改、terminal 顺序与停止语义，以及唯一的 `priority + rule_id` 运行排序。后一规则读取前一规则修改后的 working Document；复制后的规则配置、元数据、条件树、动作和生命周期字段相互独立。

真实 TDD RED 由 Cargo 自动发现 `src-tauri/crates/domain/tests/phase5_unified_rule_domain.rs`，因 Phase 5 领域类型尚不存在而编译失败（exit 101）；不是 0 test、ignore 或随机编译错误。修复后 Phase 5 Rust 9/9、最小 TS 4/4、checker mutation/正控 15/15，Domain all-target/all-feature 119/119。修复覆盖 JavaScript Number 的 `-0 == +0`、Socket 当前无 terminal capability、HTTP 无 Document binding 时递归拒绝 Document work，以及合同未声明硬上限下的 65 层、1025 leaf 和 65 action 构造。

权威 `RuleContent` 已持久化 condition tree 与统一 action list。新保存的消息规则只允许 `ProxyToUpstream`、`ProxyToApp`；旧四阶段 enum/runtime 仅按 Phase 12 边界保留，restore 不增加兼容转换。HTTP 组合停留在应用/运行编排边界，Document mutation 由 Domain 拥有。Phase 6 的整条规则链事务和 lifecycle commit、Phase 10/11 pipeline/codec、Phase 12 删除旧 runtime/enum、Phase 15 完整 editor 均未提前实现。

初始独立 Verifier 结论为 `FAILED`（P0=0/P1=3/P2=1），独立 review 结论为 `REQUEST CHANGES`（P1=6/P2=1）。每项 finding 均先由 focused RED 或 mutation 真实复现，再最小修复；checker 现实际执行 Cargo/Vitest discovery，扫描完整 production comparator helpers、真实 serde/Specta wire owner、generated 完整 golden/SHA 及精确 Phase12 file+symbol+reason allowlist。

修复后第一次完整十门在第 7 门出现既有前端焦点时序失败：`protocol-package-dialog` 删除成功后标题未及时获得焦点，结果 541/542；该完整名用例随后定向连续 3/3 PASS。未修改产品、断言、超时或重试。第二次完整十门 fresh exit 0：Phase1、bindings determinism、architecture、source-size、lint、typecheck、前端 63 files/542 tests、fmt、workspace strict Clippy、Rust workspace all-target/all-feature 全部通过。

最终独立 reviewer 结论为 `APPROVE`，最终独立 verifier 结论为 `VERIFIED / APPROVED / CHECKPOINT READY`；P0=0、P1=0、P2=0，`blockers=[]`。初版 `FAILED` / `REQUEST CHANGES`、全部 findings/repairs、首次焦点 flake 与后续 PASS 均保留。Phase 5 可以创建 rollback checkpoint；`TASK-20260829-002` 总体仍为进行中。

## 可复测资源

- 活动 fixture：`test-support/fixtures/task-20260829-002/phase-5/unified-rule-domain/contract-inventory.json`
- 当次输入快照：[inputs/contract-inventory.json](inputs/contract-inventory.json)
- 结构化结果：[outputs/verification-summary.json](outputs/verification-summary.json)
- checker fresh 输出：[outputs/checker-tests.txt](outputs/checker-tests.txt)
- TypeScript fresh 输出：[outputs/typescript-contract.txt](outputs/typescript-contract.txt)
- review 修复与完整门禁输出摘要：[outputs/repair-verification.txt](outputs/repair-verification.txt)
- 复测命令：[replay/commands.txt](replay/commands.txt)

协议原始报文、Frame、Decode/Encode、Server/App 实际字节与 UI 截图均为 `N/A`：本阶段验证领域合同、即时跨层消费者和静态门禁，不切换 Phase 10/11 pipeline/codec，也不实现 Phase 15 完整 UI。
