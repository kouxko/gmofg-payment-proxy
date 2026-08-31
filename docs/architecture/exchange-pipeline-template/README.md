# Exchange / Pipeline 最小架构模板

> 这是本项目 Exchange/Pipeline 的权威设计模板，不参与工程编译。生产实现、测试和架构门禁必须持续与本模板一致。

## 代码结构

```text
exchange-pipeline-template/
├── README.md       设计结构和强约束
├── core.rs         Protocol、Context、Pipeline、Envelope 和基础 trait
├── runtime.rs      外层 Exchange、协议模式和连接生命周期
└── transparent.rs  Socket 原始双向转发和半关闭传播
```

## 核心结构

```text
Exchange<P>
└── mode
    ├── ProtocolExchange<P>
    │   ├── AppConnection<P>
    │   │   ├── Reader<P, Upstream>
    │   │   ├── Writer<P, Downstream>
    │   │   └── shutdown()
    │   ├── ServerSlot<P>
    │   ├── upstream: Pipeline<P, Upstream>
    │   └── downstream: Pipeline<P, Downstream>
    └── TransparentExchange
        ├── App RawConnection
        ├── RawServer
        └── 两条并发 raw relay
```

一个 App Connection 始终对应一个外层 `Exchange<P>`。外层统一记录连接打开、关闭和最终结果，并只暴露 `Exchange::exchange()` 给 accept loop。Exchange core 由外部持续 poll，不在内部创建脱离连接生命周期的后台任务。

协议模式不定义 `Flow`。`ProtocolExchange` 直接持有 upstream/downstream 两个 `Pipeline`。透明模式只允许通过 `Exchange<Socket>::transparent(...)` 构造，不伪装成协议 Pipeline。

每个 `Pipeline` 固定为：

```text
Pipeline::read(Reader)
  -> Result<Option<Envelope<P, D>>, Error>
  -> Pipeline::write(Writer, &Envelope)
  -> Result<P::Context, Error>
```

`Envelope` 是 Reader 产生的不可变事实。Writer 只能读取它；Rules 接收 `envelope.document().clone()`，返回独立的输出 Document，再交给 Encode。原始 `context/document/display` 在整个 Writer 阶段保持不变。

`Some(Envelope)` 表示已经读到一条完整协议消息；`None` 只表示 Reader 到达 EOF。Reader 自身只提供协议 Context，完整的 `Envelope` 由 Read Pipeline 按协议固定串联生成：

```text
HTTP   Reader -> Decode -> Display -> Envelope
Socket Reader -> Frame  -> Decode -> Display -> Envelope

HTTP/Socket Envelope -> clone Document -> Rules -> Encode -> Writer
```

## 生产能力装配

模板中的 trait 必须由生产 adapter 直接实现，不能用一个已经完成 Decode/Rules/Encode 的
旧 processor 塞进 `Decode`，再用空 Rules 或取字段式 Encode 伪装成 Pipeline。方向 factory
返回五个真实能力；同一方向可以共享连接级 runtime handle，但每个 trait 每次只能执行自己
负责的阶段：

```rust
pub struct SocketDirectionCapabilities<D: Direction> {
    pub frame: Box<dyn Frame<D>>,
    pub decode: Box<dyn Decode<Socket, D>>,
    pub display: Box<dyn Display>,
    pub rules: Box<dyn Rules>,
    pub encode: Box<dyn Encode<Socket, D>>,
}

pub trait SocketProtocolCapabilityFactory: Send + Sync {
    fn upstream(
        &self,
        connection: &SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Upstream>, Error>;

    fn downstream(
        &self,
        connection: &SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Downstream>, Error>;
}

pub struct RulesChain {
    rules: Vec<Box<dyn Rules>>,
}

#[async_trait::async_trait]
impl Rules for RulesChain {
    async fn apply(&mut self, mut document: Document) -> Result<Document, Error> {
        for rules in &mut self.rules {
            document = rules.apply(document).await?;
        }
        Ok(document)
    }
}
```

Package adapter 的 Frame/Decode/Display/Encode 分别调用统一固定 RPC；Rust 内建 adapter
直接实现相同 trait。软件包 wire contract 不新增
`direction`、开关字段或 ExecutionContext，仍使用既定 `hooks.upstream.*` 与
`hooks.downstream.*`。

