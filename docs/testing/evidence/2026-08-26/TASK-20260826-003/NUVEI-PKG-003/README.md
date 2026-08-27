# NUVEI-PKG-003：外部 Document wire 修复与真实双向 Exchange

- 任务：`TASK-20260826-003`
- 派生自：`TASK-20260826-003 / NUVEI-PKG-002`
- 执行时间：2026-08-26 15:21:59 至 15:36:32 +08:00
- 结果：PASS

## 目的

证明 Python 包返回的 `int` Document value 符合 Proxy 的 canonical decimal string 合同，并在
`ws://10.0.28.85:8765/packages` 真实链路完成上行请求和下行响应的拆帧、只读解析、展示和逐字节编码。

## 根因与修复

- 故障报告记录 `Fatal(InvalidResponse)`，随后外部包断开且 Listener 因包离线停止。
- Python 包原先把 `frame_length` 返回为 `{"type":"int","value":1598}`。
- Proxy 权威 external Document wire 要求 `int.value` 是 i64 canonical decimal string，即
  `{"type":"int","value":"1598"}`。
- 修复仅改变 Python 包 `frame_length` 的 wire 表达，没有修改 Proxy、Listener 或后台配置。

## 自动化结果

- TDD RED：新增合同测试准确失败，实际值 `245`、期望值 `"245"`。
- GREEN：14/14 Python 测试 PASS。
- `compileall` PASS。

## 真实链路结果

修复后的包于 2026-08-26 15:27:45 +08:00 重新注册成功。首个完整 Exchange：

- 上行：1602 B；`split_frame` complete/1602，decode 6 字段，display 成功，encode 输出 1602 B。
- 下行：647 B；`split_frame` complete/647，decode 6 字段，display 成功，encode 输出 647 B。
- 随后的 1602/914 B 和 1322/896 B 双向 Exchange 同样全部成功。
- 修复后日志未出现 `InvalidResponse`、`DECODE_FAILED`、包断连或 Listener 因包离线停止。

安全元数据日志见 `outputs/live-exchange-log.jsonl`；不包含 Base64、完整报文、JSON 字段或支付数据。

## 资源与复测

- 故障输入摘要：`inputs/invalid-response-summary.json`。
- 自动化输出：`outputs/tdd-red.txt`、`outputs/tests-and-static.txt`。
- 真实链路日志：`outputs/live-exchange-log.jsonl`。
- 复测命令和判定：`steps/replay.md`。

用户提供的原始报告包含真实支付报文，因此没有复制进仓库。证据只保存复现故障所需的错误码、阶段、
wire 差异、字节数和安全运行日志。

## 不适用项

- Proxy Rust/Tauri/前端修改：N/A，本次未修改。
- CI：N/A，未获授权触发外部 CI。
- 对抗审查：N/A，用户明确该低优先级小任务无需执行。
