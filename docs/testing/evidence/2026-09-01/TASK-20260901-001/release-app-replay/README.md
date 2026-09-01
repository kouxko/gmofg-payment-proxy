# Release App HTTP、Socket 与 Wasm 重放

- 任务：`TASK-20260901-001`
- 执行时间：`2026-09-01 22:35:00 +08:00` 至 `2026-09-01 22:52:00 +08:00`
- 被测程序：`src-tauri/target/release/bundle/macos/Intercept Proxy.app`
- 主程序 SHA-256：`5ad8f36424a515bf8dc1eece3d07979966826ba85528f56326bd8ca2459b6298`
- 运行进程：PID `78045`；测试结束后保持运行，监听 `8765`、`17653`、`8080`、`8081`、`8082`、`8083`。

## HTTP

使用独立 `Live HTTP Replay` Listener，将流量从 `127.0.0.1:8080` 送往受控 Server `127.0.0.1:18083`。

- Method `POST` 命中后设置 `X-Method-Hit`：PASS。
- Header `/x-probe = alpha` 命中后设置 `X-Header-Hit`：PASS。
- Request target wildcard `/orders/*?mode=test` 命中后设置 `X-Path-Hit`：PASS。
- Plain Body RFC 6901 `/customer/age = 18` 命中后把 `/customer/name` 设置为 `matched`：PASS。
- miss 请求不执行动作：PASS。
- 非法 JSON `{` 在 Decode 阶段 fail-closed，客户端断开且 Server 未收到该请求：PASS。

实际输入在 `inputs/http/`，客户端、Server、规则和 Exchange 输出在 `outputs/http/`。

## Socket 内置 Schema

使用独立 `Live Socket Replay` Listener，将流量从 `127.0.0.1:8082` 送往受控 Server `127.0.0.1:18085`。

- `/message_type = 0200` 命中后设置为 `0100`，Server 收到修改后帧：PASS。
- `0400` miss 帧逐字节保持：PASS。
- 非法帧 `0003616263` fail-closed，客户端 EOF，Server 未收到第三帧：PASS。

实际输入在 `inputs/socket/`，客户端、Server、规则和 Exchange 输出在 `outputs/socket/`。

## AU EFTEX Wasm

当前 App 中 `au-eftex@1.1.0` 已导入、在线且通过静态校验，但 App 启动环境没有配置
`AU_EFTEX_BDK_FILE` 或 `AU_EFTEX_BDK_HEX`。71 字节公开旧向量在
`hooks.upstream.frame` 以 `PROTOCOL_PACKAGE_INVALID` fail-closed，Server 收到 0 字节；错误原文为
`configure exactly one of AU_EFTEX_BDK_FILE or AU_EFTEX_BDK_HEX`。该结果是运行配置缺失，不记为
Wasm 算法回归 PASS。

同一源码的 Rust Component 测试 6/6 PASS，项目 venv 中公开旧向量校验 2/2 PASS；真实 App 数据面
仍需在带唯一 BDK 配置的 App 启动环境中重放。实际诊断在 `outputs/au-eftex/`。

## 清理与不适用项

- 受控 Server `18083`、`18085`、`18086` 已停止。
- App 和四个 Listener 按用户要求保持运行，未清理其数据库或规则。
- 文件系统和出站 HTTP Host capability 测试：`N/A`，用户明确要求本轮不执行。
- Windows：`N/A`，由后续 Windows-only GitHub Actions 构建验证。

复测脚本保存在 `replay/`；重放前需要重新启动对应受控 Server，并确保 Listener/规则配置与本目录
输入一致。
