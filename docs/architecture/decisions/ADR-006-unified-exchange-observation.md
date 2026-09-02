# ADR-006：以统一 Exchange 模型观察 HTTP 与 Socket

- Status: Superseded by [ADR-007](ADR-007-exchange-pipeline-runtime-boundary.md) on 2026-08-24
- Date: 2026-08-21
- Scope: capture/session/exchange observation model
- Refines: ADR-001, ADR-002

> 本 ADR 仅保留为历史记录。当前设计把 `Exchange<P>` 定义为一个 accepted App Connection 的连接级运行器，
> 直接持有 upstream/downstream 两个 `Pipeline<P, D>`；不再定义 `Flow` 或 `Interaction`。运行证据按连接顺序
> 追加到有界内存 observation，且不进入 SQLite。实现不得继续以本 ADR 的聚合根和持久化定义为准。

## Context

HTTP 当前以 Session 聚合请求与响应，Socket Relay 按 upstream/downstream Frame 分别保存，
Socket LocalResponder 又使用专属 LocalExchange。相同的“客户端经过 Proxy 与服务端交互”因此被实现成
三套生命周期、三套失败分支和三套 UI。

这个分裂已经造成可观察性缺口：处理成功时可能看到收发记录，但 Frame、Decode、Rule、Encode 或写入失败时，
抓包页面可能没有可打开的记录。继续增加 `RelayFailure`、`LocalFailure` 和 HTTP 专属终态只会扩大分支。

ADR-001 和 ADR-002 要求 HTTP 与 Socket data plane、协议 DTO、规则和 runtime 保持隔离。本决策不改变
该边界，只统一 Application 观察层的交互生命周期、查询和 UI 外壳。

## Decision

### 1. Exchange 是观察层聚合根

`Exchange` 表示一次已经被观察到的交互尝试，而不是“成功配对后的请求与响应”。它可以是：

- 成功的 request/response；
- 处理或传输失败的尝试；
- 连接中断前尚未完成的尝试；
- 无法关联的单向 request；
- RemoteServer 主动发送的消息；
- 没有协议边界的透明 Socket 数据段。

Exchange 在第一次可以归属的输入证据出现时创建，并在每次状态变化时提高 `revision`。不得等整笔交易成功后
才保存抓包。

### 2. Rust 内部使用泛型 Exchange，不修改协议包

Rust 领域层使用 `Exchange<P: ExchangeProtocol>`。`P` 通过 associated types 提供 upstream/downstream
evidence、boundary 和 processing stage，使 HTTP 与 Socket 共享生命周期实现，同时让非法协议组合在编译期
不可表达。

协议包合同保持不变，继续只负责既有 `frame/decode/encode/display`。Exchange、关联、持久化和 UI 聚合均由
Rust 层负责，不向 Manifest、Rhai 或外部软件包 JSON-RPC 增加 Exchange 字段或方法。

`Exchange<P>` 是 Proxy 内一笔交互的状态聚合根，不是 data-plane processor 或网络转发器。HTTP/Socket runtime
继续执行真实 I/O 与协议处理，并把事实写入 Exchange；`ExchangeCoordinator<P>` 只负责创建、关联、状态迁移、
revision 与 snapshot。

Rust 内部使用泛型；SQLite、Tauri 和 TypeScript 的异构边界使用封闭联合：

```text
AnyExchangeDetail = Http(HttpExchangeDetail) | Socket(SocketExchangeDetail)
```

HTTP Header、Status、CONNECT、MITM 不得进入 Socket evidence；Socket Frame、Schema、Document、half-close
不得进入 HTTP evidence。协议 runtime 继续彼此隔离。

### 3. Endpoint 使用 LocalServer 与 RemoteServer

- `LocalServer`：Proxy 进程内的本地模拟服务，不建立 loopback TCP；UI 显示“本地模拟服务”。
- `RemoteServer`：通过真实网络连接的上游；UI 显示“远程服务”。

不使用“本地回环服务”，因为该名称会错误暗示存在 `127.0.0.1/::1` 网络 I/O。

### 4. Exchange 以 upstream/downstream 组织四条逻辑边

方向语义与现有协议包一致：

- `upstream`：App → Proxy → Server；
- `downstream`：Server → Proxy → App。

Exchange 使用封闭形状 `UpstreamOnly | DownstreamOnly | Paired`，不使用两个裸 `Option`，从而避免构造
upstream/downstream 都不存在的空 Exchange。

每个 Exchange 固定投影以下四条逻辑边：

1. `AppToProxy`
2. `ProxyToServer`
3. `ServerToProxy`
4. `ProxyToApp`

`LocalServer` 的第 2、3 条边使用 `InProcess`；其余边使用 `Network`。每条边状态为
`NotReached | Pending | Succeeded | Failed`，并保存计划字节数与实际提交字节数。部分写入不能标记为成功。