`Pipeline` 只保留协议和方向两个泛型；Read/Write Pipeline 作为内部 trait object 装配。因此运行时核心类型保持为 `Exchange<P>`，不暴露 Reader/Writer 实现类型参数。

## HTTP

```text
Reader<Http>
  -> HttpContext { header, body }
  -> Decode
  -> Document
  -> Display
  -> Envelope
  -> Rules
  -> Encode
  -> HttpContext
  -> Writer<Http>
```

生产装配由 `HttpProtocolRuntimeSnapshot` 直接实现 `HttpProtocolCapabilityFactory`。每条
connection、每个方向创建一个独占 `ProtocolDirectionExecutor`，四个 capability 共享该执行器，
但各自只调用一个单阶段 API：

```rust
Decode  -> ProtocolDirectionExecutor::decode_document
Display -> ProtocolDirectionExecutor::display_document
Encode  -> ProtocolDirectionExecutor::encode_document

upstream Rules   -> AppToProxy -> ProxyToUpstream
downstream Rules -> UpstreamToProxy -> ProxyToApp
```

协议包只处理 UTF-8 Body；`HttpContext.header` 由 Encode 原样保留，新的 Body 必须仍是
UTF-8。未绑定协议包时，Plan 必须显式创建 `PlainHttpCapabilityFactory`，不能把
`PipelinePorts` 或旧组合 processor 当作 Decode。`PipelinePorts::apply_request_policy` 与
`apply_response_policy` 只保留产品级 HTTP wire mutation、breakpoint、session、capture 和
fault-action 职责，不执行协议包，也不承担 Exchange UI 观测。

Wire policy 必须在该方向 Reader 创建 `HttpContext` 之前且仅执行一次；这样它的 Message
修改会成为 Decode 的明确输入。Encode 完成后不允许再次调用 wire policy。Encode 返回的
Context 交给 Writer 后，Writer 只执行此前已确定的网络故障动作和真实写出：

```text
framed HTTP Message
  -> apply_*_policy once
  -> Reader<Http>
  -> Decode -> Display -> Envelope -> Rules -> Encode
  -> Writer<Http>
  -> precomputed transport fault actions
  -> wire
```

Hyper 只负责 HTTP/1 framing，不拥有交易状态机。每条 accepted App connection 创建一个
长期 `Exchange<Http>`；完整 request frame 通过容量为 1 的 connection queue 依次交给同一个
App Reader，每条 command 用独立 oneshot 接收与它配对的 downstream 结果。Exchange 在上一笔
`App Write` 完成前不会读取下一笔，因此 HTTP pipelining 只能在 Hyper/queue 中等待，不能改变
`App Read -> Server Write -> Server Read -> App Write` 的顺序。

首次 request 的 scheme/host/port 固定该 Exchange 的 Server Endpoint。后续 request 可以改变
path、Header、Body，但必须解析到完全相同的 Endpoint；不一致由 App Reader 返回业务错误并结束
Exchange，且不得创建或拨号第二个 Server。HTTP keep-alive 保持开启，不能用关闭连接来规避该
连接级合同。CONNECT 和 Upgrade 在创建 Exchange/Server 之前拒绝。

## Socket

```text
Reader<Socket>
  -> SocketContext chunk
  -> Frame
      NeedMore -> Reader<Socket>（继续普通读取，不要求返回所需字节数）
      Complete -> consumed 必须等于 buffer.len()
  -> SocketContext frame
  -> Decode
  -> Document
  -> Display
  -> Envelope
  -> Rules
  -> Encode
  -> SocketContext
  -> Writer<Socket>
```

## Exchange 主循环

```text
App Read
  -> upstream Pipeline
  -> tracing received
  -> Server Write
  -> tracing sent
  -> 只等待 Server 回复，不并发读取 App
  -> downstream Pipeline
  -> tracing received
  -> App Write
  -> tracing sent
  -> 完成这一对交换后回到 App Read
```

## Socket 透明转发

