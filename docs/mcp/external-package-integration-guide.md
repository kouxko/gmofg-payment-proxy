# 外部软件包接入与 MCP 诊断指南

本指南描述第三方调试进程如何通过 WebSocket 端点为 HTTP Body 或 Socket Listener 提供协议处理，
以及 MCP 客户端如何确认注册、生命周期和失败阶段。本地单文件 Component 由主进程直接调用，不经过
该端点。MCP 查询保持只读；它
不会替调用方启用、停用、删除、重连软件包或启动 Listener。

## 1. 找到权威服务地址

先调用 `external_package_service_status`。只有状态为 `listening` 时才连接返回的 `ws://` 地址；路径
固定为 `/packages`，不能增加 query 或改用其他路径。服务不要求 token、HMAC、mTLS、Origin 或注册
身份；监听范围只由 bind address 决定。

设置页修改 bind address 或 port 后，需要重启 Proxy 才会改变实际服务。只有第三方调试进程主动连接；
它不获得本地 Component 字节、本机路径或主进程 Host capability。

## 2. 发送唯一一次注册通知

WebSocket 建立后，软件包必须先发送无 `id` 的 JSON-RPC 2.0 notification：

```json
{
  "jsonrpc": "2.0",
  "method": "package.register",
  "params": {
    "api": 1,
    "kind": "socket",
    "package": {
      "id": "example.protocol",
      "version": "1.0.0",
      "name": "Example Protocol",
      "description": "Example"
    },
    "document": {
      "upstream": { "schema": { "type": "object", "properties": {} } },
      "downstream": { "schema": { "type": "object", "properties": {} } }
    }
  }
}
```

`params` 就是完整 `manifest.json`，没有额外 `manifest` 包装。注册 wire 使用 closed shape：未知字段、
非法 ID/SemVer、非法递归 Schema 或不符合 `kind` 的方向能力会拒绝整条连接。注册没有成功响应；Proxy
接纳 notification 后才发出带字符串 `id` 的固定 RPC 请求。同一 WebSocket 不得再次注册，也不能
向 Proxy 发业务请求。Ping/Pong 是 WebSocket 心跳，不是 JSON-RPC 方法。

固定方法名如下，不允许 Manifest 自定义后缀：

- `hooks.upstream.frame` / `hooks.downstream.frame`：仅 Socket。
- `hooks.upstream.decode` / `hooks.downstream.decode`。
- `hooks.upstream.encode` / `hooks.downstream.encode`。
- `document.upstream.display` / `document.downstream.display`。

## 3. JSON-RPC 与本地 Component 合同

- Socket `frame.params.buffer` 和 Socket decode/encode wire bytes 使用 canonical padded Base64。
- HTTP decode 的 `params.input` 与 encode 的 `params.originalInput` 使用 Unicode string。
- `decode` 返回递归 JSON Document；值只允许 string、有限 number、boolean、null、object、array。
- `encode` 接收 `originalInput` 和规则处理后的 `document`；HTTP 返回 string，Socket 返回 canonical padded Base64。
- `display` 接收 Document 并返回显示文本；失败只影响观测回退，不改变已成功的业务线路。
- 成功响应复制请求 `id`；失败响应的 `error.data.code` 是稳定 machine code。错误 ID、重复响应、非法
  envelope、缺失稳定 code 或结果类型不匹配会按对应边界失败。

本地包是带唯一顶层 Manifest 的 WebAssembly Component，Socket WIT 直接传递 `list<u8>`，HTTP WIT
传递 Unicode `string`，Document 使用 JSON UTF-8 `string`。主进程提供 WASI Preview 2、WASI HTTP 和
版本化 Host WebSocket；本地 Hook 不使用 JSON-RPC 或 Base64。跨 `/packages` 的远端 Socket wire 仍使用
Base64，第三方进程只实现上述 JSON-RPC wire，不应把某个源码语言或 runtime 当作协议要求。

## 4. 规则事务与过程证据

Proxy 只在两个写出边界执行统一规则：`Proxy -> Server` 和 `Proxy -> App`。HTTP Body 与 Socket 的
每条规则必须且只能包含一个条件和一个对应动作；需要组合逻辑时创建多条规则。命中规则的唯一
action 在一次事务中执行，最后最多 Encode 一次。
Frame、Decode、Rules 或 Encode 失败会终止当前 Exchange；Encode 失败不能提交 hit 或 Document
修改。

