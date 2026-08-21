# ISO 8583 Deno External Package

这是一个零第三方依赖的 Deno + TypeScript 示例进程。它作为 WebSocket JSON-RPC peer 连接 Intercept
Proxy 的 `/packages` 服务，为 Socket listener 提供 ISO 8583 报文分帧、解析、重建和安全展示。

它是一个明确受限的接入 Profile，不是“完整 ISO 8583 标准实现”。生产接入必须用收单机构、交换网络或厂商
规范核对字段长度、字符集、二进制域、MAC 和密钥处理。

## 运行

要求 Deno 2.x：

```bash
deno task check
deno task start
```

默认连接 `ws://127.0.0.1:8765/packages`。可通过环境变量配置：

```bash
EXTERNAL_PACKAGE_URL=ws://127.0.0.1:8765/packages \
RECONNECT_DELAY_MS=1000 \
deno task start
```

在 Proxy 中首次注册后，外部软件包默认为停用。需要在“协议包”页面启用它，再将精确版本
`iso8583-deno-ascii@1.0.0` 绑定到 Socket listener。软件包断线重连只恢复在线状态，不会自动重启
listener。

## 与 Proxy 的合同

- Proxy 建立连接后主动且仅调用一次 `package.register`；本进程不会主动发送注册消息。
- 支持上下行相同方法：
  - `hooks.<direction>.split_frame`
  - `hooks.<direction>.decode_iso8583`
  - `hooks.<direction>.encode_iso8583`
  - `document.<direction>.render_message`
- JSON-RPC 响应严格复用请求的 string/number `id`，未知方法返回标准 error。
- Deno 原生 WebSocket 栈处理 Ping/Pong；业务处理不实现自定义心跳消息。
- 断线后进程按配置延迟重连。Proxy 不会自动重启已经停止的 Socket listener。
- 单条 JSON-RPC wire message 最大 1 MiB；display HTML 最大 128 KiB。
- LocalResponder 未配置响应规则时，Proxy 传入空的下行 Document；示例会生成最小 `0210`、DE39=`00`
  响应。响应规则只要明确写入 `message_type`，示例就完全按规则产生的 Document 编码。

## ISO 8583 Profile

- Frame：2 字节大端 payload 长度头，长度不包含头本身；一帧总长度不超过 65,535 字节。
- Message：4 字节 ASCII MTI、8 字节二进制主位图、可选 8 字节二进制第二位图。
- 字符集：文本和长度前缀仅支持 ASCII。
- 变量字段：支持 ASCII 两位 LLVAR、三位 LLLVAR；二进制字段按字节计长。
- DE4 在 Document 中使用 canonical i64 十进制字符串；其他数字域使用 string 保留前导零。
- 主要字段：DE2/3/4/7/11-14/18/22/23/25/32/35/37-39/41-43/49/52-55/60-64、
  DE70/90/100/102/103/128。精确清单与长度见 `src/iso8583.ts` 的 `FIELD_SPECS`。
- 设置位图中未实现字段会明确失败，不会猜测长度或透明转发。
- 不支持第三位图、BCD MTI/字段、EBCDIC、自定义 TPDU、网络特有复合子域、加密、DUKPT、PIN block
  解释、MAC 计算或验证。DE52/55/64/128 仅作为不透明 blob 保存并重建。

`frame` 使用 Proxy 提供的完整方向累积缓冲，因此可以处理长度头半包、payload
半包和粘包；一次只返回首帧的 `consumed_bytes`，剩余字节由 Proxy 继续处理。

## 安全与日志

外部软件包服务当前没有身份认证；只应监听在受信网络。需要远程连接时应由部署环境提供网络隔离或安全隧道。

日志是单行 JSON，只记录以下诊断元数据：连接尝试、连接/断线、JSON-RPC ID、方法、耗时和结果类别。
实现不会记录 frame、Document 字段值、PAN、PIN、ICC、MAC、密钥或 JSON-RPC body。Proxy 侧可用相同的
软件包身份、业务连接、方向、方法和请求 ID 关联 MCP 只读诊断。

## 故障排查

- 一直无法注册：确认 Proxy 设置页显示的实际地址和端口，并确认路径严格为 `/packages`。
- 注册后不能选择：首次注册默认停用，需要手动启用；入口只列出“在线 + 启用”的精确版本。
- listener 不能恢复：软件包重连不会自动重启入口，需要在 Proxy 中重新检查并启动。
- `DE<n> is not supported`：对端位图使用了本示例未声明字段；依据对端正式 Profile 增加 FieldSpec
  和测试。
- `length header does not match`：对端可能使用不同的长度头语义、TPDU 或包含/排除头长度约定。
- `message has trailing bytes`：字段定义或某个 LLVAR/LLLVAR 长度与对端 Profile 不一致。
- `response exceeds 1 MiB`：缩小报文/Document；当前 Proxy WebSocket transport
  的单消息上限不可动态放大。

测试覆盖注册一次性、完整方法名、相关 ID、半包、粘包、decode/encode roundtrip、HTML 转义、Ping/Pong
职责边界和断线重连。执行 `deno task check` 同时运行格式、lint 和测试门禁。
