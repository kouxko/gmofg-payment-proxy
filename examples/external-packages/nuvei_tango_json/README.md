# Nuvei Tango JSON External Package

`nuvei-tango-json@1.0.1` 同时提供两种等价入口：

- `nuvei_tango_json/`：保留的 Python WebSocket 外部调试实现，适合快速修改和观察 JSON-RPC 日志。
- `component/`：Rust 实现的单文件 WebAssembly Component，供 Proxy 在同一进程内直接加载。

两种实现都只读地拆分并解析 Nuvei Tango 观察到的长度前缀 JSON 报文，不允许修改或重新生成业务内容。
Component `1.0.1` 将脱敏后的 JSON preview 递归渲染为嵌套 HTML table，不再输出原始 `<pre>` 文本。

## 线路合同

```text
4-byte unsigned big-endian body length
+ 4-byte opaque control header
+ 8-byte ASCII decimal sequence
+ one UTF-8 JSON object with exactly one top-level message key
```

长度值只计算其后的 body，不包含自身 4 字节。最大 body 为 `1 MiB - 4 bytes`。拆包支持 TCP 分段和
粘包；非法长度、非数字序号、非法 UTF-8、重复 JSON key、`NaN/Infinity`、非对象或多个顶层消息均
fail-closed。

## 只读边界

- Decode 只输出 frame 长度、不透明控制头、序号、消息类型和经过掩码的 JSON 预览。
- PAN、Track1/2、PIN、MAC、KSN、Key 和 cryptogram 类字段递归替换为 `[redacted]`。
- 完整原始 frame 只保存在进程内的有界上下文中，不放入 Document、日志或测试 fixture。
- Encoding context 绑定上下行方向并带 HMAC；篡改、跨方向复用或包进程重启后使用都会失败。
- Encode 仅在所有 Document 字段保持不变时返回原始 frame；新增、删除或修改任意字段均拒绝。
- 本包不解释控制头内部语义，不验证支付业务、密钥或 MAC。

## 安装与测试

### Python 外部调试实现

要求 Python 3.11 或更高版本：

```bash
cd examples/external-packages/nuvei_tango_json
python3 -m venv .venv
.venv/bin/python -m pip install --upgrade pip
.venv/bin/python -m pip install -e .
.venv/bin/python -m unittest discover -s tests -v
```

Windows 使用 `.venv\\Scripts\\python.exe`。

### WebAssembly Component

要求安装 Rust 的 `wasm32-wasip2` target：

```bash
rustup target add wasm32-wasip2
cargo test --locked --manifest-path component/Cargo.toml
pnpm build:protocol-packages
```

构建产物为：

```text
dist/protocol-package-components/intercept-proxy-nuvei-tango-json-component.wasm
```

直接执行 `cargo build --target wasm32-wasip2` 得到的是尚未追加顶层 Manifest 的编译器原始产物，
不能替代统一构建生成的可导入文件。

该 Component 内嵌 `intercept-proxy:manifest`，直接导出 Socket WIT 的上下行
`frame/decode/encode/display` 函数，不连接本地 `/packages` WebSocket。`encoding_context` 仍由实例内随机
HMAC key 认证、绑定方向并有界保存；WIT 传入的 `original-input` 还必须与上下文中的原始 frame
逐字节一致。组件实例重建后，旧 context 与 Python 进程重启后的行为一样失效。

## 启动与绑定

以下启动方式只适用于 Python 外部调试实现。它默认连接本机 Proxy：

```bash
.venv/bin/nuvei-tango-json
```

环境变量：

- `EXTERNAL_PACKAGE_URL`：默认 `ws://127.0.0.1:8765/packages`，路径必须是 `/packages`。
- `RECONNECT_DELAY_SECONDS`：默认 `1`。
- `NUVEI_TANGO_ALLOW_INSECURE_REMOTE_WS=1`：仅在隔离测试中允许连接远端明文 `ws`；生产使用 `wss`
  或受控网络。

包注册成功后，在 Proxy 的“协议包”页面启用 `nuvei-tango-json@1.0.1`，并绑定到 Nuvei Socket
Listener。Listener 保持 `relay`、上游 `tangodev.nuvei.com:9081` 和 TLS→TLS。外部包只负责协议拆帧、
只读解析和显示，不改变 Listener 的连接、TLS 或读取超时配置。

## 诊断日志

软件包向 stdout 输出单行 JSON，不记录 Base64、完整 frame、JSON 内容、字段名或字段值。主要事件：

- `connection_attempt`、`connected`、`disconnected`、`connection_error`：连接生命周期和异常类型。
- `protocol_error`：非法 WebSocket JSON-RPC wire；只记录异常类型。
- `rpc_started`：方法、上下行方向、frame/decode/encode/display 阶段、输入字节数或 Document 字段数。
- `rpc_completed`：结果、耗时、拆帧状态、消费/输出字节数、JSON-RPC 错误码和稳定处理错误码。
- `wire_response_rejected`：响应超过 1 MiB，连接关闭。

安全日志示例：

```json
{"timestamp":"2026-08-26T06:50:00Z","level":"info","event":"rpc_completed","method":"hooks.upstream.split_frame","direction":"upstream","operation":"frame","input_bytes":647,"outcome":"ok","frame_status":"complete","consumed_bytes":647,"duration_ms":1}
```

Windows 保存日志供排查：

```bat
.venv\Scripts\nuvei-tango-json.exe >> nuvei-tango-json.jsonl 2>&1
```

日志文件轮转和保留由启动环境负责；软件包本身不创建隐藏日志文件。排查时提供对应时间段的 JSONL
即可，不需要提供真实报文。

## 安全测试说明

自动化测试只使用合成的非支付 JSON。真实授权报文可能产生重复交易，也包含 PAN、Track2、密钥和
MAC，因此不得复制到源码、fixture、测试日志或网络重放脚本。
