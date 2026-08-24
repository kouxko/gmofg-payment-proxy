# 外部软件包接入与 MCP 诊断指南

本指南描述第三方进程如何通过 WebSocket 为 Socket Listener 提供协议处理，以及 AI 如何使用只读 MCP
确认服务、注册、运行和失败阶段。MCP 不会启用、停用、删除、重连或修改软件包配置。

## 1. 找到权威服务地址

先调用 `external_package_service_status`。只有状态为 `listening` 时才连接返回的 `ws://` 地址；路径固定为
`/packages`，不能增加 query 或改用其他路径。服务当前没有认证，同一可达网络范围内的进程都可能尝试连接，
因此应把监听地址限制在受信任环境。

设置页修改监听地址、端口、RPC timeout 或并发额度后，需要重启 Proxy 才会改变实际服务。MCP 是只读接口，
不能代替设置页完成这些操作。

## 2. 完成唯一一次注册

连接建立后，Proxy 首先发送 JSON-RPC 2.0 请求 `package.register`。第三方必须用完全相同的 `id` 返回
`result`，并声明 `api: 1`、精确包身份、上下行 Document Schema、显示方法以及 frame/decode/encode 方法后缀。
注册结果使用严格 closed wire，未知字段、非法 SemVer、重复方法或不完整 Schema 都会拒绝整条连接。

第三方只声明方法后缀。Proxy 生成实际方法名：

- `hooks.upstream.<frame|decode|encode suffix>`：应用到 Server。
- `hooks.downstream.<frame|decode|encode suffix>`：Server 到应用。
- `document.upstream.<display suffix>`：上行 Document 展示。
- `document.downstream.<display suffix>`：下行 Document 展示。

同一 WebSocket 上不得再次注册，也不能发送无关业务请求。Ping/Pong 属于 WebSocket 心跳，不是 JSON-RPC 方法。

## 3. JSON-RPC 处理合同

- `frame` 接收累积缓冲区的 canonical Base64，返回 `need_more` 或正数 `consumed_bytes`。
- `decode` 接收完整 frame，返回严格匹配当前方向 Schema 的 Document。
- Proxy 在 Document 上执行四阶段规则。
- `encode` 接收规则处理后的 Document，返回 canonical Base64 frame。
- `display` 返回未信任 HTML；Proxy 会清洗后展示，失败只回退 Hex，不反写已提交线路。

每个响应必须关联当前连接代次中的请求 ID。错误 ID、重复响应、非法 envelope 或类型不匹配会使软件包协议失效并
断开。普通 JSON-RPC error、调用 timeout 或低于 transport wire 上限的调用级大小错误只失败对应处理调用。

整个连接的单条 JSON-RPC wire message 上限为 1 MiB，display 结果上限为 128 KiB。入站消息超过 1 MiB 时
无法安全取得请求 ID，因此属于连接致命错误，软件包会离线并停止引用它的活动 Listener。

## 4. 在 Proxy 中启用和绑定

首次注册只会创建“在线但停用”的精确版本。用户需要在协议包详情中启用该版本，再在 Socket Listener 中选择同一
`id + version`。目录只把 online、enabled、valid 且具有完整 Socket 能力的版本列为可用。

停用会停止引用入口但保留 WebSocket；删除要求没有任何 Workspace 引用。软件包断线后，Proxy 将其标记为 offline，
并停止引用该精确版本的活动 Listener。重连必须提交相同规范注册指纹，且不会自动启动 Listener。

## 5. MCP 排障顺序

1. 调用 `external_package_service_status`，确认实际 URL、`/packages`、监听状态和在线数。
2. 调用 `protocol_package_detail`，确认精确版本的 source、online、enabled、远端地址、连接 ID、方法映射、RPC timeout、
   指纹和最近稳定错误。
3. 调用 `protocol_package_usage`，确认哪些 Workspace/Listener 引用了该版本。
4. 调用 `diagnostics_query`，用包 ID、连接 ID、`external package`、`外部软件包`、方法名或稳定错误码筛选。
5. 调用 `diagnose_recent_failures` 获取不执行修改的建议。
6. 对已产生业务流量的连接，使用 `exchange_observation_query` 按 Workspace、Listener 查询，再用
   `exchange_observation_get` 查看同一连接按顺序追加的收到、发送、失败与关闭证据。

生命周期日志只包含阶段、包身份、连接 ID、远端地址、稳定错误码和安全消息。业务报文、注册原文、密码、密钥与远端
JSON-RPC `data` 值不会写入 diagnostics；`data` 只允许记录 `string(bytes=N)`、`array(items=N)`、
`object(fields=N)` 等形状摘要。

## 6. 常见判断

- `service_failed`：检查端口占用、监听地址和重启边界。
- `websocket_handshake`：确认使用权威 URL、精确 `/packages` 且没有 query。
- `registration`：核对 API、SemVer、Schema、方法后缀和 JSON-RPC response ID。
- `EXTERNAL_PACKAGE_ALREADY_ONLINE`：相同精确版本已有活动或关闭中的连接。
- `PROTOCOL_PACKAGE_IDENTITY_CONFLICT`：内部或外部来源已占用相同 `id + version`，或者重连注册指纹改变。
- RPC `frame/decode/encode/display`：按日志中的 direction、method、request ID 和 connection ID 对齐第三方进程日志。
- offline：查看最近稳定错误，再检查第三方进程退出、心跳、wire 大小和 transport 状态；不要盲目自动重试。

第三方进程自己的日志也应只记录请求 ID、方法、耗时、结果分类和字节计数，不应默认记录完整 ISO8583、密码或密钥。
