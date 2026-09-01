# Phase 21 HTTP / Socket 单条件单动作实际链路

## 目的

验证当前 arm64 Release App 中规则编辑器和生产链路的当前合同：每条规则只有一个条件和一个动作；HTTP 覆盖 Method、Header、request target/path+query wildcard、Plain JSON Body；Socket 覆盖带 Schema 的 ISO 8583 Document 条件和动作。

## 被测对象

- App：`src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Intercept Proxy.app`
- 主进程：PID 84166
- 协议包 sidecar：由主进程启动的 `intercept-proxy-package-sidecar`
- MCP：`127.0.0.1:17653`
- HTTP Listener：`127.0.0.1:8080`，受控 Server：`127.0.0.1:18083`
- Socket Listener：`127.0.0.1:8081`，受控 Server：`127.0.0.1:18084`

## HTTP 输入与预期

四条规则分别为：

1. Method `POST` -> `SetHeader X-Method-Hit=yes`
2. Header `/x-probe` equals `alpha` -> `SetHeader X-Header-Hit=yes`
3. Request target wildcard `/orders/*?mode=test` -> `SetHeader X-Path-Hit=yes`
4. Document `/customer/age` number equals `18` -> set `/customer/name` to `matched`

执行 6 个有效请求和 1 个非法 JSON 请求。有效请求分别覆盖单项命中、全部命中和全部未命中；非法 JSON 必须在 Decode 阶段关闭连接且不得到达 Server。

结果：PASS。`outputs/http-summary.json` 为 `valid_cases=6`、`invalid_fail_closed=true`；`outputs/http-server-success-round.jsonl` 正好 6 条 Server 实收。首次复跑因旧日志累计为 12 条而被测试脚本拒绝，该轮保存在 `outputs/http-server-prior-rounds.jsonl`，不计产品结论。

## Socket Schema 输入与预期

- 包：`iso8583-ascii-standard@1.0.0`
- 条件：`/message_type` string equals `0200`
- 动作：set `/message_type` to string `0100`
- match：0200 应重编码为 0100，并保留前导零
- miss：0400 应逐字节保持不变
- invalid：`0003616263` 必须 Decode fail-closed，不到达 Server

结果：PASS。`outputs/socket-summary.json` 为 `match_rewritten_to=0100`、`miss_unchanged=true`、`invalid_fail_closed=true`；`outputs/socket-server.jsonl` 正好包含 0100、0400 两个有效 Frame。

## UI 验证

- HTTP 与 Socket 使用同一套单条件、单动作编辑器。
- 创建规则只有一个 `启用规则` Switch，不再有启用 Select、单次或第 N 次命中。
- 已有 Header 规则能反解 `/x-probe`、`equals`、`alpha` 和 Set Header JSON。
- HTTP 动作来源与动作类型同一行；动作参数 JSON 独占下一行。
- 最终界面：`outputs/final-http-rule-editor.png`。

## 回放

```bash
python3 replay/http_matrix.py server
python3 replay/http_matrix.py prepare
# 在 App 中启动 8080 Listener
python3 replay/http_matrix.py client

python3 replay/socket_matrix.py server
python3 replay/socket_matrix.py prepare
# 在 App 中启动 8081 Listener
python3 replay/socket_matrix.py client
```

执行前应清空或换名当轮的 `outputs/http-server.jsonl` / `outputs/socket-server.jsonl`，避免把历史轮次计入当前断言。

## 清理与最终状态

- 8080、8081、18083、18084 均已释放。
- App PID 84166、MCP 17653、package sidecar/8765 保持运行。
- App 留在 HTTP Header 规则编辑页供人工检查，未关闭。
