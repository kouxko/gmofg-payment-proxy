# 运行时观测与诊断

本文说明 Exchange 结构化观测、普通运行日志、UI 实时刷新、MCP 查询/环境配置和复现报告之间的关系。
核心原则是：观测系统可以丢证据，但不能阻塞或篡改交易；业务 Pipeline 失败则必须明确失败。

## 1. 责任与证据通道

```text
业务代码 / Exchange
  └─ tracing subscriber（进程唯一）
       ├─ 普通 tracing -> 有界队列 -> RuntimeLogStore -> JSONL + MCP/复现报告
       └─ exchange::ui -> 有界队列 -> ExchangeObservationStore -> 抓包 UI/MCP
```

普通日志适合错误、状态转换、资源和生命周期诊断。ExchangeObservation 保存可逆的 HTTP 文本或
Socket 字节、Document、Display 及实际发送内容。两者不能互相替代：格式化日志不应被反向解析成
协议报文，Exchange 证据也不承担全局系统日志职责。

应用没有业务 payload 的隐私过滤要求。HTTP Body、Socket bytes、解码 Document、Display 和复现所需
完整测试证据都允许进入各自的有界观测通道；容量控制通过显式条数/字节上限、淘汰和丢弃计数实现，
不通过脱敏或把 payload 改成摘要实现。密码、私钥等凭据仍不属于业务 payload，也不进入这些通道。

| 证据通道 | 所有者 | 数据与保留 | 读取者 | 不承担的职责 |
| --- | --- | --- | --- | --- |
| Exchange observation | Infrastructure `ExchangeObservationStore`，Tauri tracing consumer 写入 | 完整 HTTP Header/Body、Socket bytes、Document、Display 和连接事件；进程内、受共享 `CapacityLedger` 字节上限约束 | Capture UI、MCP `exchange_observation_*` | 不持久化、不替代普通日志 |
| HTTP capture/session | Application session/capture store | 完整 HTTP 请求/响应与规则结果；进程内、受 session/event 共享容量约束 | Capture UI、MCP `http_capture_*` | 不记录 Socket、不作为系统日志 |
| Runtime log | Tauri `RuntimeLogStore` | Rust/Tauri 格式化日志；20,000 条且 JSONL 最多 32 MiB，约 75% 低水位批量滚动 | MCP `application_log_*`、复现报告 | 不作为可逆 wire 报文来源；当前 UI 展示的是 Structured diagnostics |
| Structured diagnostics | Application `EventHub` | Listener、Android、TLS、Socket、规则和外部包的类型化阶段/结果；随 EventHub 有界回放 | UI、MCP `diagnostics_query` | 不复制 Exchange/HTTP payload；这是职责隔离，不是隐私过滤 |
| Reproduction report | Application report + Tauri composer | 精确 Workspace/Listener 快照、diagnostics、最近 200 条 runtime logs；Markdown 最多 256 Ki 字符 | 原生导出、MCP `reproduction_report` | 不隐式聚合 Exchange observation 或 HTTP capture |
| MCP projection | Tauri MCP backend | 34 个查询工具只读投影现有 store/Application；五个环境配置工具调用 Application 候选用例；预算按工具类别独立限制 | 任意网络可达 MCP 客户端 | 不创建第二份观测存储，不直接访问 SQLite/保护器/任意文件 |

## 2. ExchangeObservation 模型

### 2.1 一条记录对应一条 App 连接

一个 accepted App connection 创建一个 `ExchangeObservationRecord`，稳定身份包括：

- `exchange_id`；
- Workspace、Listener、runtime epoch；
- 对端地址；
- HTTP 或 Socket 协议；
- 按实际发生顺序追加的事件列表；
- 是否发生证据淘汰。

记录只存在于有界内存，不写入 SQLite。连接关闭不会覆盖之前的报文，而是在同一记录末尾追加
`Closed`。长连接中的 D2 不会覆盖 D1。

### 2.2 事件类型

