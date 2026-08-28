# RUNNING-APP-REPLAY-001：当前运行 App 的 Proxy 与模拟 Server 重放

## 结论

- 总结：`FAILED_WITH_NOT_RUN`
- 数据面：HTTP 与 Socket `PASS`
- 诊断日志倒序：`PASS`
- 空 Workspace 规则入口能力：`PASS`
- HTTP 服务器响应生成 Mock 草稿：`FAIL`
- Workspace/入口运行态 UI 刷新：`FAIL`
- TLS/mTLS、外部协议包、Android 真机：`NOT_RUN`
- 清理：`PASS`

## 环境与前置条件

- 执行时间：`2026-08-28 09:40:55 +08:00` 至 `2026-08-28 10:08:23 +08:00`
- 被测对象：用户已启动的 macOS Release `Intercept Proxy.app`
- MCP：`http://127.0.0.1:17653/mcp`，协议版本 `2026-07-28`
- 当前 Workspace：`默认 Workspace`；开始时 0 个 Listener、0 条 HTTP 规则、0 条协议规则、0 个 Android 网络方案。
- 父用例：`TASK-20260827-003 / FINAL-REPLAY-001`
- 父证据：`docs/testing/evidence/2026-08-27/TASK-20260827-003/FINAL-REPLAY-001/`

## 实际步骤

1. 读取当前 Workspace、入口运行状态和外部包服务状态。
2. 因当前 Workspace 为空，通过 MCP 环境候选临时加入一个 HTTP 固定 Server Listener 和一个透明 Socket Relay Listener。
3. 在 App UI 中分别启动两个 Listener。
4. 启动本地临时 HTTP 模拟 Server 和 TCP Echo 模拟 Server。
5. 通过真实 Proxy Listener 发送 HTTP POST 和原始 Socket 字节。
6. 对比客户端收到的数据、模拟 Server 实收数据、HTTP 抓包与 Exchange 时间线。
7. 在 App 实时抓包界面打开本轮 HTTP Exchange，点击“用此服务器响应创建 Mock 草稿”。
8. 停止两个 Listener，通过 MCP 恢复空 Workspace，并重新读取 Workspace 与入口状态。
9. 在诊断日志页面确认事件按时间倒序显示；在空 Workspace 打开新建规则菜单确认四类入口均禁用并显示原因。

## PASS 结果

### HTTP Proxy

- 请求：`POST /replay/http?case=running-app-replay-20260828`
- 请求正文：`{"case":"running-app-replay-20260828","amount":1234}`
- 模拟 Server 实际收到 method、path、标记 Header 和正文，均与输入一致。
- App 客户端收到 `201 Created`、`X-Mock-Server: running-app-replay-20260828` 和模拟 Server JSON 响应。
- HTTP 抓包保存 request、response、terminal 三个阶段，terminal 状态为 201。

### Socket Proxy

- 输入字节见 `inputs/socket-request.hex`。
- TCP Echo Server 实收字节与输入一致。
- App 客户端实收 `ECHO:` 加原始输入，逐字节与 `outputs/socket-response.hex` 一致。
- Exchange 时间线记录 opened、upstream received/sent、downstream received/sent、closed completed。

### 日志和规则能力

- 诊断日志首行是 `10:05:15.731` 的停止事件，后续依次为 `10:04:44.628`、`10:02:53.675` 等更早事件，证明最新事件在最上方。
- 空 Workspace 的 HTTP、Body、Socket 和故障预设创建入口全部禁用，并分别显示缺少兼容 Listener 的原因。

### 生命周期与清理

- 运行中直接替换 Workspace 被稳定错误 `RUNTIME_ACTIVE` 拒绝，未发生持久化提交。
- 停止两个 Listener 后，恢复候选依次达到 `preview_ready`、`apply_queued`、`committed`。
- 恢复后同一 Workspace 名称保持不变，Listener、HTTP 规则、协议规则、Android 网络方案和运行入口数量全部为 0。
- 本轮 HTTP/TCP 模拟 Server 均已关闭。

## FAIL 结果

### 1. HTTP 服务器响应无法生成 Mock 草稿

- UI 操作：在真实 HTTP Exchange 的 `Server → Proxy` 响应事件点击“用此服务器响应创建 Mock 草稿”。
- 实际：App 跳转到规则页，但没有创建草稿，并提示“抓包响应生成的规则草稿校验失败：该 Header 由 Rust 转发管线统一管理，规则不得直接设置”。
- 当前已验证根因：Application 的响应投影会删除原 `content-length` 后重新写入计算值；Domain 规则校验又禁止规则直接设置 `content-length`，所以含正文的正常服务器响应在最终草稿校验阶段被拒绝。
- 影响：已完成实现声明中的主要成功路径在真实 App 中不可用。

### 2. Workspace/入口变化未及时刷新 UI

- 临时配置提交后，停留在代理入口页时仍显示“当前工作区还没有代理监听”；切换到 Workspace 页再返回后才显示两个 Listener。
- 恢复空 Workspace 后，停留在代理入口页时仍显示两个临时 Listener；切换页面后列表才清空。
- 即使列表已经刷新为空，顶栏仍显示“全部入口已停止 入口 2 · 活动 0”。
- 影响：通过 MCP、导入或其他外部路径改变当前 Workspace 后，UI 展示会与权威 Workspace 不一致。

## NOT_RUN

- TLS/mTLS：当前空 Workspace 没有可复用的 TLS Listener 与对应材料；本轮不导入或替换证书。
- 外部协议包：服务监听正常但没有在线外部包连接；本轮不安装或切换协议包。
- Android 真机：当前没有已授权测试设备；不修改真实设备。
- 远端业务交易：禁止发送，本轮只使用本机模拟 Server。

## 复测入口

```bash
python3 docs/testing/evidence/2026-08-28/TASK-20260828-001/RUNNING-APP-REPLAY-001/replay/run_running_app_replay.py
```

脚本提交临时配置后，需要在 120 秒内通过 App UI 启动两个 Listener；看到
`READY_FOR_LISTENER_STOP` 后，在 240 秒内停止两个 Listener，脚本才会恢复空 Workspace 并退出。
