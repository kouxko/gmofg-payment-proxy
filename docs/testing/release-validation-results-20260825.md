# Intercept Proxy App 测试结果（2026-08-25）

## 本次结论

`PASS`，本次 App 测试全部通过。

## 测试用例

| App 测试用例 | 结果 |
| --- | --- |
| release App 启动并进入主界面 | PASS |
| App 内嵌 MCP 启动、列出 37 个工具并执行只读调用 | PASS |
| App 内嵌 MCP 拒绝超出公开 Schema 限制的参数 | PASS |
| HTTP Fixed Server 请求、响应与规则修改 | PASS |
| Socket Direct 双向字节保持与半关闭 | PASS |
| Socket Scripted 四阶段规则处理 | PASS |
| Socket 不完整 Frame 失败关闭且不连接 Server | PASS |
| 抓包列表显示成功和失败 Exchange | PASS |
| Deno 外部协议包真实 Listener 往返 | PASS |
| Deno 断线停止 Listener，重连不自动恢复 | PASS |
| AU EFTEX 外部协议包真实 Listener 往返 | PASS |
| AU EFTEX 断线停止 Listener，重连不自动恢复 | PASS |