| 事件 | 含义 | 主要内容 |
| --- | --- | --- |
| `Opened` | Exchange 已建立 | 时间 |
| `Received` | Reader 完成一个消息 | 方向、HTTP/Socket Context、可选 Document、固定 Display |
| `Sent` | Writer 已提交消息 | 方向、实际写入 Context |
| `Failed` | 某阶段失败 | 可选方向、阶段、可选 Context、错误 |
| `Closed` | 连接结束 | completed/failed 与可选错误 |

HTTP Context 保留 Header 和 Body 文本；Socket Context 保留字节。透明 Socket 没有 Document 或
Display。协议模式的 Received 展示 Reader 时的事实，Sent 展示 Rules/Encode 后实际发送的事实。
HTTP Context 额外记录 `body_is_utf8`。当它为 `false` 时，Body 文本只是 lossy 观测投影，Application
不得用它生成会写回网络的 Mock 规则草稿。

## 3. tracing 到结构化事件

Exchange 在连接 span 中记录 primitive fields。专用 `ExchangeUiLayer` 只读取字符串、整数、布尔、
字节等明确字段，刻意忽略 `record_debug`，防止把 Debug 文本误认为可逆报文。

处理顺序是：

1. `opened` 提供完整连接元数据并创建记录；
2. 后续事件按 `exchange_id`、协议和可选 runtime epoch 校验归属；
3. 成功追加后发布 `ExchangeObservationChanged`；
4. UI 当前位于抓包页面时重新查询同一内存仓储；
5. MCP 也读取同一个 `Arc<ExchangeObservationStore>`，因此 UI 与 MCP 看到同一证据来源。

缺少 opened、字段解析失败、身份不匹配或重复 opened 时不会猜测归属，只增加 ignored 计数。

## 4. 有界容量和 fail-open

tracing callback 不直接做持久化或 UI IPC，而是尝试写入有界非阻塞队列。队列条数和逻辑字节预算
任一耗尽时，当前观测会被丢弃并增加原子计数；业务线程不等待消费者。

`ExchangeObservationStore` 使用全局 `CapacityLedger`：

- 新记录需要容量时优先淘汰最旧连接；
- 当前记录追加超预算时撤销该事件，并标记 `evidence_evicted`；
- 被整体淘汰的连接按 Workspace 计数；
- producer 队列入口丢弃通过 `dropped_events` 暴露；字段解析、缺少 opened、身份不匹配和 Store
  容量拒绝通过 `ignored_events` 暴露，两者不合并；
- 清空 Workspace 证据时同时释放容量账本。

观测队列还有独立的一槽 loss notification 通道，使 UI 即使拿不到丢失 payload，也能收到“证据有
变化/损失”的刷新信号。

生产配置的队列条数 `N` 来自 `settings.capacity.ui_event_capacity`，默认 4,096、最大 65,536；普通
runtime log 队列与 Exchange observation 队列各自有 `N` 个槽位。两者共享一个字节预算 `B`，其值为
进程 `max_memory_bytes / 4`；任一队列使总预留从 `B` 变成 `B+1` 时立即丢弃当前消息。loss notification
使用独立的 1 槽、64 B 控制通道，不被 payload 压力占用。队列满、字节满、consumer 断开和 producer
锁竞争都不等待业务线程。

## 5. 业务失败与观测失败

### 5.1 业务 Pipeline：fail-closed

以下阶段失败会返回稳定错误并结束当前 Exchange：

```text
transport read -> Frame -> Decode -> Rules -> Encode -> transport write
```

Server 在请求后断开且没有回复，同样是 Exchange 失败。系统不会构造空回复、切换协议包或静默改为
透明转发。

### 5.2 观测：fail-open

以下失败不能把成功交易改成失败：

- Display 入口失败：HTTP 回退 Body，Socket 回退 Hex；
- tracing 队列已满；
- 结构化字段无法解析；
- UI 事件发布失败；
- 内存证据被淘汰；
- Exchange 已成功后连接 shutdown 的附加诊断失败。

观测 fail-open 不等于隐藏问题。所有可计数损失必须通过 dropped/ignored/evicted 或日志持久化状态
暴露，排障结论必须注明证据是否完整。

## 6. 普通运行日志与滚动文件