透明转发的含义是“读到多少，就原样写多少”，因此它不能经过 `Frame`、`Decode`、`Document`、`Display`、`Rules` 或 `Encode`。这些步骤一旦出现，就不再是透明转发。

```text
App RawConnection
  -> read first raw bytes
  -> tracing received(upstream)
  -> RawServer.connect(first bytes)
  -> write exact first bytes
  -> tracing sent(upstream)
  -> concurrently
       upstream:   App RawReader    -> Server RawWriter
       downstream: Server RawReader -> App RawWriter
  -> one side EOF: finish opposite write half
  -> other direction continues until its own EOF or failure
```

透明模式只使用 `RawReader`、`RawWriter`、`RawConnection` 和 `RawServer`：

```text
RemoteRawServer -> real TCP/TLS RawConnection
LocalRawServer  -> in-process Echo RawConnection
```

LocalRawServer 不是旁路 responder。它创建进程内全双工 Echo 连接，让数据仍然完整经过 `TransparentExchange` 的 read/write、日志和半关闭流程。

`RawWriter::finish()` 只表达 TCP 写半关闭传播，不是连接级 shutdown，也不加入协议 `Writer<P, D>`。协议模式仍由 `Connection::shutdown()` 管理完整连接生命周期。

透明模式保留操作系统实际 read 的 chunk 边界作为观测事实，但不承诺应用协议消息边界。一个 chunk 可以是半个协议帧、一个帧或多个粘连帧；发送字节必须与收到字节完全一致。任意读取、连接或写入失败都会结束 Exchange，绝不在协议包处理失败后静默退回透明模式。

## 日志与 UI 观测

`Exchange` 和 `Pipeline` 只调用 `tracing`，不直接依赖 UI、数据库或日志存储：

```text
Exchange / Pipeline
  -> structured tracing event
      ├── RuntimeLog Layer      全部诊断日志 -> 有界内存，可选滚动文件
      ├── Exchange UI Layer     只接收 intercept_proxy::exchange::ui
      └── fmt Layer             开发控制台
```

事件分为两个 target：

```text
intercept_proxy::exchange::ui
  opened / received / sent / failed / closed
  received: direction、context、document、display
  sent: direction、context
  failed: direction、stage、可获得的 context、error

intercept_proxy::exchange::diagnostic
  Socket chunk、Frame NeedMore、Display 回退、阶段耗时等排查细节
```

UI Layer 读取 tracing 的结构化字段，不解析格式化后的日志字符串。通用日志页面可以继续展示格式化文本。所有 Layer 都使用有界、非阻塞、fail-open 的输出；UI 关闭、内存已满或日志文件失败都不能改变交易结果。

`context`、`document` 和 raw bytes 不使用 tracing 的 `?Debug` 作为 UI 数据源。producer
必须投影为可无损读取的 primitive 字段：HTTP 使用 `context_header/context_body`，Socket/raw
使用 `context_bytes_hex`，Document 使用 `document_json`。父 span 携带
`exchange_id/workspace_id/listener_id/runtime_epoch/connection_id/peer/protocol/endpoint`；
UI Layer 合并父 span 与事件字段后写入同一个连接记录。

```rust
pub struct ExchangeRecord {
    pub exchange_id: ExchangeId,
    pub metadata: ExchangeMetadata,
    pub events: Vec<ExchangeEvent>, // Vec 下标就是唯一顺序，不增加 sequence
}

pub enum ExchangeEvent {
    Opened { at: DateTime<Utc> },
    Received {
        at: DateTime<Utc>,
        direction: DirectionKind,
        context: ExchangeContext,
        document: Option<Document>,
        display: Option<String>,
    },
    Sent {
        at: DateTime<Utc>,
        direction: DirectionKind,
        context: ExchangeContext,
    },
    Failed {
        at: DateTime<Utc>,
        direction: Option<DirectionKind>,
        stage: String,
        error: String,
        context: Option<ExchangeContext>,
    },
    Closed { at: DateTime<Utc>, outcome: ExchangeOutcome },
}
```

运行时报文不进入业务数据库。内存日志允许清空并在重启后消失；如果启用滚动日志文件，它只是诊断产物，也不是业务数据。

