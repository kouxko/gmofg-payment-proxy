# MCP-CONFIG-CONTRACT-001：环境配置合同、DTO 与回归锁

## 测试信息

- 任务 ID：`TASK-20260825-006`
- 用例 ID：`MCP-CONFIG-CONTRACT-001`
- 对应 Goal：`G033-task-006-contract-dto-and-regression`
- 状态：`PASS`
- 证据归档创建时间：2026-08-26 08:23:59 +08:00

## 目的和范围

证明 G033 已建立严格的 `EnvironmentConfigurationCandidateV1` DTO、五个环境配置 MCP 工具合同与
JSON Schema、闭合公共字面量、显式终态结果，以及有效、无效、Document 和 Schema 漂移回归锁。
本用例只证明合同层，不证明候选生命周期、持久化提交、远程 MCP 传输或完整环境应用。

完整 G033 白名单见 `inputs/task-related-files.txt`。它包含 Application DTO、MCP 合同、测试、
ADR-008、ADR-004 的 G033 supersession 说明和七个测试实际使用的 fixture。

## 实际测试资源

七个活动 fixture 已逐字节快照到
`resources/active-fixtures/environment_configuration_candidate_v1/`。来源和用途见
`resources/fixture-manifest.json`；归档副本与活动 fixture 内容一致。

## RED 阶段

测试工程师先建立回归锁，实际得到预期失败：

- negative：4 PASS / 1 FAIL，嵌套 unknown field 未被拒绝；
- document：6 PASS / 1 FAIL，终态 unknown field 未被拒绝；
- schema：4 PASS / 3 FAIL，发布 Schema 与闭合 literal 合同不完整。

RED 的精确执行时间和完整 stdout 未单独保留；`outputs/red-phase-summary.txt` 只记录主 Agent
收集到的实际计数与原因，不补造日志。

## GREEN、门禁与审查

主 Agent 最终复验汇总：

- Application：211 + 14 + 7 + 5 + 12，合计 249/249 PASS；
- MCP：31/31 PASS；
- strict Clippy、Rust fmt、architecture scan、source-size scan：全部 PASS。

主 Agent 精确执行时间未单独保留。归档过程中再次执行 Application 全套和 MCP 测试，结果一致；
完成结果在 2026-08-26 08:21:56 +08:00 捕获，见
`outputs/archive-replay-validation.txt`。

第三轮独立审查者 `/root/g033_contract_red_complete` 对最新实现给出 `APPROVE`，P0/P1/P2
为 0/0/0；fresh focused Application negative 5/5 PASS，MCP schema 12/12 PASS。精确审查时间未单独
保留，见 `outputs/adversarial-review-round-3.txt`。

预期、实际和逐项比较见 `inputs/expected-results.json`、`actual.json` 和
`comparison.json`；全部适用检查一致。

## N/A

- 网络、TCP chunks、Frame、Decode、Encode、Server/App 业务报文：N/A；G033 未接入传输或业务 payload。
- UI、可访问性、截图：N/A；G033 不修改 UI。
- 数据库、Schema 迁移、持久化：N/A；属于后续故事。
- 真实设备、Android、外部包连接：N/A；本用例未运行这些边界。
- CI、Push、发布、制品：N/A；未获授权且未触发。
- 清理：无长期进程、数据库或外部资源；见 `steps/cleanup.md`。

## 复测

从包含 G033 变更的 fresh checkout 仓库根目录执行 `replay/replay.md`。