`RuntimeLogStore` 同时维护有界内存索引和 JSONL 文件：

- 每条日志有单调 `log_id`、时间、级别、target、message 和截断标记；
- message 最多 65,536 字符，target 也有独立上限；
- 查询支持级别、target、关键字、时间范围和游标；
- 返回最旧/最新保留 ID、淘汰数、坏行数、持久化错误、路径和容量；
- 启动时读取现有 JSONL，坏行或重复 ID 被计数并触发规范重写；
- 条数或字节超限时批量回落到约 75% 低水位，避免每条日志都全量重写；
- 写文件失败不让业务调用失败，而是保留 `persistence_error` 和 dirty 状态供下次重试。

生产保留上限固定为 20,000 条和 32 MiB。`application_log_query` 额外公开
`queue_dropped_full`（条数或共享字节预算不足）、`queue_dropped_disconnected`（consumer 已关闭）和
`queue_dropped_contended`（producer 不等待 sender 锁）三个进程累计计数；它们与
`evicted_count`（已经进入 Store 后被保留策略淘汰）含义不同。查询默认 200 条、最大 500 条。

Structured diagnostics 复用 `EventHub` 的条数与共享字节保留合同。查询返回
`oldest_retained_event_id`；当 `after_event_id` 与最早保留事件之间存在缺口时返回
`snapshot_required=true`。该判断基于 EventHub 全局事件窗口，因此可能保守地要求刷新，但不会把不连续
历史伪装成完整结果。

日志桥接有独立队列和过滤策略。高频依赖噪声不会无条件进入持久化文件；结构化 Exchange UI 事件
由专用 Layer 消费，避免同一大报文再被普通 formatter 复制一份。

## 7. UI 实时刷新

Rust `EventHub` 发布稳定的 UI 事件，前端 query hook 收到失效信号后重新读取 Rust ViewModel。
页面不直接持有第二份业务状态，也不通过定时器推断 Exchange 是否完成。

抓包页面展示时应保持以下关系：

```text
同一 Exchange 行
  -> 详情中的 Opened
  -> 按顺序追加 Received / Sent / Failed
  -> 最后一条 Closed
```

切换页面后重新查询只是恢复视图，不应是看到新数据的必要条件。相关回归测试需要证明事件到达时当前
列表自动更新，同时暂停列表滚动只影响视图，不影响网络、规则或内存记录。

HTTP 响应生成 Body 替换草稿时，Tauri 只把 Exchange ID 与事件索引交给 Application。Application 使用
同一 `ExchangeObservationRecord` 配对实际送往 Server 的 request-target 和 Server 返回的 UTF-8 Body，
生成 Proxy → App `ReplaceBodyText` 规则；status/Header 不进入草稿。该规则需配合 LocalHttpServer，
前端不解析原始 HTTP，也不会自动保存或启用生成的草稿。

## 8. MCP 查询与环境配置

MCP 使用官方 `rmcp`，当前协议版本为 `2026-07-28`。服务以明文 Streamable HTTP 监听
`0.0.0.0:17653`，并在平台支持时监听 `[::]:17653`。IPv4 绑定失败会终止桌面启动；IPv4 成功后，
IPv6 独立绑定、双栈覆盖、不支持和其他绑定失败分别通过 capability/warning 如实公开。

服务不验证 Host、Origin、Authorization、API key、Cookie、来源 IP 或 CIDR，也不提供 TLS、认证或
授权。任何能够连接端口的主机都可以读取公开数据并调用环境配置工具；网络观察者可能看到明文提交的
私钥、密码和 confirmation token。这是接受的高风险远程写边界，不是安全远程管理能力。

34 个既有工具继续只读，主要诊断工具包括：

- `application_snapshot`：一次 generation 校验后的应用快照；
- `application_log_query/get`：普通运行日志；
- `exchange_observation_query/get`：与 UI 相同的 Exchange 内存记录；
- `diagnostics_query`、`diagnose_recent_failures`：结构化故障与确定性建议；
- `reproduction_report`：按 Workspace/Listener 组合有界 Markdown 报告；
- Listener、规则、协议包、证书和 Android 状态的只读查询。

