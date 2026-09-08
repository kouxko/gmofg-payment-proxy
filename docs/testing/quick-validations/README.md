# 快速配置验证索引

本目录保存证书、URL、Host、Port、TLS/mTLS 和临时 Proxy 配置的观察性验证。快速验证只证明指定
输入、路径、环境和层级在当次执行中的结果，不属于待实现任务、正式验收、发布或业务成功证据。

- 执行规则：[AGENTS.md 快速配置验证](../../../AGENTS.md#105-快速配置验证)
- 记录模板：[TEMPLATE.md](TEMPLATE.md)
- 编号格式：`QV-YYYYMMDD-NNN`
- 档案路径：`<执行日期>/<验证ID>/`

新记录按执行日期倒序排列，同日按 QV 编号倒序排列。生命周期使用 `RESERVED`、`RUNNING`、`FINAL`；
最终结果只使用 `VERIFIED`、`FAILED`、`INCONCLUSIVE`、`NOT_RUN`。生命周期不是 `FINAL` 时最终结果
必须留空，不能用 `NOT_RUN` 表示活动记录。

| 执行时间 | 验证 ID | 目标或用途 | 最高验证层级 | 生命周期 | 最终结果 | 清理结果 | 执行所有者 | 索引更新时间 | 关联任务 | 档案 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-09-08 16:05:34 +08:00 | QV-20260908-001 | Launcher 10.0.34.117:7788 | TCP | FINAL | FAILED | VERIFIED | 01a07fed-750e-7511-8509-fc0762cfbb31/root | 2026-09-08 16:14:28 +08:00 | TASK-20260908-002 | [QV-20260908-001](2026-09-08/QV-20260908-001/README.md) |


常见判定示例：

| 场景 | 最高层结果 | 清理 | 整体结果 |
| --- | --- | --- | --- |
| 直接 TLS 及其必要前置均有直接证据 | VERIFIED | N/A | VERIFIED |
| 下游与上游两个独立握手均成功，但没有同一连接关联 | NOT_RUN | VERIFIED 或 N/A | INCONCLUSIVE |
| 已尝试完整链路，但缺少同一 `path_id` 因果证据 | INCONCLUSIVE | VERIFIED 或 N/A | INCONCLUSIVE |
| 目标层成功，但遗留临时 Listener | VERIFIED | FAILED | FAILED |
| 执行者失活，部分结果和清理无法完整判定 | INCONCLUSIVE | INCONCLUSIVE | INCONCLUSIVE |
