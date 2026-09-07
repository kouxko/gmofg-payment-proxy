# ADR-004: 本机只读 MCP 信任边界

- Status: Superseded by [ADR-008](ADR-008-mcp-environment-configuration.md)
- 日期：2026-08-19
- Refined by: [ADR-007](ADR-007-exchange-pipeline-runtime-boundary.md)

> 历史范围说明：本文记录的是已经被 ADR-008 替代的原始 loopback-only、read-only 决策，不再描述
> 当前运行时。G036 已把五个环境配置工具接入生产 catalog/dispatch，并把明文 MCP 改为全接口
> IPv4/IPv6 监听；当前合同、风险和生命周期以 [ADR-008](ADR-008-mcp-environment-configuration.md)
> 为准。以下决策、备选方案和后果保留为历史记录。

## 决策

应用默认启动一个嵌入式、无认证、只读 MCP 服务，唯一地址为
`http://127.0.0.1:17653/mcp`。实现使用官方 Rust SDK `rmcp 3.1.3`，只声明并支持
MCP `2026-07-28`，使用无会话 Streamable HTTP。

MCP 以共享的 `Arc<Application>` 只读 facade 为主，并持有 `RuntimeLogStore` 与
`ExchangeObservationStore` 两个进程内窄只读句柄。它不能直接打开 SQLite、读取任意文件，
也不提供保存、导入、导出、清空、启停或其他写操作。已安装协议包的源码和 ZIP 不在
Application 只读模型中，因此 MCP 只能读取其 Manifest 投影、能力、Schema、引用关系，
以及应用内置官方模板 ZIP。

两个窄句柄只允许有界查询自身拥有的日志或内存观察记录，不接受文件路径、不暴露网络/数据库能力，
也不把 Exchange observation 并入 reproduction report。

## 用户接受的本机信任边界

本服务不提供认证，也不承诺隐私保护。用户明确接受以下边界：同一电脑上任何能够连接
`127.0.0.1:17653` 的进程，都可能读取 MCP 暴露的 Workspace 配置、运行状态、诊断、抓包、
会话、规则、Android 信息和公开证书元数据。loopback 绑定只阻止远程主机直接连接，不能
隔离本机进程。

证书工具当前只返回 Application 已公开的证书元数据，不返回私钥、密码或原始密钥库内容。
这是当前类型化 API 的能力边界，不是认证或隐私保证。产品允许后续把更多完整诊断数据加入只读 MCP，
但只有相应 Application DTO、资源预算和测试落地后才能记为已实现能力。

## 可用性与资源边界

- 端口被占用或绑定失败时，MCP 记录不可用，代理主应用继续启动。
- 全局同时接受最多 16 个连接和 32 个请求。
- HTTP 请求、工具参数、工具/资源结果和最终 HTTP 响应都有明确字节预算。
- 每个请求和工具调用都有 deadline。
- 应用退出时取消 MCP，并等待有界时间；超时后中止剩余 MCP 任务。
- `application_snapshot` 由 Application 在 mutation gate 内编排一次完整投影。Workspace
  摘要与详情来自仓储的单次聚合读取，协议包引用计数复用相同 Workspace 与 Listener
  运行态观察，不再执行 N+1 或双全量读取；结果附带稳定 generation 指纹和观察时间。

## 协议依据

- 官方 Rust SDK：<https://github.com/modelcontextprotocol/rust-sdk>
- MCP 2026-07-28 Streamable HTTP：
  <https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/transports/streamable-http.mdx>
- MCP 2026-07-28 发布说明：<https://blog.modelcontextprotocol.io/posts/2026-07-28/>

## Alternatives

- Rejected：允许 MCP 直接访问 SQLite 或任意文件。这样会绕过 Application 的只读业务边界。
- Rejected：在当前仅绑定 loopback 的产品约束中引入认证和权限配置。用户已明确接受本机信任边界，
  额外凭据只会增加部署与排障复杂度。
- Rejected：提供写操作。当前目标是读取现场并给出建议，任何应用变更仍由用户在应用界面完成。

## 后果

优点是 AI 可以直接读取足够完整的现场证据并给出确定性操作建议，且不会修改应用。
代价是本机恶意进程可读取同样的数据；如未来需要跨机器监听或不受信任本机用户隔离，
必须另立 ADR，引入认证、授权、审计和数据分级，不能复用当前信任假设。

## Open items

- 如 future 版本支持非 loopback 监听，必须先完成新的安全设计与 ADR；当前实现不预留远程监听兜底。
