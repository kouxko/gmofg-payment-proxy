# DOC-GOV-003：测试资源归档与跨任务复用规范验证

## 测试信息

- 任务 ID：`TASK-20260825-003`
- 用例 ID：`DOC-GOV-003`
- 执行时间：2026-08-25 17:07:48 +08:00
- 结果：PASS

## 派生关系

- `derived_from.task_id`：`TASK-20260825-001`
- `derived_from.case_id`：`DOC-GOV-001`
- `derived_from.evidence`：`2026-08-25/TASK-20260825-001/DOC-GOV-001/README.md`

## 资源清单

| 资源 | 来源 | 用途 | 必需 | 路径 |
| --- | --- | --- | --- | --- |
| 用户确认的归档要求 | 当前会话 | 验证规则覆盖范围 | 是 | `resources/requirements.md` |
| 当前 AGENTS 规则 | 当前工作区 | 被测活动文档 | 是 | 仓库根目录 `AGENTS.md` |
| 复测步骤 | 本任务 | 后续重复验证 | 是 | `steps/replay.md` |
| 原始复测输出 | 本任务 | 保存复测命令的实际输出 | 是 | `outputs/replay-command-output.txt` |
| 人工验收摘要 | 本任务 | 人工核对实际结果 | 是 | `outputs/actual-rules.txt` |

## 验收结果

- 文件型测试资源必须进入证据归档：PASS。
- 不可复制外部依赖必须记录可重建信息：PASS。
- 活动 fixture 与不可变证据快照职责清楚：PASS。
- 派生任务使用父任务、父用例和父证据复合引用：PASS。
- 兼容修改与替代旧合同采用不同复测方式：PASS。
- 整体独立对抗审查：APPROVE，P0/P1/P2 为 0/0/0。

## 不适用项

- 网络报文：N/A，本任务不执行网络功能。
- 证书文件：N/A，本用例验证证书等资源的归档规则，不进行证书握手。
- UI 截图：N/A，本任务不改变 UI。
