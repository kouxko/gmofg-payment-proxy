# 运行时观测与诊断

本文说明 Exchange 结构化观测、普通运行日志、UI 实时刷新、只读 MCP 和复现报告之间的关系。
核心原则是：观测系统可以丢证据，但不能阻塞或篡改交易；业务 Pipeline 失败则必须明确失败。

## 1. 两条独立的观测通道

```text
业务代码 / Exchange
  └─ tracing subscriber（进程唯一）
       ├─ 普通 tracing -> 有界队列 -> RuntimeLogStore -> JSONL + 日志 UI/MCP
       └─ exchange::ui -> 有界队列 -> ExchangeObservationStore -> 抓包 UI/MCP
```

普通日志适合错误、状态转换、资源和生命周期诊断。ExchangeObservation 保存可逆的 HTTP 文本或
Socket 字节、Document、Display 及实际发送内容。两者不能互相替代：格式化日志不应被反向解析成
协议报文，Exchange 证据也不承担全局系统日志职责。

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
- 无法可信归属的丢弃通过 `ignored_events` 暴露；
- 清空 Workspace 证据时同时释放容量账本。

观测队列还有独立的一槽 loss notification 通道，使 UI 即使拿不到丢失 payload，也能收到“证据有
变化/损失”的刷新信号。

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

## 8. 只读 MCP

MCP 使用官方 `rmcp`，当前协议版本为 `2026-07-28`。服务只绑定 loopback、无认证且只读；同机
其他进程可读取暴露信息，因此它是本地诊断面，不是远程管理 API。

主要诊断工具包括：

- `application_snapshot`：一次 generation 校验后的应用快照；
- `application_log_query/get`：普通运行日志；
- `exchange_observation_query/get`：与 UI 相同的 Exchange 内存记录；
- `diagnostics_query`、`diagnose_recent_failures`：结构化故障与确定性建议；
- `reproduction_report`：按 Workspace/Listener 组合有界 Markdown 报告；
- Listener、规则、协议包、证书和 Android 状态的只读查询。

MCP 输入逻辑预算为 256 KiB，输出预算为 8 MiB，单工具截止时间为 8 秒。超预算或超时返回结构化
错误，不执行删除、重连、启停、规则修改或文件写入。

## 9. 复现报告

复现报告把同一范围内的配置快照、Listener 状态、规则/协议包身份、结构化诊断、运行日志和可选
Exchange 证据组合成可复制 Markdown。它用于共享“当时观察到了什么”，不是数据库备份，也不保证
包含已被容量策略淘汰的历史。

报告生成失败不能影响 Listener；部分来源失败应进入 collection errors，而不是丢弃已取得的证据。

## 10. 排障读取顺序

1. 先确认 Workspace、Listener 与 runtime epoch；
2. 查 ExchangeObservation 是否有 `Opened`；
3. 对照四个方向的 `Received`/`Sent`；
4. 若出现 `Failed`，先看 stage，再看同一 exchange_id 的普通日志；
5. 检查 `evidence_evicted`、`evicted_records` 和 `ignored_events`；
6. 外部协议包再对齐 connection generation、RPC request ID 和 method；
7. 最后生成复现报告，明确区分自动化证据、真机证据和业务响应。

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
- MCP 只读标注、输入输出预算与 deadline；
- UI、MCP 查询同一个 ExchangeObservationStore。