每个 Direction 内部拥有 `receive + transform + send + display`；receive 与 send 分别固定持有 ingress/egress
Leg，协议 Evidence 由 receive 持有：

- upstream ingress/egress 固定为 `AppToProxy/ProxyToServer`；
- downstream ingress/egress 固定为 `ServerToProxy/ProxyToApp`；
- 一个 sealed `ExchangeDirection<P>` trait 统一描述方向合同；`Upstream` 与 `Downstream` 两个零大小类型分别
  实现它，并提供各自的 Evidence、ProcessingStage、ingress 和 egress；
- runtime `ProtocolDirection` 枚举只用于日志、SQLite、IPC 和 UI 投影；
- marker type、associated types 与私有构造函数共同阻止协议、方向、Evidence、Stage 和 Leg 错配。

Frame、Decode、Rule、Encode 和协议加解密按职责分别进入 receive、transform、send。`display` 是 Direction
内的异步旁路状态，失败时保存完整错误并回退 Hex/raw，但不得改变已经确定的网络 outcome。

每个 Direction 的主链进一步固定为 `receive → transform → send`：

- receive：ingress Leg、Socket 可选 Frame、Decode、received Document；
- transform：从 received Document 执行 Rules，生成独立 output Document；
- send：从 output Document Encode/Serialize，并通过 egress Leg 交付；
- display：读取 output Document 的异步旁路，不阻塞 send，不改变网络 outcome。

协议包合同规定 Display 读取规则处理后的 Document。为了保持协议包零修改，不把 package Display 移到
Decode 与 Rules 之间。UI 对原始输入的展示使用 origin、received Document 和内建 Hex/字段视图；不得通过
额外调用 package Display 伪造第二套展示语义。

### 5. 报文边界必须如实表达

Exchange 的边界类型为：

- `HttpMessage`：HTTP runtime 已确认的消息；
- `SocketFrame`：协议包 `frame` 已确认的完整 Frame；
- `StreamChunk`：透明 Socket 的一次真实 read/write 数据段，或无法形成 Frame 的残留证据。

没有协议包时继续保持“读到多少写多少”的透明语义。平台不得把任意 TCP read 当作业务报文，也不得把
两个方向的 `StreamChunk` 猜测成 request/response。

### 6. 关联必须显式且可证明

关联质量为：

- `Exact`：HTTP 生命周期、同一 LocalServer 调用或协议字段提供确定关联；
- `Sequential`：协议显式声明单笔在途，且连接状态证明没有歧义；
- `Unpaired`：单向消息已完成传输，但无法安全关联另一半；
- `Unsolicited`：RemoteServer 主动发送且没有对应请求。

协议包不承担关联配置。Rust Socket Entry/Listener 可以配置 `None | SequentialSingleFlight |
DocumentFields`，并使用既有 Decode Document 计算关联。没有 Rust 侧配置时不配对；平台禁止隐式 FIFO。

### 7. 生命周期与关联相互独立

Exchange 生命周期为：

```text
Active | Completed | Failed | Interrupted
```

`Completed + Unpaired` 是合法组合，表示单向数据已经成功交付，但不存在可靠的 request/response 关联。
连接关闭时仍为 `Active` 的 Exchange 必须转成 `Interrupted`，不能从列表消失。

### 8. 泛型 ExchangeCoordinator 只协调观察状态

同一 Socket 连接的双方向 processor 共享一个 `ExchangeCoordinator`。HTTP 使用相同状态合同，但继续由
HTTP pipeline 持有 HTTP message 生命周期。

Coordinator 负责 Exchange ID、revision、合法状态迁移、关联、终态和 immutable snapshot；它不执行
HTTP 解析、Socket 分帧、规则、编码、LocalServer 响应生成或网络写入。

网络 outcome 进入 terminal 后不得回到 `Active`，但异步 `display` 仍可从 `Pending` 更新到
`Rendered | Failed` 并提高 revision，不改变 terminal outcome。

### 9. Snapshot 使用 revision 幂等更新

每个状态变化产生完整 immutable snapshot。持久化与 WebSocket 都按 `(runtime_epoch, exchange_id, revision)`
幂等更新，只允许更高 revision 覆盖。前端更新同一行，不为每个方向追加互不关联的记录。

清空抓包会提高 `capture_generation`；旧 generation 的迟到 snapshot 必须丢弃，避免记录被清空后重新出现。

### 10. 统一存储索引，隔离协议 evidence

SQLite 使用公共 Exchange 索引和四边表，以获得统一排序、分页、筛选和清理语义；HTTP 与 Socket 完整证据
分别保存在协议专属表。开发阶段直接替换旧 Socket capture 表和 DTO，不提供兼容投影。

Workspace、Listener、协议包和密钥配置不属于抓包迁移范围，不得删除。

### 11. 使用全量诊断日志

