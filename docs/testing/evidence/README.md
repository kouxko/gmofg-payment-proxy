# 测试证据索引

本文按执行日期倒序记录可复用测试证据及其派生关系。原证据目录保持不可变；新需求通过父任务 ID、
父用例 ID 和父证据稳定路径建立关系。

## 2026-08-26

| 任务 ID | 用例 ID | 用途 | 状态 | 父任务 | 父用例 | 父证据 | 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TASK-20260826-004 | NTR-RHAI-002 | 在真实 Proxy 导入/启用 Rhai 包并完成三组 Nuvei Listener 双向 Exchange | NOT_RUN | TASK-20260826-003 | NUVEI-PKG-003 | [父证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-003/README.md) | [证据](2026-08-26/TASK-20260826-004/NTR-RHAI-002/README.md) |
| TASK-20260826-004 | NTR-RHAI-001 | 验证 Nuvei Tango Rhai 包的 Python parity、原文 Display、只读失败路径、已知字节数和确定性 ZIP | PASS | TASK-20260826-003 | NUVEI-PKG-003 | [父证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-003/README.md) | [证据](2026-08-26/TASK-20260826-004/NTR-RHAI-001/README.md) |
| TASK-20260826-003 | NUVEI-PKG-003 | 验证 external Document int wire 修复及真实上下行 split/decode/display/encode 逐字节保持 | PASS | TASK-20260826-003 | NUVEI-PKG-002 | [父证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-002/README.md) | [证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-003/README.md) |
| TASK-20260826-003 | NUVEI-PKG-002 | 验证 Python 外部包连接与 RPC 结构化诊断日志不泄露报文或字段内容 | PASS | TASK-20260826-003 | NUVEI-PKG-001 | [父证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-001/README.md) | [证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-002/README.md) |
| TASK-20260826-003 | NUVEI-PKG-001 | 验证 Nuvei Tango 长度前缀 JSON 的只读 Python 外部包、掩码和逐字节保持 | PASS | 无 | 无 | 无 | [证据](2026-08-26/TASK-20260826-003/NUVEI-PKG-001/README.md) |
| TASK-20260826-002 | DOC-GOV-006 | 验证快速配置验证分流、生命周期、分层结论、清理门禁和正式任务升级合同 | PASS | 无 | 无 | 无 | [证据](2026-08-26/TASK-20260826-002/DOC-GOV-006/README.md) |
| TASK-20260826-001 | DOC-GOV-005 | 验证需求就绪、根因分析、高低优先级、风险分级测试和锁目录规则 | PASS | 无 | 无 | 无 | [证据](2026-08-26/TASK-20260826-001/DOC-GOV-005/README.md) |
| TASK-20260825-006 | MCP-CONFIG-CONTRACT-001 | 验证环境配置 v1 DTO、严格 Schema、公共 literal、fixture、终态合同和 active MCP 隔离 | PASS | 无 | 无 | 无 | [证据](2026-08-26/TASK-20260825-006/MCP-CONFIG-CONTRACT-001/README.md) |

## 2026-08-25

| 任务 ID | 用例 ID | 用途 | 状态 | 父任务 | 父用例 | 父证据 | 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TASK-20260825-007 | DOC-GOV-004 | 验证单工作区全局任务锁、所有权恢复链和任务索引一致性 | PASS | 无 | 无 | 无 | [证据](2026-08-25/TASK-20260825-007/DOC-GOV-004/README.md) |
| TASK-20260825-003 | DOC-GOV-003 | 验证测试资源归档和跨任务复用规范 | PASS | TASK-20260825-001 | DOC-GOV-001 | [父证据](2026-08-25/TASK-20260825-001/DOC-GOV-001/README.md) | [证据](2026-08-25/TASK-20260825-003/DOC-GOV-003/README.md) |
| TASK-20260825-002 | DOC-GOV-002 | 验证小任务对抗审查改为可选、整体审查保持强制 | PASS | TASK-20260825-001 | DOC-GOV-001 | [父证据](2026-08-25/TASK-20260825-001/DOC-GOV-001/README.md) | [证据](2026-08-25/TASK-20260825-002/DOC-GOV-002/README.md) |
| TASK-20260825-001 | DOC-GOV-001 | 验证项目 AGENTS 治理规范 | PASS | 无 | 无 | 无 | [证据](2026-08-25/TASK-20260825-001/DOC-GOV-001/README.md) |
