# HTTP-CHUNKED-RESPONSE-001

- 任务：`TASK-20260904-005`
- 目的：验证 App 使用 Keep-Alive 时，上游 `404 + Transfer-Encoding: chunked + Connection: close` 的已缓冲响应可合法转发，不产生 `Content-Length + Transfer-Encoding` 冲突或代理 `IO_ERROR`。
- 环境：macOS，Rust 1.98 workspace，Git HEAD `68051ebf` 加本任务未提交变更。
- 执行时间：`2026-09-04 17:34:00 +08:00` 至 `2026-09-04 17:57:38 +08:00`。

## 前置条件与输入

- App 请求：`GET /ex-tms/v1/terminal-status HTTP/1.1`，`Connection: Keep-Alive`。
- 真实本地 TCP 上游响应：`HTTP/1.1 404 Not Found`，`Content-Type: application/json`，`Transfer-Encoding: chunked`，`Connection: close`，并实际发送 `15` 大小块、JSON Body 与结束块。
- 输入快照：[upstream-response.txt](inputs/upstream-response.txt)、[upstream-conflicting-framing.txt](inputs/upstream-conflicting-framing.txt)、[upstream-close-delimited-transfer-coding.txt](inputs/upstream-close-delimited-transfer-coding.txt)。

## 步骤与结果

1. 新增端到端回归测试后、生产修复前运行 focused test。
2. 实际结果：`FAILED`；客户端未收到完整 HTTP response head，复现运行记录中的发送阶段中止。
3. 经审查和用户确认，保留 `Transfer-Encoding: chunked`；响应构造和 Body 替换见到 Transfer-Encoding 时均不添加 `Content-Length`，由 Hyper 重新生成 chunk wire。
4. 重跑 focused test，实际收到 [downstream-response.txt](outputs/downstream-response.txt)，状态 404、chunked 与 Body 原样保留，不含 `Content-Length`，连接回调错误为 `None`。
5. 补充真实 TCP 上游同时携带 TE+CL 的对抗用例，确认转发前只删除冲突 CL，结果见 [downstream-conflicting-framing.txt](outputs/downstream-conflicting-framing.txt)。
6. 补充 `gzip, chunked + CustomStatus` 与 chunked 截断用例，确认只改状态码不丢失复合 Transfer-Encoding，截断后无 `0` 结束块，结果见 [chunked-truncation-wire.txt](outputs/chunked-truncation-wire.txt)。
7. 补充真实 TCP `Transfer-Encoding: gzip` + EOF 定界用例，确认下游 Header 与 Hyper 实际 wire 都使用 `gzip, chunked`，结果见 [downstream-reframed-transfer-coding.txt](outputs/downstream-reframed-transfer-coding.txt)。
8. 运行 Proxy 全量测试、fmt、严格 Clippy、ESLint 和差异检查。

## 命令

见 [commands.txt](replay/commands.txt)。

## 验收

- RED：focused test 在修复前 `FAILED`，失败点为缺少完整 response head。
- GREEN：focused test `1/1 PASS`。
- Proxy：单元 `184/184`、fault `4/4`、raw HTTP `19/19`、reverse `16/16`、TLS/mTLS `6/6`、upstream HTTP `7/7`，合计 `236/236 PASS`。
- 静态：`cargo fmt --check`、Proxy `cargo clippy -D warnings`、`deno task lint`、`git diff --check` 均 PASS。
- 逐字段比较：状态码、Content-Type、Connection、`Transfer-Encoding: chunked` 与 Body 保持；没有 `Content-Length`，下游 wire 包含合法 `15` 数据块与 `0` 结束块。
- 对抗边界：上游 TE+CL 输入只保留 TE；CustomStatus 保留 `gzip, chunked`；TruncateResponse 发送严格前缀后异常结束，不生成终止 chunk。
- 关闭定界 Transfer-Encoding：上游 `gzip` 完整保留，下游追加 `chunked`，canonical Header 与实际 chunk wire 一致。
- 结论：`PASS`。

## 不适用项

- TLS、证书、真实外部服务：`N/A`；真实本地 TCP 上游已覆盖 HTTP wire 读取链，故障不依赖外部服务或 TLS。
- UI 截图：`N/A`；用户截图用于定位，正式验收由协议端到端测试和原始响应文本完成。
- CI、Windows、Android 真机：`N/A`；本用例先证明本地协议修复，后续 GitHub 快速构建另行记录。