五个环境配置工具是 `mcp_environment_capabilities`、`environment_candidate_create`、
`environment_candidate_status`、`environment_candidate_cancel` 和 `environment_candidate_apply`。
create 分层验证并返回完整公开预览与一次性 token；apply 原子消费 token，返回 `apply_queued` ack 后由
Application owned task 继续执行。create 返回前断开会取消请求并清理私有材料；apply ack 后断开不会
取消已移交任务，调用方必须通过 status 观察终态。MCP 不自动启停 Listener，也不中断活动连接。

原有查询和 capabilities 的逻辑预算为输入 256 KiB、输出 8 MiB、期限 8 秒；create 为输入/输出
1 MiB、总期限 30 秒；status/cancel/apply 为输入 16 KiB、输出 1 MiB、ack 期限 8 秒。所有输入 Schema
递归封闭未知字段，成功值必须符合公开 output schema。私钥、密码、保护后字节和原始请求体不进入
预览、status、终态、错误、日志或 diagnostics；confirmation token 只在 create 成功响应中交付。

## 9. 复现报告

复现报告把同一范围内的配置快照、Listener 状态、规则/协议包身份、结构化诊断和运行日志组合成
可复制 Markdown。它不读取 `ExchangeObservationStore`，也不聚合 HTTP 抓包；线路证据必须通过
`exchange_observation_query/get` 或 `http_capture_query/get` 独立查询。报告用于共享“当时观察到了
什么”，不是数据库备份，也不保证包含已被容量策略淘汰的历史。

报告生成失败不能影响 Listener；部分来源失败应进入 collection errors，而不是丢弃已取得的证据。

## 10. 排障读取顺序

1. 先确认 Workspace、Listener 与 runtime epoch；
2. 查 ExchangeObservation 是否有 `Opened`；
3. 对照四个方向的 `Received`/`Sent`；
4. 若出现 `Failed`，先看 stage，再看同一 exchange_id 的普通日志；
5. 检查 `evidence_evicted`、`evicted_records` 和 `ignored_events`；
6. 外部协议包再对齐 connection generation、RPC request ID 和 method；
7. 最后生成复现报告，明确区分自动化证据、真机证据和业务响应。

跨通道关联字段按“适用才记录”统一使用以下名字，不要求无关事件伪造空身份：

| 范围 | 关联字段 |
| --- | --- |
| Listener 一次运行 | `runtime_epoch`、`listener_id`；Workspace 范围再带 `workspace_id` |
| 一条连接级 Exchange | `exchange_id`，并继承 `runtime_epoch`、`listener_id`、`workspace_id` |
| 协议阶段 | `direction`、`stage`；完整 Context/Document 由 Exchange observation 保存 |
| 外部协议包连接/调用 | `package_id`、`package_version`、connection `generation`、`request_id`、`method` |

`reproduction_report` 与 runtime log 可用 Listener/runtime/package 字段对齐；需要逐报文对齐时必须再查
Exchange/HTTP 专用通道。不能因为某一通道按职责没有复制 payload，就声称 payload 没有发生。

“端口已监听”只证明 Listener 启动；“Sent 已记录”只证明代理写入；“Closed completed”才表示当前
Exchange 流程完成。任何一项都不能单独证明外部业务系统已经完成结算。

## 11. 验证门禁

观测改动至少覆盖：

- opened 缺失、重复和身份不一致；
- 事件追加顺序以及 D2 不覆盖 D1；
- UI 当前页面实时失效刷新；
- 队列条数/字节背压和独立 loss signal；
- Store 淘汰、当前事件回滚与容量释放；
- Display 失败不影响交易；
- Pipeline 失败产生 Failed + Closed failed；
- JSONL 重开、坏行、重复 ID、低水位滚动和写失败；
- MCP 五个环境工具的精确 read-only/destructive/idempotent 注解，以及各类输入输出预算与 deadline；
- MCP 顶层/嵌套封闭输入、34 个查询加五个环境工具的目录/分发一致性、成功输出根类型与文档全量工具名；
- UI、MCP 查询同一个 ExchangeObservationStore。