`exchange_observation_get` 的时间线可包含：

- `received`：原始 HTTP/Socket context、Decode Document 和 Display。
- `processed`：逐规则 typed `changes`（`record_match`、`set`、`clear`、`insert`、`append`）、
  `changes_truncated` 和 `final_document`。
- `encoded` / `sent`：Encode 后和实际写出的 context。
- `failed`：stage、稳定错误以及可选的 typed `external_package_call`。

`changes_truncated=true` 表示过程操作摘要触及有界证据预算；不能据此断言未列出的规则或动作没有执行。
`final_document` 仍用于判断事务最终值，真实线路结论还必须与 `encoded`、`sent` 和对端实际接收对齐。

## 5. 生命周期与持久化

首次远端注册创建精确 `package.id + version` 的 registry 记录并按合同启用；后续相同身份必须保持
注册指纹一致。本地导入保存完整 Component bytes，并在数据库提交前完成实例化；远端版本要求第三方
进程已经在线。同一精确身份不能同时由本地 Component 与远端连接占有。Listener 只有在精确版本
enabled、online、valid 且能力与数据面匹配时才能
启动。

持久化详情包括 registration、SHA-256 fingerprint、可选 local archive、enabled、首次/最后连接时间、
最后远端地址和最近稳定错误。断线将版本标记 offline 并停止引用它的活动 Listener；重连必须提交
相同精确身份与注册指纹，不会自动重启 Listener。停用在同一 mutation gate 中先停止阻止停用的活动
精确引用，再把 `enabled` 设为 false；删除要求没有任何 Workspace 引用，并关闭仍在线的精确连接。

产品 1.00 的数据库兼容基线是 Schema 100。开发期旧 Schema 可在启动时走明确的 recreate 分支；
Schema 100 及以后不能用该分支清空数据，未知、损坏或更新版本必须 fail closed。不要把开发 reset
当成发布升级或恢复策略。

## 6. 资源与隔离边界

Transport 对 WebSocket handshake、registration、heartbeat 和 message 使用源码定义的独立预算；HTTP
承载入口还应用其 `max_body_bytes`。文档不把这些边界合成为不存在的普通 RPC、in-flight 或 service
status 配置。超大或 malformed transport、错误/重复 ID 会关闭对应包连接；一次业务 `frame` 返回
越界 consumed bytes 或调用失败只终止对应业务连接。一个精确包失败不能停止无关 Listener。

## 7. MCP 排障顺序

1. `external_package_service_status`：确认实际 URL、精确 `/packages`、监听状态和在线数。
2. `protocol_package_detail`：确认精确身份、kind、online/enabled、local process、连接 ID、指纹、首次/
   最后连接、最后远端地址和最近稳定错误。
3. `protocol_package_usage`：确认引用该版本的 Workspace/Listener 及运行状态。
4. `diagnostics_query` / `application_log_query`：按包 ID、连接 ID、method、request ID 或 stable code 对齐。
5. `exchange_observation_query/get`：检查同一 runtime epoch 与 Exchange 的 received、processed、encoded、
   sent、failed、closed 顺序。
6. `diagnose_recent_failures`：取得不执行修改的建议，并以实际缺失条件决定复测入口。

常见分类：

- `websocket_handshake`：地址、精确路径或 transport 不匹配。
- `registration`：notification 带 `id`、Manifest/Schema/身份非法或注册超时。
- `EXTERNAL_PACKAGE_ALREADY_ONLINE`：相同精确版本已有活动或关闭中的连接。
- `PROTOCOL_PACKAGE_IDENTITY_CONFLICT`：相同身份的注册指纹或 archive 不一致。
- RPC failure：按 direction、method、request ID、remote code 和 stable code 对齐第三方日志。
- offline：检查最近稳定错误、进程退出、heartbeat、wire 大小和 transport；不要用盲目重试掩盖合同错误。

诊断控制面只保留有界、类型化的生命周期和错误摘要；完整 payload、Document 与线路字节应在专用
Exchange evidence 或正式测试证据中保存。任何未执行的真实 App、远端、系统权限或人工检查必须明确
记录为 `NOT_RUN`，不能由单元测试或 MCP 查询成功替代。
