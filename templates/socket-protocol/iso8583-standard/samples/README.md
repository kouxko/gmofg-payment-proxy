# Samples

本目录保存协议作者的说明和测试向量，不是 Rhai 入口，也不会被 Host API v1 自动执行。

`financial-request.json` 同时提供：

- `tcp_chunks_hex`：同一 Frame 被拆成多个 TCP 读取片段。
- `complete_frame_hex`：`decode(origin, context)` 实际收到的完整 Frame。
- `expected_document`：Schema 字段和值。
- `expected_encode`：Document 未修改时的回编码预期。
- `expected_display`：展示结果类型。

通用格式和兼容性规则见 [Host API Samples](../../API.md#12-samples)。
