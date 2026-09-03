# HTTP LocalServer 与文本 Mock 验收

## 目的

验证 HTTP Listener 可选择进程内 LocalServer，LocalServer 不创建真实上游连接且请求、响应继续经过
Proxy → Server 与 Proxy → App Pipeline；同时验证 MockResponse 公共 body 为文本、响应 BodyCodec
编码、旧配置兼容、MCP schema、规则名称状态和 Path 选择器防溢出。

## 环境与被测状态

- 时间：2026-09-03 15:01:40 +08:00
- 分支：`codex/intercept-proxy-generalization`
- 基线提交：`77e26874a0ea6fbde6f1b850921a548e03299dec`
- macOS arm64；Rust 1.98.0；Cargo 1.98.0；Deno 2.9.6；Node 26.8.1
- 测试期间暂停修改被测源码；未连接远端 `10.0.28.99`，未触发 CI、发布或推送。

## 输入、步骤与结果

1. 使用 [request.txt](inputs/request.txt) 中记录的精确 Rust byte literal，向随机本机端口启动的 HTTP
   LocalServer Listener 发送请求。
2. 测试 Pipeline 在 Proxy → Server 记录一次请求，在 Proxy → App 返回
   [D48-response.json](outputs/D48-response.json) Mock。
3. 断言响应为 HTTP 200、Body 逐字节以 D48 JSON 结尾，请求与响应策略计数均为 1；测试代码没有
   创建或调用真实 `UpstreamConnector`。
4. 验证 `MockResponse.body` 序列化为字符串，旧 UTF-8 `body_bytes` 可读取，非文本原始故障动作仍保持
   bytes；请求阶段文本 Mock 按响应 Shift-JIS codec 编码。
5. 验证 HTTP Local/Remote topology 往返、旧顶层 `fixed_server` 迁移，以及新旧字段同时出现时拒绝。
6. 验证 MCP 完整候选与 schema snapshot；验证 UI LocalServer 切换、规则名称保持、
   `Path（包含 Query 参数）` 文案及选择器单行截断类。

完整命令与结果见 [test-summary.txt](outputs/test-summary.txt)。全部验收 PASS。

## 比较结果

- 预期/实际 HTTP Body：`{"ErrorCode":"D48"}` / `{"ErrorCode":"D48"}`，PASS。
- Proxy → Server / Proxy → App 次数：`1 / 1`，PASS。
- 真实上游连接：预期 `0`；LocalServer connector 为进程内 Echo，PASS。
- Mock 公共类型：预期 `body: string`；生成 TypeScript 与 MCP schema 均为 string，PASS。
- Path 语义：底层使用 URI `path_and_query()`，包含 Query；UI 文案与截断断言 PASS。
- 规则名称：选择 Listener、阶段、条件和动作来源后仍为原输入，PASS。

## N/A

- 远端服务日志、TLS 证书、真实设备：本用例只验证本机 LocalServer 与公共合同，不连接远端。
- UI 截图：本次使用 jsdom 组件回归直接断言文本、状态和 CSS 类；未启动打包桌面 App。
- 清理脚本：测试使用随机端口并由 runtime stop/测试进程清理，无外部持久服务。