`Stage` 不定义为 Rust enum。阶段是错误发生位置已经知道的观测字段，由该层直接写入
`tracing::error!(stage = "decode", ...)`。`Error` 只负责通过 `Result` 把失败返回给调用方；tracing 记录不能代替错误返回，也不能让失败后的 Pipeline 继续执行。

## 强约束

1. 一个 App Connection 对应一个长期运行的 `Exchange`。
2. App 不断开，`Exchange::exchange()` 就持续被 poll。
3. 每个 `Pipeline::read()` 只返回一个完整 `Envelope`。
4. Socket 每次先读取数据，再调用 `Frame`。
5. Frame 不完整时继续读取；完整后立即向 Server 发送。
6. 一次 Socket 读取出现一个 Frame 之外的数据是协议错误。
7. 协议模式严格按 `App Read -> Server Write -> Server Read -> App Write` 顺序执行；同一时刻只 poll 当前步骤。
8. 协议模式下 App EOF 结束 Exchange；Server EOF/失败使 Exchange 失败。透明模式 EOF 遵循第33条。
9. Display 失败只回退展示，不影响交易。
10. Decode、Rules、Encode、Connect、Read、Write、Flush 失败都结束 Exchange。
11. LocalServer 和 RemoteServer 实现同一个 `Server<P>`。
12. `Envelope` 仅保存不可变的 `context/document/display`，不提供 `document_mut()`。
13. Exchange 只输出结构化 tracing 事件，不持有自定义 RuntimeStore。
14. 数据库只保存 Workspace、Listener、Rules 和 Package 等配置。
15. 项目自定义无泛型异步 `Display` trait，直接返回 `String`，不定义 `DisplayOutput`。
16. 方向和协议在 Pipeline 组装时确定，`Display` 只依赖 `Document`。
17. `Protocol::Context` 只约束基础 Rust 能力，不定义转字节的 `evidence()`。
18. HTTP 运行记录保存 `HttpContext`，Socket 运行记录保存 `SocketContext`，不做类型抹平。
19. UI 事件与诊断日志按 tracing target 分流，UI 不解析日志字符串。
20. 日志和 UI 观测必须 fail-open；观测失败不影响交易。
21. `FrameResult::NeedMore` 不携带长度。它只表达当前缓冲区不足；精确长度并非所有协议都能提前得知，且当前 `Reader::read()` 不接受读取长度提示。
22. 不定义 `Stage` enum；阶段名称是 tracing 的结构化字段，业务失败仍必须通过 `Result::Err` 传播。
23. Writer 接收 `&Envelope`；Rules 修改 Document 的 clone，不能覆盖 Reader 产生的原始 Document。
24. `shutdown()` 属于 `Connection<P, RD, WD>`；`Reader` 只负责 read，`Writer` 只负责 write。
25. 每次 `Pipeline::read()` 返回 Envelope 后立即记录 `received`；Writer 成功后才记录 `sent`，因此 Rules/Encode/Connect/Write 失败不会隐藏已收到的 Envelope。
26. 不增加 message sequence 或 interaction ID；同一连接内的事件按产生顺序逐条追加显示。
27. `Pipeline` 只使用 `Pipeline<P, D>`；`Exchange` 只使用 `Exchange<P>`，不暴露 Read/Write 实现泛型。
28. Connect 和 shutdown 在真实发生位置记录 tracing；shutdown 观测失败不改变原交易结果。
29. 透明转发只适用于 Socket，并由 `Exchange<Socket>::transparent()` 构造；HTTP 不提供透明模式入口。
30. 透明转发不创建 Envelope，不调用任何协议 Pipeline stage，也不修改原始字节。
31. TransparentExchange 收到第一段 App 字节后才连接 Server，并先完整写出首段字节，再启动双向并发 relay。
32. 透明模式按每次真实 I/O 追加 `received`/`sent` 事件；不增加 message ID，也不把 chunk 误称为协议消息。
33. 任一方向 EOF 只传播对应的写半关闭；另一方向必须继续处理到 EOF 或失败。
34. LocalRawServer 和 RemoteRawServer 实现同一个 `RawServer`；本地回环仍走 TransparentExchange。
35. 协议模式失败不得自动降级为透明转发。
36. 每个 Exchange 持有内部 `ExchangeId`，只用于并发 tracing/UI 事件归属；它不进入协议、Envelope、Document、Rules、Server 数据或普通业务展示。
37. 当前架构不支持 HTTP CONNECT 或 Upgrade；`Exchange<Http>` 不切换为 raw tunnel，也不在 Exchange 外设置旁路处理。该能力只有经过新的明确设计确认后才能加入。
38. 一个 App Connection 在创建 Exchange 时固定绑定一个 LocalServer 或 RemoteServer Endpoint；整个 Exchange 生命周期内不得因 HTTP Host、scheme、port、URL 或 Rules 结果切换 Endpoint。HTTP path、Header 和 Body 可以变化。
39. LocalRawServer 每次 raw read 得到非空 bytes 后立即 Echo 完全相同的字节；不等待 Frame、EOF 或空闲超时，不解析、不累计、不修改，只保证字节流一致。
40. RawConnection 正常单向 EOF 使用对应 `RawWriter::finish()` 传播写半关闭；任一 relay 失败取消另一方向，全部 raw halves 释放时必须关闭底层 TCP/TLS。该 Drop 合同由所有实现和测试保证，不增加 RawCloser。
41. 协议模式只约束 Proxy 的处理顺序，不限制 App 的网络写入时机。等待 Server 时不并发读取或探测 App；提前到达的数据留在 transport 缓冲区，上一笔完成后才作为下一笔读取。
42. Endpoint 合同要求一次请求只返回一次回复。Proxy 不运行后台 Server Reader；同一次 Socket read 出现第二个 Frame 是协议错误，稍后才到达的额外回复不主动探测，属于违反 Endpoint 合同的未支持行为。
43. 必要数据完成前的 read/write/flush/half-close 失败使 Exchange 失败；协议交易已完整写给 App 后的最终 Connection shutdown 失败只记录附加诊断，不把成功改为失败，也不覆盖既有业务错误。
44. 不区分或保存 partial write 的 committed prefix；write/flush 未完整成功统一记录为 write 失败并结束 Exchange，不产生 `sent`，也不自动重试。
45. Exchange 运行记录使用有界内存；达到容量后淘汰最旧记录并显式标记已经发生数据淘汰。内存淘汰和观测失败均不得改变交易结果。
46. Exchange 不定义新的超时或容量默认值：Connect/Read/Write 使用 Listener 配置，Socket 单次读取与诊断队列/内存使用 Listener 的显式 `runtime_limits`，Session/UI Event 使用 Settings 配置，外部包 RPC 使用 external-package runtime 配置。Exchange 只接收已校验值，不在内部 fallback，也不使用 `max(1)` 静默修正非法输入。
47. 除已确认的 Display fallback、观测 fail-open 和最终 shutdown 附加诊断外，不增加任何兜底、自动重试、静默降级、旁路处理或失败后的透明回退。
48. 实现不受旧 Runtime、旧 DTO、旧数据库或旧模块边界限制；允许大规模破坏性重构，不增加兼容层。新行为通过测试和构建后删除未使用、重复和被替代的代码。
49. 分包必须按 Protocol、Pipeline、Connection、Server、Exchange、Observation 和具体 adapter 的职责边界组织；不得为了满足文件行数机械拆分相互依赖的碎片。
50. 每个生产源码和测试源码文件不得超过500行；超过前必须按单一职责拆分。
51. Frame、Decode、Display、Rules、Encode、Reader、Writer、Connect、Echo、raw relay、EOF、half-close、错误、内存淘汰和多 Exchange 事件归属都必须有完整单元或集成测试。
52. 公共 trait、核心类型、状态转换和非直观错误分支必须写清楚注释，明确数据从哪里读取、经过哪些转换、写向哪里以及失败如何传播；注释解释原因和合同，不复述语法。
53. Exchange 实现不得遗留未调用分支、无效 feature、旧 adapter、死代码或仅为兼容旧设计存在的抽象。
54. 完成标准包括 Rust fmt/clippy/test、前端 lint/typecheck/test、架构检查和最终 Tauri App 编译成功；缺少任何一项不得宣称完成。