当前应用没有隐私或脱敏要求。日志允许并要求输出完整诊断证据，包括：

- HTTP Header、Body 和序列化前后内容；
- Socket 原始字节、Hex、Frame、Document、规则输入输出和 Display；
- 关联字段、完整第三方错误和堆栈；
- DUKPT/加解密调用可获得的输入、输出和密钥诊断材料；
- Exchange、Session、stage、revision、时间、长度和 hash。

日志采用结构化记录并轮转；容量控制不得通过脱敏、截断证据或静默丢弃实现。

## Alternatives

### 保持三套模型，仅统一 UI 样式

Rejected。它不能统一失败、状态迁移、持久化和真实 Relay 关联，后端分支仍会持续扩大。

### 一个包含全部 HTTP/Socket 字段的万能 DTO

Rejected。大量 nullable 字段允许非法组合，违反 ADR-001/002 的 data-plane 边界。

### 把泛型 Exchange 直接暴露给 SQLite/Tauri/TypeScript

Rejected。异构列表无法容纳不同的 `P`，生成绑定也会泄漏 Rust 实现细节；公共边界应使用稳定的封闭联合。

### upstream/downstream 使用两个裸 Option

Rejected。它允许构造两个方向都不存在的空 Exchange；封闭 `ExchangeFlow` 明确表达单向与配对形状。

### 为 upstream/downstream 定义两个互不相关的 trait

Rejected。两个 trait 会复制相同 API，也允许实现形成不一致的能力组合。一个 sealed `ExchangeDirection<P>`
加两个方向实现，可以共享算法并让协议与方向的 associated types 保持绑定。

### Relay 默认 FIFO 配对

Rejected。并发请求、迟到响应、粘包和服务端主动推送会造成错误关联，错误证据比没有关联更危险。

### 把透明 Socket read 当作完整业务报文

Rejected。TCP read 只代表本次读取的数据段，不提供业务消息边界。

### LocalServer 伪造成 loopback 网络 Server

Rejected。运行时没有 loopback Socket、resolver、connector 或 upstream TLS，伪造会破坏排障证据。

### 仅在成功后保存一条完整记录

Rejected。它会继续丢失 Frame、Decode、Encode、连接和部分写入失败，是当前问题的直接来源。

### 完整 Event Sourcing

Rejected for now。当前需求只需要 revision snapshot、完整阶段证据和结构化日志。完整事件溯源会增加回放、
迁移和一致性成本，但暂时没有对应产品收益。

## Consequences

- 成功、失败、中断和单向消息使用同一生命周期与 UI。
- LocalServer 与 RemoteServer 使用相同 Exchange 外壳，但不会伪造不存在的网络行为。
- HTTP 与 Socket data plane 继续隔离，只共享泛型 Exchange 核心、Repository port、查询和 UI shell。
- 协议包和外部软件包合同保持不变；Socket 关联策略属于 Rust Socket Entry/Listener 配置。
- 观察写入需要 revision、generation 和异步乱序测试。
- Display 完成可以在网络 terminal snapshot 之后继续提高 revision，因此 terminal outcome 与观察 enrichment
  必须分别建模。
- 全量日志会显著增加磁盘写入和文件体积，必须实现轮转、容量告警和明确的写入失败诊断。

## Acceptance Conditions

- HTTP、RemoteServer Socket、LocalServer Socket 成功时均可显示为一笔 Exchange。
- 任一处理或写入失败均保留此前证据，后续步骤明确为 `NotReached`。
- 连接关闭时未完成 Exchange 进入 `Interrupted`。
- LocalServer 中间边显示 `InProcess`，不得出现虚构的地址、TLS 或网络写入。
- 透明 Socket 使用 `StreamChunk`，无关联证据时不得合并 request/response。
- Rust 内部使用 `Exchange<P>`，upstream/downstream Direction 不能组合出非法 Leg 或空 Exchange。
- `Upstream/Downstream` 必须分别实现同一个 sealed `ExchangeDirection<P>`；不得复制两套方向 trait API。
- 每个 Direction 有独立 Display 状态；Display 失败不改变网络 outcome，异步完成可提高 revision。
- 协议包 Manifest、Rhai 和外部软件包 JSON-RPC 合同不因 Exchange 重构而变化。
- WebSocket 对同一 Exchange 只更新一行，并拒绝旧 revision 和旧 generation。
- 日志输出完整 payload、Document、关联、加解密和错误证据，不进行脱敏或静默截断。
- HTTP/Socket 协议专属类型边界继续由架构扫描 fail-closed。

## Implementation Deferred

- 生产代码重构、数据库迁移和 IPC 替换在详细设计评审后实施。
- 具体 Rust 类型和 SQL 字段名可在不改变本 ADR 语义的前提下调整。
- HTTP Mock、CONNECT、WebSocket upgrade 与 Socket server push 的详细映射由实现测试锁定。
