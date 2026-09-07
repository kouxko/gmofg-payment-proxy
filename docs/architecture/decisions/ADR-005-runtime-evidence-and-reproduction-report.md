# ADR-005: 持久化运行证据与故障复现报告

- Status: Accepted
- 日期：2026-08-21
- Refined by: [ADR-007](ADR-007-exchange-pipeline-runtime-boundary.md)
- Current MCP transport/security: [ADR-008](ADR-008-mcp-environment-configuration.md)

## Context

`diagnostics_query` 是有界内存中的业务分类事件，抓包回答线路实际输入和写出；两者都不能代替
Rust/Tauri 进程日志。只让 MCP 分别查询这些接口，会遗漏应用启动、底层 I/O、外部软件包连接、
优雅关闭等因果链，也无法一次取得复现所需的入口配置、拓扑、转发方式、规则、包版本和测试输入。

## Decision

桌面进程拥有一个专用、追加写入、容量受限的 JSONL 运行日志存储。`tracing` 与 `log` 记录进入同一
存储，每条记录具有稳定 `log_id`、时间、级别、模块和有界消息。MCP 通过
`application_log_query` / `application_log_get` 只读查询；它不能提交文件路径、打开任意文件或
直接读取 SQLite。

默认采集应用模块的 Debug 及以上记录、第三方模块的 Info 及以上记录。逐次 poll、WebSocket frame 等
第三方 Trace 不进入持久存储，避免固定容量被高频实现细节占满；需要关联的业务阶段、方法、请求 ID、
错误码和连接身份由结构化 diagnostics 与有界内存 Exchange observation 保存，而不是依赖第三方 Trace 推断。

Application 层提供 `diagnostic_report_generate`，聚合精确 Workspace/Listener 的配置、运行态、
规则、协议包、外部服务、Android 网络和结构化诊断。桌面组合根追加最近持久日志，
形成同源 `reproduction_report`：

- `reproduction_report` 返回结构化 bundle、日志页和 Markdown，不写本机文件；
- 桌面“导出复现 Markdown”通过原生保存对话框和原子文件写入显式落盘；
- 报告、单条日志、分页和日志文件均有明确上限，并公开淘汰、损坏、截断和持久化错误状态。

## Boundary

这是 ADR-004 的窄扩展，不是任意文件读取例外。`application_log_*` 与 `reproduction_report` 仍通过
只读 Application facade 和进程创建并持有的 `RuntimeLogStore` 只读句柄取证。该句柄只认识自己的
JSONL 文件，不接受 MCP 提供的路径，也不暴露写入方法；ADR-008 的环境配置工具不复用该句柄或报告
路径执行 mutation。

本决策中的“持久化运行证据”只指专用、容量受限的进程 JSONL 日志。Exchange observation、连接时间线和
运行报文继续存放在有界内存中，不写入 SQLite；配置数据库、内存观察记录和诊断日志具有独立生命周期。
观察队列、投影或 UI 事件发布失败必须 fail-open，不得改变网络交易结果。
当前 `reproduction_report` 不读取 `ExchangeObservationStore`；需要 Exchange 时间线时通过独立只读查询取得。
MCP 对该 store 的只读句柄是 ADR-004 的窄例外：它只查询有界内存记录，不接受路径、不访问 SQLite、
不提供清空或其他写操作。

当前 MCP 的全接口明文、无认证远程风险以 ADR-008 为准。日志可以包含故障复现需要的连接、端点、
模块、错误和关联 ID；容量与结构边界用于可靠性和可诊断性，不宣称提供访问控制。

## Alternatives

- Rejected：让 MCP 读取日志目录或任意路径。这样会扩大权限并破坏稳定分页/保留语义。
- Rejected：只导出结构化 diagnostics。它无法覆盖底层启动、I/O 和进程生命周期。
- Rejected：只返回全量 dump。它无法在 MCP 输出预算内稳定传输，也无法继续分页。
- Rejected：让 `reproduction_report` 直接保存 Markdown。该工具只返回内容；文件写入属于桌面用户
  通过原生对话框触发的显式操作，不能与环境配置 mutation 混用。

## Consequences

故障报告可以直接复制给开发者或 AI，并通过精确 ID 继续查询日志与抓包。代价是桌面进程增加有界
磁盘写入，且只保留容量范围内的历史；报告必须把 `has_more`、保留 ID 范围、
`evicted_count`、损坏行、持久化错误、容量、文件上限、消息截断统计和
`collection_errors` 视为证据缺口，不能将未查询到误写成未发生。

Runtime log producer 的当前进程丢失必须分别公开为 `queue_dropped_full`（条数或共享字节预算）、
`queue_dropped_disconnected`（consumer 已关闭）和 `queue_dropped_contended`（producer 不等待锁）；
这些计数由 `application_log_query` 和 `reproduction_report.application_logs` 返回，并写入 Markdown。
它们不能与已经进入 Store 后发生的 `evicted_count` 混为一个数字。

## Open items

- future 若要把 Exchange observation 合入 reproduction report，必须先定义一致性、容量、淘汰和失败投影，
  并通过新的 ADR 或本 ADR 修订确认；当前保持独立查询。
