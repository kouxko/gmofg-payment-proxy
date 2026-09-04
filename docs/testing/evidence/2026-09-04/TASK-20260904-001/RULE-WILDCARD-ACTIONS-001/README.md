# RULE-WILDCARD-ACTIONS-001

- 任务：`TASK-20260904-001`
- 目的：验证 Document 条件可使用的单层 `*` 同样适用于 Set、Clear、Insert、Append 动作，并验证规则页动作下拉与本地安装 App。
- 环境：macOS 27.0（26A5425a）、arm64、Rust 1.98.0、Deno 2.9.6。
- 被测对象：当前工作区源码、生成绑定以及 `/Applications/Intercept Proxy.app`。
- 执行时间：`2026-09-04 11:11:16 +08:00` 至 `2026-09-04 11:42:06 +08:00`。
- 结果：`PASS_WITH_KNOWN_UNRELATED_BASELINE_FAILURES`。

## 输入与预期

- `/items/*/state` Set：修改动作前存在的全部 `state` 节点，不创建缺失字段。
- `/items/*` Clear：删除动作前存在的全部数组元素，不受下标移动影响。
- `/groups/*/items` Insert/Append：对每个命中的数组执行；任一具体动作失败时不保留部分修改。
- Schema item-template：`/GBRD_01/*/aid` 等带 `*` 路径应提供 Set/Clear；数组模板目标还应提供 Insert/Append。
- 精确路径：序列化仍为原字符串 wire 形状，保持已有规则兼容。

## 执行步骤与结果

1. 先运行新增 Domain 测试，修改前因 `DocumentMutation` 仅接受 `JsonPointer` 出现 8 个类型错误，确认 RED。
2. Domain 全量：70 个库测试、15 个 Phase 5 测试、6 个 Phase 6 测试、9 个统一规则合同测试，合计 100/100 PASS；其中新增 5 个 wildcard/schema/atomicity 测试 PASS。
3. Application 定向：Schema 能力 6/6、Document factory 4/4 PASS；全量 414/416，其余 2 项在当前 HEAD 已可独立复现，分别是 response action 既有数量断言 12/实际 13，以及既有 ProxyToApp MockResponse 预期拒绝/生产能力允许，不由本次 Document 变更引起。
4. Infrastructure HTTP Pipeline 15/15 PASS；外部 Relay 定向 4/4 PASS；workspace all-targets `cargo check` 与 strict Clippy PASS。
5. 前端规则定向 15/15、完整 Vitest 64 文件 551/551、UI 合同 30 文件 303/303、typecheck、lint、架构扫描、生成绑定确定性、Rust fmt、diff check均 PASS。
6. 源码尺寸门禁仍仅被 7 个当前 HEAD 已有超限文件阻断；本次触及的 `models/unified_rule.rs` 为 494 行，未新增超限文件。
7. `deno task tauri build --bundles app` PASS，Next 生成 13 个静态页面；bundle 重新 ad-hoc 签名并严格校验通过。
8. 旧安装移至 `/Users/codin/.Trash/Intercept Proxy.app.codex-old-20260904-114006`，新 App 安装到 `/Applications/Intercept Proxy.app`；bundle id `com.interceptproxy.desktop`、版本 `1.0.0`、可执行文件 SHA-256 `482084cc5c0515c213e17fe65d344e625722cf8af15baafde72922ab3e09a483`、PID `23649`。
9. 实际 App 中选择 Payment DLL / Proxy → App / 执行动作 / Document 后，Schema 下拉显示 `/GBRD_01/*/aid`；选中后 Document 动作下拉显示 `set`、`clear`，可选中 `set`，并显示“`*` 仅展开一层；动作会应用到当前命中的全部节点”。未保存测试规则，未改变 Workspace 规则数据。

## 不适用项

- 真实业务报文与外部 Server：N/A；Domain/HTTP Pipeline 已覆盖动作执行，本次没有用户授权发送真实交易。
- Windows、Android、CI、push、发布：N/A；不在本次本地实现与安装范围。
- UI 截图文件：N/A；实际 App 的可访问性树完整记录了路径、动作选项、选中值和提示，未另外生成截图文件。

复测命令和 UI 状态摘要见 `outputs/`。
