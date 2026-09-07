# NUVEI-PKG-002：Nuvei Tango 外部包安全诊断日志

- 任务：`TASK-20260826-003`
- 派生自：`TASK-20260826-003 / NUVEI-PKG-001`
- 执行时间：2026-08-26 14:45:00 +08:00
- 结果：PASS

## 目的

证明 Python 外部包能够记录足以定位连接和 RPC 阶段问题的结构化元数据，同时不泄露 Base64、完整
报文、JSON 内容、字段名或敏感字段值。

## 输入与预期

- 合成 frame 和合成非法 frame，不联网、不使用真实支付数据。
- 正常拆帧日志必须包含方法、方向、阶段、输入字节数、`complete`、消费字节数和耗时。
- 失败日志必须包含方向、输入字节数、JSON-RPC `-32002` 和稳定码 `DECODE_FAILED`。
- 序列化后的全部日志不得包含 frame Base64、合成敏感值或 `json_preview` 字段名。

## 实际结果

- TDD RED：新增测试因找不到 `rpc_started` 事件产生预期失败。
- GREEN：13/13 包测试 PASS；日志成功覆盖正常与错误路径。
- `compileall` PASS。
- 安全日志样例见 `outputs/safe-log-sample.jsonl`。

## 复测与不适用

- 复测命令见 `steps/replay.md`。
- 真实报文和真实后台：N/A，日志测试只使用合成输入。
- 对抗审查：N/A，用户明确该任务无需对抗审查。
- CI：N/A，未获授权触发。
