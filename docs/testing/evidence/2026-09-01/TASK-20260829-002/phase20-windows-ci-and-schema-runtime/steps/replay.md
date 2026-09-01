# 复测步骤

## 1. 隔离启动 App

从仓库根目录执行。`RUN_ROOT` 必须是本轮新建目录；不要使用 `open` 或 LaunchServices。

```bash
RUN_ROOT="$(mktemp -d /tmp/gmofg-phase20-runtime.XXXXXX)"
mkdir -p "$RUN_ROOT/home" "$RUN_ROOT/tmp"
APP_BIN="$PWD/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Intercept Proxy.app/Contents/MacOS/intercept-proxy"
env HOME="$RUN_ROOT/home" CFFIXED_USER_HOME="$RUN_ROOT/home" TMPDIR="$RUN_ROOT/tmp" \
  "$APP_BIN" >"$RUN_ROOT/app.log" 2>&1 &
APP_PID=$!
```

确认 `8765`、`17653` 已监听，并用 `lsof -p "$APP_PID"` 确认 SQLite 与运行日志位于
`$RUN_ROOT/home/Library/Application Support/com.interceptproxy.desktop/`。MCP 客户端复用
`scripts/e2e_macos_mounted_release.py` 的 `mcp_call`，协议版本为 `2026-07-28`。

候选配置始终先通过：

1. `workspace_list` 找到当前 selected Workspace；
2. `workspace_get` 读取当前 revision；
3. `environment_candidate_create` 取得 `candidate_id` 与 `confirmation_token`；
4. `environment_candidate_apply`；
5. 轮询 `environment_candidate_status` 至 `committed`。

不得复用历史 Workspace ID、Listener ID 或 revision。当前 MCP 不暴露 `listener_start`；候选提交后，
在这个已经直接启动的 App 窗口中进入“入口配置”并点击“启动监听”。

## 2. 无 Schema：Plain HTTP Body

- Proxy：`127.0.0.1:8080`
- Server：`127.0.0.1:18083`
- Body processing：`plain`
- 条件：`/customer/age`，number equal `18`
- 动作：`record_match`
- 阶段：`proxy_to_upstream`

完整候选见 `../resources/plain-candidate.json`。受控 HTTP Server 读取完整 request body，按 JSON 解析，
把 `{"upstream_received": <request-json>}` 作为 HTTP 200 响应，并把路径、Header、原始 hex 和 UTF-8
文本逐条写入 `plain-server.log`。

依次发送：

```text
{"customer":{"age":18}}
{"customer":{"age":17}}
{
```

验收：

- `18`：HTTP 200；Server 实收 age 18；Capture `matched_rule_ids` 非空；hit count 增加 1。
- `17`：HTTP 200；Server 实收 age 17；Capture `matched_rule_ids=[]`；hit count 不增加。
- 非法 `{`：客户端连接关闭；Server 无第三条请求；Exchange 为 `stage=decode`、`JSON_INVALID`、
  `outcome=failed`，不得透传、重试或回退。

复测后查询并保存：`workspace_get`、`rule_list`、`http_capture_query`、
`exchange_observation_query`、`application_log_query`。

## 3. 带 Schema：ISO8583 Socket 软件包

- 软件包：内建 `iso8583-ascii-standard@1.0.0`
- 来源：`templates/socket-protocol/iso8583-standard/` 与 `scripts/e2e_socket_cases.py`
- Proxy：`127.0.0.1:8081`
- Server：`127.0.0.1:18084`
- Schema 字段：`/message_type` string
- 条件：equal `0200`
- 动作：Set `/message_type` 为 `0220`
- 阶段：`proxy_to_upstream`

完整候选见 `../resources/schema-candidate.json`。受控 Socket Server 必须在 candidate validation 前启动；
它应忽略 validation 产生的空 TCP 探测，然后对两个完整 Frame 原样 echo，并逐 Frame 保存声明长度、
完整 hex 与 message type。

依次发送 `../inputs/schema-match-0200.hex`、`schema-miss-0400.hex`、
`schema-invalid-short.hex`。客户端按前两个长度字节读取一个完整 Frame。

验收：

- `0200`：Decode 文档为 `0200`；规则修改后 Encode/Sent 为 `0220`；Server 和客户端均收到 `0220`。
- `0400`：不命中；Decode、Encode、Server 和客户端均保持 `0400`。
- `0003616263`：客户端 EOF；Server 无第三个完整 Frame；Exchange 为 `stage=decode`、
  `DECODE_FAILED`、`outcome=failed`。
- 本轮有效三请求只增加 1 次 hit count。

首轮环境尝试中，Server 在 validation 空探测后等待 UI 时超时退出；相关文件单独位于
`../outputs/attempt-1-invalid-environment/`，不得计入产品 PASS。有效重放不重建配置或 Listener，
只重启稳定 Server 后立即发送三条 Frame。

## 4. 清理

先在 App 中停止 8080/8081 Listener，再仅终止本轮记录的 `APP_PID`、HTTP Server PID 与 Socket
Server PID；不得使用宽泛 `pkill`。确认本轮 App 的 Sidecar 子进程退出，并检查：

```bash
for port in 8080 8081 18083 18084 8765 17653; do
  lsof -nP -iTCP:"$port" -sTCP:LISTEN
done
```

本次测试进程清理后上述端口均释放。若随后发现新的 App PID，应按 PID、启动时间、二进制路径和
父子关系分类，不得把用户另行启动的实例当作本轮 orphan 终止。

## 5. Windows build-only CI

```bash
gh workflow run windows-release.yml \
  --ref codex/task-20260829-002 \
  -f run_mode=build-only \
  -f platform=windows
```

验收：Verify job 与 macOS job 为 skipped；Windows `build` 在 Tauri 打包前成功执行
`Stage Windows Boa package sidecar`，随后 MSI、NSIS、portable、OpenSSL DLL gate 与 artifact upload
全部成功。手动分支构建不要求 Authenticode 签名，不创建 tag 或 GitHub Release。
