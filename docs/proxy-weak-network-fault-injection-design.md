# GMO-FG Payment Proxy 弱网与连接故障注入详细设计

> 文档状态：实施设计输入  
> 适用项目：GMO-FG Payment Proxy  
> 目标平台：Windows x64、macOS Apple Silicon  
> 核心约束：不在 Android 端安装 VPN，不要求 Android root，不建立第二套规则执行引擎  
> 与主需求的关系：本设计必须在实现前合并回 `docs/requirements.md` 的故障模拟、规则、运行时、IPC、测试和追踪矩阵章节。

## 1. 目标

在现有链路：

```text
GMO-FG Server <-> Rust Proxy <-> Payment App / DLL
```

中增加连接级和业务级弱网故障注入能力，使 Payment App 或 DLL 只要连接 Proxy，就能够在不使用 Android `VpnService` 的条件下测试：

- 请求发送失败。
- 请求发送超时。
- 上游连接失败或连接超时。
- 服务器响应丢失。
- 服务器响应超时。
- 响应中途断开。
- 响应截断。
- 上行和下行延迟。
- 上行和下行抖动。
- 上行和下行限速。
- 间歇性暂停和恢复。
- 第 N 次命中。
- 一次性故障。
- 指定交易通道或 DLL 通道故障。
- 按终端、请求路径、请求类型或 JSON 字段精确注入故障。
- Mock 或修改 Shift-JIS JSON 响应。
- 模拟或确认 `D48` 等业务返回。

本功能的定位是：

> 在 Rust Proxy 中模拟弱网对 Payment 业务和连接造成的结果，而不是伪装成 Android IP 层网络设备。

## 2. 强制前提

### 2.1 流量必须经过 Proxy

Payment App 和 DLL 必须将目标地址配置为 Proxy：

```text
Payment App -> Proxy 交易监听端口 -> GMO-FG 交易 Server
DLL         -> Proxy DLL 监听端口  -> GMO-FG DLL Server
```

默认监听端口沿用主需求：

| 通道 | Proxy 默认监听端口 |
| --- | ---: |
| 交易 | `16627` |
| DLL | `16127` |

如果 Payment 仍直接连接 GMO-FG Server，并且无法修改目标地址、DNS或外部路由，则普通桌面 Proxy 无法截获该连接。本设计不使用 Android VPN、root、iptables、系统级透明代理或设备侧注入。

### 2.2 TLS前提

必须满足现有双向 mTLS 设计：

- Payment 信任 Proxy 本地 CA。
- Payment 使用客户端证书连接 Proxy。
- Proxy 验证 Payment 客户端证书。
- Proxy 使用导入的上游客户端身份连接 GMO-FG Server。
- Proxy 验证上游 Server CA、主机名、SAN及有效期。

如果 Payment 使用无法更换的证书固定或目标地址固定，本功能不能绕过该安全限制。

## 3. 能力边界

### 3.1 可以实现

| 能力 | Rust Proxy语义 |
| --- | --- |
| 请求前断开 | 收到并匹配请求后，不建立上游连接，关闭App连接。 |
| 上游连接超时 | 不执行真实连接，等待配置的连接超时时间后返回稳定终态。 |
| 上游写入超时 | 上游连接和TLS可正常建立，但请求写入阶段保持等待直到超时。 |
| 上游读取超时 | 请求完整写入Server后，不向管线交付响应直到读取超时。 |
| 请求延迟 | 在向Server发送请求前等待。 |
| 响应延迟 | 收到Server响应后，在返回App前等待。 |
| 丢弃响应 | 请求正常到达Server，可选择读取完整响应或写完请求后立即放弃；不向App返回。 |
| 连接中途断开 | 在配置阶段主动关闭对应方向的连接。 |
| 响应截断 | 保持原始或指定`Content-Length`，只发送前N字节后关闭。 |
| 限速 | 将Body拆成有节奏的字节块发送。 |
| 抖动 | 每个分块或阶段加入有界随机等待。 |
| 间歇通断 | 在发送过程中按开启/关闭时间窗暂停和恢复。 |
| 第N次命中 | 按终端IP与客户端证书指纹组合计数。 |
| 精确业务匹配 | 按通道、阶段、路径/请求类型、JSON字段和匹配运算符执行。 |
| Mock业务响应 | Rust生成HTTP状态、Header及Shift-JIS Body。 |
| D48模拟 | Rust构造或修改包含D48的合法业务响应。 |
| D48确认 | Rust解码真实Server响应并将业务码写入会话ViewModel和测试断言。 |

### 3.2 不能等价实现

以下能力不属于普通应用层Proxy范围：

- Android Wi-Fi开关或移动网络开关。
- 整台设备断网。
- Android系统DNS失败。
- 影响Payment之外的其他应用。
- 真实IP包乱序、重复和内核级随机丢包。
- Android到Proxy之间的真实TCP SYN丢失。
- Android内核TCP拥塞控制和重传细节。
- App建立Proxy连接之前的网络故障。
- 射频、基站、Wi-Fi漫游或物理链路故障。

### 3.3 TCP“丢包”的正确实现边界

Proxy从TCP流读取到字节时，下层TCP已经完成可靠传输。不得把“随机删除HTTP Body字节”实现成普通随机丢包，否则得到的是人为损坏的应用报文，不是真实TCP丢包。

Proxy中的弱网动作必须使用以下可解释语义：

- 暂停发送。
- 延迟发送。
- 限速发送。
- 停止读取。
- 半关闭或关闭连接。
- 放弃完整响应。
- 显式截断HTTP报文。

只有“非法报文”和“截断报文”模板允许故意破坏应用层字节，而且必须明确标记为协议故障，不得标记为网络丢包。

## 4. 设计原则

| 编号 | 原则 |
| --- | --- |
| WN-ARCH-001 | 故障模板最终创建或更新普通拦截规则，不建立第二套执行引擎。 |
| WN-ARCH-002 | 规则匹配、计数、随机数、计时、限速、状态机、日志和校验全部由Rust实现。 |
| WN-ARCH-003 | Next.js只展示Rust ViewModel、收集输入和发送用户意图。 |
| WN-ARCH-004 | 不在TypeScript中实现延迟、随机数、限速、命中计数或故障状态判断。 |
| WN-ARCH-005 | 所有等待必须可被Proxy停止、客户端断开和规则停用取消。 |
| WN-ARCH-006 | 同一规则的一次性停用和命中计数必须沿用现有SQLite CAS事务。 |
| WN-ARCH-007 | 所有随机行为必须可复现、可审计、可在测试中使用确定性时钟验证。 |
| WN-ARCH-008 | Windows和macOS共享相同Rust领域与传输实现，不依赖平台专属网络驱动。 |
| WN-ARCH-009 | 故障动作不得阻塞Tokio执行线程，不允许使用同步sleep。 |
| WN-ARCH-010 | UI关闭、WebView刷新或未订阅事件时，Proxy故障任务仍按Rust状态继续执行。 |

## 5. 与现有代码的关系

当前实现已经具有以下基础：

- `domain::RuleAction::Delay`
- `domain::RuleAction::CustomHttpStatus`
- `domain::TerminalAction::RejectTlsHandshake`
- `domain::TerminalAction::DisconnectBeforeUpstream`
- `domain::TerminalAction::UpstreamConnectTimeout`
- `domain::TerminalAction::UpstreamWriteTimeout`
- `domain::TerminalAction::UpstreamReadTimeout`
- `domain::TerminalAction::DropUpstreamResponse`
- `domain::TerminalAction::MockResponse`
- `domain::TerminalAction::InvalidJson`
- `domain::TerminalAction::IncorrectContentLength`
- `domain::TerminalAction::TruncateResponse`
- `proxy::FaultAction`
- `proxy::fault::apply_response_actions`
- `proxy::transport::PipelinePorts`
- `proxy::transport::HyperUpstreamConnector`

必须保留现有分层：

```text
故障模板
  -> FaultConfigurationDraft
  -> 普通 RuleDraft
  -> Rule匹配与CAS命中提交
  -> proxy::FaultAction
  -> transport执行
```

不得创建：

```text
第二套WeakNetworkRule
第二套命中计数器
第二套规则持久化表
第二套优先级系统
第二套事件执行器
```

允许增加独立的传输执行器，例如 `PacedBody`、`TrafficSchedule` 和 `FaultExecutionContext`，但它们只负责执行已经由普通规则引擎决定的动作。

## 6. Rust领域模型

### 6.1 新增规则动作

建议扩展现有 `RuleAction`：

```rust
pub enum RuleAction {
    SetJsonField {
        path: String,
        value: serde_json::Value,
    },
    ReplaceBodyText(String),
    SetHeader {
        name: String,
        value: String,
    },
    Delay {
        milliseconds: u64,
    },
    Jitter {
        minimum_milliseconds: u64,
        maximum_milliseconds: u64,
        scope: JitterScope,
    },
    Throttle {
        bytes_per_second: u64,
        chunk_bytes: usize,
        direction: TrafficDirection,
    },
    Intermittent {
        available_milliseconds: u64,
        blocked_milliseconds: u64,
        direction: TrafficDirection,
    },
    Pause,
    CustomHttpStatus {
        status: u16,
    },
    Terminal(TerminalAction),
}
```

### 6.2 方向

```rust
pub enum TrafficDirection {
    Upstream,
    Downstream,
}
```

在本项目中的精确定义：

| 方向 | 定义 |
| --- | --- |
| `Upstream` | Proxy向GMO-FG Server发送请求Body。 |
| `Downstream` | Proxy向Payment App或DLL发送响应Body。 |

这不代表：

- `Upstream`能够减慢App到Proxy之间已经完成的TCP读取。
- `Downstream`能够控制Server到Proxy之间已经完成的TCP读取。

如需在匹配业务内容之前就限制原始入站流，必须另行设计只基于连接身份和通道的传输层规则。本期不将其与可解析业务规则混合。

### 6.3 抖动作用域

```rust
pub enum JitterScope {
    BeforeMessage,
    PerChunk,
}
```

| 作用域 | 行为 |
| --- | --- |
| `BeforeMessage` | 整条请求或响应发送前随机等待一次。 |
| `PerChunk` | 每个限速分块发送前随机等待。 |

如果没有Throttle动作，`PerChunk`使用系统默认分块大小，不允许退化为逐字节计时。

### 6.4 新增终止动作

建议扩展 `TerminalAction`：

```rust
pub enum TerminalAction {
    RejectTlsHandshake,
    DisconnectBeforeUpstream,
    UpstreamConnectTimeout {
        milliseconds: u64,
    },
    UpstreamWriteTimeout {
        milliseconds: u64,
    },
    UpstreamReadTimeout {
        milliseconds: u64,
    },
    DropUpstreamResponse {
        mode: DropResponseMode,
    },
    DisconnectDuringUpstreamWrite {
        after_bytes: u64,
    },
    DisconnectDuringDownstreamWrite {
        after_bytes: u64,
    },
    MockResponse {
        status: u16,
        headers: Vec<(String, String)>,
        shift_jis_body: Vec<u8>,
    },
    InvalidJson {
        shift_jis_body: Vec<u8>,
    },
    IncorrectContentLength {
        delta: i64,
    },
    TruncateResponse {
        bytes: u64,
    },
}
```

### 6.5 参数范围

Rust必须统一校验：

| 参数 | 最小值 | 最大值 | 说明 |
| --- | ---: | ---: | --- |
| 固定延迟 | 0 ms | 600,000 ms | 所有组合延迟总和仍不得超过600秒。 |
| 抖动下限 | 0 ms | 600,000 ms | 必须小于等于上限。 |
| 抖动上限 | 0 ms | 600,000 ms | 与固定延迟合计校验。 |
| 限速 | 1 B/s | 100 MiB/s | 0不得表示无限制；不启用时不创建动作。 |
| 分块大小 | 1 B | 1 MiB | 默认16 KiB。 |
| 可用窗口 | 1 ms | 600,000 ms | 与阻断窗口均必须大于0。 |
| 阻断窗口 | 1 ms | 600,000 ms | 不允许两个窗口同时为0。 |
| 中途断开字节数 | 0 | Body长度-1 | Body已知后再次执行运行时校验。 |
| 第N次命中 | 1 | `u64::MAX` | 0非法。 |
| 故障持续时间 | 1 ms | 600,000 ms | 超时后按动作定义结束或关闭。 |

所有字段错误通过Rust返回稳定字段路径，例如：

```text
actions[1].maximum_milliseconds
actions[2].bytes_per_second
actions[2].chunk_bytes
actions[3].blocked_milliseconds
```

## 7. 故障执行上下文

每次规则命中后，Rust创建只存在于当前会话的执行上下文：

```rust
pub struct FaultExecutionContext {
    pub runtime_epoch: RuntimeEpoch,
    pub connection_id: ConnectionId,
    pub session_id: SessionId,
    pub message_id: MessageId,
    pub rule_id: RuleId,
    pub rule_revision: Revision,
    pub hit_number: u64,
    pub terminal_identity: TerminalIdentity,
    pub channel: ChannelKind,
    pub stage: MessageStage,
    pub started_at: DateTime<Utc>,
    pub deterministic_seed: u64,
}
```

确定性随机种子必须由Rust根据以下稳定输入生成：

```text
runtime_epoch
+ rule_id
+ rule_revision
+ session_id
+ hit_number
```

要求：

- 不使用前端提供的随机数。
- 不使用不可记录的线程本地随机状态。
- 同一个测试输入和种子可在单元测试中重放。
- 审计日志记录种子，但不得记录敏感Payload。

## 8. 动作执行顺序

普通规则仍按优先级和创建顺序评估。单条规则内部动作按配置顺序执行。

### 8.1 可组合动作

以下动作可以组合：

- 修改JSON。
- 修改Header。
- 固定延迟。
- 抖动。
- 限速。
- 间歇暂停。
- 暂停断点。

示例：

```text
修改请求字段
-> 延迟2秒
-> 上行限速1KiB/s
-> 转发Server
```

### 8.2 终止动作

以下动作命中后终止后续动作和低优先级规则：

- TLS握手拒绝。
- 请求前断开。
- 模拟连接/读写超时。
- 发送上游后丢弃响应。
- 上行或下行中途断开。
- Mock响应。
- 非法JSON响应。
- 错误Content-Length。
- 截断响应。

规则保存时Rust必须拒绝：

- 终止动作之后仍存在其他动作。
- 同一规则包含两个终止动作。
- 请求阶段使用仅响应阶段可执行的动作。
- 响应阶段使用仅上游请求阶段可执行的动作。
- TLS握手阶段使用需要HTTP路径或Body的条件。
- `PerChunk`抖动没有可用分块策略。
- 上行Throttle配置在响应阶段。
- 下行Throttle配置在请求阶段且没有Mock响应。

## 9. 请求与响应管线

### 9.1 请求管线

```text
接收App TLS连接
  -> 验证客户端证书
  -> 读取HTTP请求与Body限制
  -> 创建Session和Request Message
  -> Shift-JIS解码与JSON解析
  -> 普通规则匹配
  -> CAS提交命中次数与一次性状态
  -> 执行请求阶段修改/等待
  -> 执行请求终止动作，或建立上游mTLS
  -> 以可控分块Body向Server写入
  -> 等待Server响应
```

### 9.2 响应管线

```text
读取Server HTTP响应
  -> 创建Response Message
  -> Shift-JIS解码与JSON解析
  -> 提取业务返回码
  -> 普通规则匹配
  -> CAS提交命中次数与一次性状态
  -> 执行响应修改/等待
  -> 以可控分块Body向App写入
  -> 或执行丢弃/截断/中途断开
  -> 完成Session终态
```

### 9.3 当前Full Body实现的改造

当前上游请求使用 `http_body_util::Full`，响应也主要在完整收集后返回。固定延迟、Mock、完整响应丢弃和静态截断可以继续工作，但限速、分块抖动、间歇暂停和中途断开需要可控流式Body。

建议增加：

```text
src-tauri/crates/proxy/src/traffic/
├── mod.rs
├── schedule.rs
├── paced_body.rs
├── deterministic_rng.rs
└── metrics.rs
```

核心抽象：

```rust
pub struct TrafficSchedule {
    pub initial_delay: Duration,
    pub jitter: Option<JitterProfile>,
    pub throttle: Option<ThrottleProfile>,
    pub intermittent: Option<IntermittentProfile>,
    pub disconnect_after_bytes: Option<u64>,
}

pub struct PacedBody {
    source: bytes::Bytes,
    cursor: usize,
    schedule: TrafficSchedule,
    cancellation: CancellationToken,
    metrics: Arc<FaultMetrics>,
}
```

`PacedBody`实现Hyper所需Body接口，按计划产生Data Frame。不得：

- 在单次poll中同步等待。
- 使用阻塞sleep。
- 为每个字节创建Timer。
- 在取消后继续产生Frame。
- 在Body被drop后保留后台任务。

### 9.4 限速算法

推荐使用单调时钟上的字节预算算法：

```text
理论发送时间 = 累计已发送字节 / bytes_per_second
本次最早发送时刻 = 开始时刻 + 理论发送时间
```

行为要求：

- 使用Tokio单调时间，不使用墙上时间计算速率。
- 发送块大小默认16 KiB，可由Rust规范化。
- 如果目标速率低于单块速率要求，等待到预算恢复。
- 不因任务调度延迟累计额外突发流量。
- 取消后立即结束。
- 记录实际发送字节、实际持续时间和平均速率。

不得简单实现为：

```text
每发送一个chunk固定sleep
```

因为固定sleep会把执行开销额外叠加，造成持续速率漂移。

### 9.5 抖动算法

每次等待值必须位于闭区间：

```text
[minimum_milliseconds, maximum_milliseconds]
```

要求：

- 使用确定性种子。
- 最大值等于最小值时退化为固定延迟。
- `BeforeMessage`只采样一次。
- `PerChunk`每块采样一次。
- 实际采样值记录到规则执行轨迹或聚合统计。
- 总等待受最大动作持续时间和Proxy取消控制。

### 9.6 间歇通断算法

以故障动作开始时间为零点循环：

```text
可用窗口 -> 阻断窗口 -> 可用窗口 -> 阻断窗口
```

要求：

- 可用窗口内允许发送。
- 进入阻断窗口后暂停产生新Body Frame。
- 已经交给内核的字节不追回。
- 阻断窗口结束后从原Body游标继续。
- Proxy停止、客户端断开或规则任务取消时立即退出。
- 不通过反复创建和销毁上游连接实现间歇通断。

## 10. 超时、断开与会话终态

### 10.1 超时语义

每种超时必须对应明确阶段：

| 动作 | 是否真实连接上游 | 是否写入请求 | 是否读取响应 | App侧表现 |
| --- | --- | --- | --- | --- |
| 上游连接超时 | 否 | 否 | 否 | 等待后连接关闭或Proxy错误响应，按模板定义。 |
| 上游写入超时 | 是 | 不完整或不交付 | 否 | 等待后连接关闭。 |
| 上游读取超时 | 是 | 是 | 不向管线交付 | 等待后连接关闭。 |
| 请求前延迟 | 延迟结束后是 | 是 | 是 | 整体响应变慢。 |
| 响应延迟 | 是 | 是 | 是 | Server已完成，但App迟迟收不到。 |

模拟超时不允许通过连接一个随机不可达公网IP实现；必须由Rust时钟和取消令牌确定性控制。

### 10.2 丢弃响应模式

沿用现有：

```rust
pub enum DropResponseMode {
    ReadCompleteResponse,
    CloseAfterRequestWrite,
}
```

精确定义：

| 模式 | 行为 | 用途 |
| --- | --- | --- |
| `ReadCompleteResponse` | 请求发送Server，读取完整响应并记录，但不返回App。 | 确认Server实际返回内容，同时模拟App未收到。 |
| `CloseAfterRequestWrite` | 请求写入后不等待完整响应，关闭或放弃上游及App方向。 | 模拟请求已可能到达Server但客户端连接丢失。 |

“是否出现D48”的自动验证只能使用 `ReadCompleteResponse`，因为另一模式不保证Proxy获得完整响应。

### 10.3 会话结果

扩展或明确 `SessionResult`：

```rust
pub enum SessionResult {
    Success,
    Mocked,
    RejectedTls,
    DisconnectedBeforeUpstream,
    UpstreamConnectTimeout,
    UpstreamWriteTimeout,
    UpstreamReadTimeout,
    ResponseDroppedAfterRead,
    ResponseAbandonedAfterRequest,
    UpstreamDisconnectedAfterBytes(u64),
    DownstreamDisconnectedAfterBytes(u64),
    ResponseTruncated(u64),
    FaultCancelled,
    ClientDisconnected,
    ProxyStopped,
    ResourceExhausted,
    InternalError,
}
```

不得把主动故障统一记录成 `InternalError`。

## 11. 生命周期与取消

每个故障等待和流式发送同时监听：

```text
Proxy根CancellationToken
连接CancellationToken
故障动作本地CancellationToken
客户端断开
上游断开
操作超时
```

### 11.1 停止Proxy

Proxy停止时：

1. 停止接受新连接。
2. 取消全部故障延迟和节流任务。
3. 停止产生新的Body Frame。
4. 关闭上下游连接。
5. 将未完成会话转换为 `ProxyStopped`。
6. 推送最终会话与故障状态事件。
7. 等待所有任务退出。

### 11.2 停用活动故障

停用规则只影响尚未命中的新报文。已经命中并进入执行阶段的动作采用以下默认语义：

- 固定延迟、抖动、限速和间歇动作：收到停用信号后取消，当前会话以 `FaultCancelled` 关闭，不自动无故障放行。
- 终止动作：一旦CAS命中提交成功，不可撤销。
- 已修改但尚未发送的报文：不得在停用后绕过剩余动作直接发送，避免出现未经重新校验的混合状态。

UI必须在停用确认中显示：

```text
停用只阻止后续命中；正在执行的可取消故障将终止对应连接，不会自动恢复放行。
```

## 12. 规则匹配与计数

### 12.1 计数维度

沿用主需求：

```text
终端IP + Payment客户端证书指纹
```

计数还必须绑定：

```text
rule_id + rule_revision + runtime_epoch
```

以下事件重置计数：

- Proxy重新启动。
- 规则关闭后重新启用。
- 匹配条件修改。
- 通道修改。
- 阶段修改。
- 规则复制后生成新RuleId。

仅修改描述不得改变匹配计数；实现时由领域层明确哪些字段构成匹配签名。

### 12.2 第N次命中

`NthHit(N)`表示该规则匹配条件成立的第N次，不等于底层TCP第N个包。

由于当前HTTP/1.1默认 `Connection: close`，一般是一条连接一个请求，但业务语义仍以“规则匹配次数”记录，不使用模糊的“第N个网络包”文案。

### 12.3 背景请求

Proxy能够按路径、请求类型和JSON字段进一步过滤，因此应避免仅按端口计数。如果无法解析业务类型，UI必须显示：

```text
当前规则只按通道和命中序号匹配；相同通道的额外请求可能改变第N次命中结果。
```

## 13. D48能力设计

### 13.1 模拟D48

D48模板必须使用Rust构造符合真实协议格式的响应。实现者不得假设D48一定是顶层固定JSON字段。

实施前必须从以下证据确定：

- Payment当前解析代码。
- 实机Server响应样本。
- Shift-JIS编码后的真实报文。
- HTTP状态、Header和业务字段结构。

模板配置至少包含：

```text
HTTP状态
返回码字段路径
返回码值 D48
其他必填业务字段
Shift-JIS编码
Content-Length重算
```

如果真实协议需要签名、MAC、流水号或请求字段回显，Rust必须根据当前请求生成，不能使用固定静态字符串冒充合法响应。

### 13.2 确认真Server返回D48

Rust响应解析结果增加：

```rust
pub struct BusinessResultViewModel {
    pub code: Option<String>,
    pub code_path: Option<String>,
    pub decoded: bool,
    pub parse_error_code: Option<String>,
}
```

规则测试或实机验证成功条件必须是：

```text
Proxy已收到Server完整响应
AND Shift-JIS严格解码成功
AND JSON/业务报文解析成功
AND 配置的返回码字段等于"D48"
```

以下证据不能单独判定D48成功：

- HTTP 200。
- 收到任意响应。
- Payment显示通用错误。
- 会话结束。
- 响应Body包含未定位字段的文本片段“D48”。
- Proxy丢弃响应后Payment进入自动取消。

### 13.3 审计记录

诊断日志不得保存完整敏感Payload，但可以保存：

```text
business_result_code=D48
business_result_path=$.resultCode
session_id
message_id
rule_id
terminal_identity_hash
```

字段路径必须来自实际协议配置，不得在实现前固定为示例中的 `$.resultCode`。

## 14. 预置故障模板

### 14.1 通用模板

| 模板ID | 阶段 | 精确行为 | 默认终态 |
| --- | --- | --- | --- |
| `disconnect_before_upstream` | 请求 | 不连接上游并断开App。 | `DisconnectedBeforeUpstream` |
| `delay_before_upstream` | 请求 | 转发前固定延迟。 | 正常或客户端先断开 |
| `upstream_connect_timeout` | 请求 | 等待连接超时但不真实连接。 | `UpstreamConnectTimeout` |
| `upstream_write_timeout` | 请求 | 建连后模拟写入超时。 | `UpstreamWriteTimeout` |
| `upstream_read_timeout` | 请求 | 请求已发送，模拟响应读取超时。 | `UpstreamReadTimeout` |
| `drop_response_after_read` | 请求 | 完整读取Server响应，不返回App。 | `ResponseDroppedAfterRead` |
| `abandon_after_request` | 请求 | 写完请求后放弃响应。 | `ResponseAbandonedAfterRequest` |
| `delay_response` | 响应 | 返回App前延迟。 | 正常或客户端先断开 |
| `throttle_upstream` | 请求 | Proxy向Server限速发送Body。 | 正常或超时 |
| `throttle_downstream` | 响应 | Proxy向App限速发送Body。 | 正常或断开 |
| `jitter_upstream` | 请求 | 请求发送前或分块抖动。 | 正常或超时 |
| `jitter_downstream` | 响应 | 响应发送前或分块抖动。 | 正常或超时 |
| `intermittent_upstream` | 请求 | 上行按时间窗暂停/恢复。 | 正常或超时 |
| `intermittent_downstream` | 响应 | 下行按时间窗暂停/恢复。 | 正常或超时 |
| `disconnect_upstream_mid_body` | 请求 | 上行发送N字节后断开。 | `UpstreamDisconnectedAfterBytes` |
| `disconnect_downstream_mid_body` | 响应 | 下行发送N字节后断开。 | `DownstreamDisconnectedAfterBytes` |
| `truncate_response` | 响应 | 发送前N字节后关闭。 | `ResponseTruncated` |
| `mock_shift_jis_json` | 请求 | 绕过Server并返回Mock。 | `Mocked` |
| `mock_d48` | 请求 | 绕过或按配置使用真实请求上下文返回D48。 | `Mocked` |

### 14.2 Payment业务场景映射

以下映射是候选模板，不得把业务结果写成固定承诺：

| 场景 | 推荐匹配 | 推荐动作 | 验证点 |
| --- | --- | --- | --- |
| 交易前网络不可用 | 交易通道、目标请求、第1次 | 请求前断开或连接超时 | Payment连接错误与恢复路径 |
| 授权响应丢失 | 授权请求、第1次 | 完整读取后丢弃响应 | Server实际结果、Payment超时、是否启动自动取消 |
| 自动取消响应丢失 | 自动取消请求 | 读取超时或丢弃响应 | Payment自动取消重试/终态 |
| IC Result响应丢失 | IC Result请求 | 完整读取后丢弃响应 | Payment后续状态和Server记录 |
| Advice响应丢失 | Advice请求 | 完整读取后丢弃响应 | Advice重试及终态 |
| DLL电子小票异常 | DLL通道、对应请求 | 延迟、超时或丢弃响应 | DLL错误返回及Payment处理 |
| Server返回D48 | 精确请求条件 | 不注入或Mock D48 | Proxy解析字段严格等于D48 |

“可能触发T03”“可能触发自动取消”等只能作为提示，最终行为必须通过真实Payment版本和真实设备验证。

## 15. UI设计

页面沿用现有“故障模拟”，只组合HeroUI官方组件，不自定义基础UI组件。

### 15.1 页面分区

#### A. 故障模板列表

显示：

- 模板名称。
- 阶段。
- 精确网络语义。
- 影响方向。
- 默认参数。
- 风险等级。
- 是否需要真实Server响应。
- 是否能自动确认业务码。

#### B. 配置面板

字段：

- 交易/DLL通道。
- 终端过滤。
- 路径或请求类型。
- JSON字段条件。
- 第N次命中。
- 一次性生效。
- 规则优先级。
- 固定延迟。
- 抖动下限/上限。
- 抖动作用域。
- 限速字节/秒。
- 分块大小。
- 可用窗口。
- 阻断窗口。
- 中途断开字节数。
- 丢弃响应模式。
- 故障最大持续时间。

Rust根据模板返回字段定义和可见性，前端不得自行推断某模板显示哪些字段。

#### C. 精确行为摘要

Rust实时校验后返回自然语言摘要，例如：

```text
第2次匹配交易通道授权请求时：
请求正常发送至GMO-FG Server；
Proxy读取完整响应并记录业务结果；
响应不返回Payment，随后关闭Payment连接；
该规则命中一次后自动停用。
```

#### D. 活动故障

显示：

- RuleId。
- 模板。
- 当前revision。
- 通道。
- 目标终端。
- 匹配条件。
- 第N次命中。
- 已匹配次数。
- 已实际执行次数。
- 当前正在执行会话数。
- 当前动作阶段。
- 已阻断/发送字节数。
- 实际平均速率。
- 最近命中时间。
- 状态。
- 停用按钮。

### 15.2 页面状态

必须覆盖：

- Proxy停止。
- Proxy启动中。
- Proxy运行。
- Proxy停止中。
- Proxy故障。
- 模板加载中。
- 模板加载失败。
- 没有活动故障。
- 配置校验失败。
- 当前规则被高优先级终止规则遮蔽。
- 活动故障正在执行。
- 停用正在提交。
- 事件游标失效，需要重新查询快照。

### 15.3 UI文案边界

UI不得显示：

```text
真实丢包率
Android设备已断网
Wi-Fi已断开
TCP包已随机丢弃
```

除非存在设备侧或网络侧证据。

应该显示：

```text
Proxy已暂停下行发送
Proxy未向Payment返回Server响应
Proxy在发送N字节后关闭连接
Proxy以目标速率X B/s向Payment发送
```

## 16. IPC设计

沿用现有Commands：

```text
fault_template_list
fault_configure
fault_active_list
fault_stop
```

建议增加：

```text
fault_validate
fault_get_execution_detail
fault_execution_query
```

### 16.1 `fault_validate`

输入：

```rust
pub struct FaultConfigurationDraft {
    pub template_id: String,
    pub expected_rule_revision: Option<Revision>,
    pub channel: ChannelKind,
    pub terminal_filter: Option<String>,
    pub path_or_request_type: Option<String>,
    pub conditions: Vec<RuleCondition>,
    pub nth_hit: u64,
    pub one_shot: bool,
    pub priority: u32,
    pub parameters: BTreeMap<String, FaultParameterValue>,
}
```

输出必须包含：

```rust
pub struct FaultValidationViewModel {
    pub normalized_draft: FaultConfigurationDraft,
    pub valid: bool,
    pub field_errors: Vec<FieldErrorViewModel>,
    pub warnings: Vec<WarningViewModel>,
    pub behavior_summary: String,
    pub generated_rule_preview: RuleDraftViewModel,
}
```

### 16.2 Channel事件

增加或扩展：

```text
FaultExecutionStarted
FaultExecutionProgress
FaultExecutionCompleted
FaultExecutionCancelled
```

进度事件必须批量或限频。默认：

- 每100毫秒最多推送一次同一执行实例的进度。
- 或累计变化超过64 KiB时推送。
- 以先到者为准。

每个事件携带：

```text
runtime_epoch
execution_id
session_id
message_id
rule_id
rule_revision
stage
state
sent_bytes
withheld_bytes
elapsed_milliseconds
sampled_delay_milliseconds
```

完整Payload不得放入Channel事件。

## 17. ViewModel设计

```rust
pub struct ActiveFaultViewModel {
    pub rule_id: RuleId,
    pub revision: Revision,
    pub template_id: String,
    pub template_name: String,
    pub channel_text: String,
    pub target_text: String,
    pub behavior_text: String,
    pub hit_count: u64,
    pub execution_count: u64,
    pub active_execution_count: u32,
    pub bytes_sent: u64,
    pub bytes_withheld: u64,
    pub average_bytes_per_second: Option<u64>,
    pub last_hit_at: Option<DateTime<Utc>>,
    pub state_text: String,
    pub tone: UiTone,
    pub can_stop: bool,
    pub stop_disabled_reason: Option<String>,
}
```

```rust
pub struct FaultExecutionDetailViewModel {
    pub execution_id: FaultExecutionId,
    pub runtime_epoch: RuntimeEpoch,
    pub session_id: SessionId,
    pub message_id: MessageId,
    pub rule_id: RuleId,
    pub terminal_text: String,
    pub channel_text: String,
    pub stage_text: String,
    pub action_text: String,
    pub state_text: String,
    pub deterministic_seed: u64,
    pub planned_duration_milliseconds: Option<u64>,
    pub elapsed_milliseconds: u64,
    pub sampled_delays: Vec<u64>,
    pub bytes_sent: u64,
    pub bytes_withheld: u64,
    pub result_code: Option<String>,
    pub error: Option<AppErrorViewModel>,
}
```

前端不得根据数值自行生成业务状态和错误原因。

## 18. 持久化与日志

### 18.1 SQLite

继续只持久化普通规则和故障模板生成关系：

- Rule。
- Rule revision。
- 模板ID。
- 模板参数。
- 命中次数。
- 是否启用。

不得持久化：

- 完整会话Payload。
- Body分块。
- Payment敏感字段。
- TLS密钥。

### 18.2 内存执行状态

活动执行实例只存在内存：

```text
FaultExecutionId -> FaultExecutionState
```

Proxy停止或应用退出时全部进入终态并释放。

### 18.3 结构化日志

允许记录：

```text
runtime_epoch
connection_id
session_id
message_id
rule_id
rule_revision
execution_id
stage
action_kind
sampled_delay
target_rate
sent_bytes
withheld_bytes
session_result
business_result_code
error_code
```

禁止记录：

- 完整请求/响应Body。
- 证书私钥。
- PKCS12密码。
- 终端敏感身份明文。
- 卡号、PIN、磁道和支付凭据。

## 19. 错误码

新增稳定错误码：

| 错误码 | 含义 |
| --- | --- |
| `FAULT_PROFILE_INVALID` | 故障参数组合非法。 |
| `FAULT_STAGE_INCOMPATIBLE` | 动作与规则阶段不兼容。 |
| `FAULT_DURATION_EXCEEDED` | 组合动作超过最大允许持续时间。 |
| `FAULT_RATE_INVALID` | 限速参数非法。 |
| `FAULT_JITTER_INVALID` | 抖动区间或作用域非法。 |
| `FAULT_INTERMITTENT_INVALID` | 间歇窗口非法。 |
| `FAULT_DISCONNECT_OFFSET_INVALID` | 中途断开字节位置非法。 |
| `FAULT_EXECUTION_NOT_FOUND` | 执行实例不存在或已淘汰。 |
| `FAULT_EXECUTION_CANCELLED` | 故障执行被停用、连接或Proxy取消。 |
| `FAULT_STREAM_ABORTED` | 分块发送期间连接异常终止。 |
| `BUSINESS_RESULT_NOT_FOUND` | 响应中没有找到配置的业务返回码字段。 |
| `BUSINESS_RESULT_MISMATCH` | 实际业务返回码与预期不一致。 |

错误ViewModel仍由Rust提供：

```text
稳定错误码
中文消息
字段错误
是否可重试
建议操作
关联ID
```

## 20. 测试设计

### 20.1 领域单元测试

必须覆盖：

- 每个新动作的参数边界。
- 抖动下限大于上限。
- 0 B/s限速非法。
- 0字节和超过Body长度的断开位置。
- 间歇窗口非法组合。
- 请求/响应阶段兼容性。
- 终止动作之后存在动作。
- 同一规则两个终止动作。
- 一次性规则CAS。
- 第N次命中与重置。
- 规则revision变化后的执行隔离。
- 确定性种子相同产生相同序列。
- 不同Session产生不同序列。

### 20.2 Tokio时间测试

使用暂停的Tokio时间验证：

- 固定延迟在精确时间前不继续。
- 取消立即终止延迟。
- 限速预算不超发。
- 调度延迟不产生额外突发。
- 分块抖动在范围内。
- 间歇阻断窗口不产生Frame。
- 恢复窗口从正确游标继续。
- Proxy停止时所有Timer退出。

测试不得依赖真实墙上等待70秒。

### 20.3 传输集成测试

使用本地TLS Server和测试App客户端验证：

1. 请求前断开时Server没有收到请求。
2. 完整读取后丢弃时Server收到请求，Proxy获得完整响应，App无响应。
3. 写完请求后放弃时Server可能收到请求，但Proxy不承诺获得响应。
4. 上游连接超时不发起真实外部网络连接。
5. 上游写入超时会关闭相关任务和连接。
6. 上游读取超时发生在请求写入成功之后。
7. 上行限速的Server接收时间符合容差。
8. 下行限速的App接收时间符合容差。
9. 下行中途断开只收到配置字节数。
10. 截断响应保持指定声明长度并提前关闭。
11. Shift-JIS Mock可无损编码。
12. 不可表示字符禁止发送。
13. D48真实响应能够按配置字段提取。
14. 仅Body中其他文本包含D48不算业务码命中。
15. 停止Proxy后没有残留发送任务。

### 20.4 IPC测试

覆盖：

- 模板字段定义。
- `fault_validate`规范化值。
- 字段错误路径。
- 生成的RuleDraft与模板一致。
- Command revision冲突。
- 事件顺序：

```text
FaultExecutionStarted
-> 零或多个FaultExecutionProgress
-> FaultExecutionCompleted或FaultExecutionCancelled
```

- 迟到事件带旧Epoch时前端丢弃。
- 事件队列溢出后发送 `SnapshotRequired`。

### 20.5 前端测试

只验证：

- 正确渲染Rust返回的模板和行为摘要。
- 正确发送用户输入。
- 字段错误显示到HeroUI字段。
- 活动故障进度正确替换ViewModel。
- 停用按钮pending去重。
- 小窗口配置面板可滚动到可见位置。
- 表格可横向滚动且操作按钮可访问。

不得在前端测试中重复实现Rust限速、随机、规则或业务码判断。

### 20.6 Windows与macOS测试

两平台必须运行同一组：

- Rust领域测试。
- Tokio时间测试。
- 本地mTLS传输测试。
- IPC测试。
- Next.js渲染测试。
- Tauri构建。
- 应用启动冒烟测试。

平台专项：

| Windows | macOS |
| --- | --- |
| Windows x64 CI构建。 | Apple Silicon CI或实机构建。 |
| 安装版和便携版启动。 | `.app`启动。 |
| DPAPI证书材料。 | Keychain证书材料。 |
| Windows防火墙提示与监听。 | macOS本地网络权限与监听。 |
| Edge/WebView2 UI验证。 | WKWebView UI验证。 |

弱网算法不得使用Windows专属网络API，以保证macOS行为一致。

### 20.7 真实设备验收

设备验收必须使用真实Payment版本，并保存以下证据：

- 设备序列号。
- Payment APK版本。
- Proxy版本和commit。
- 规则导出。
- Proxy日志。
- 会话ID。
- Server响应业务码。
- Payment界面或日志结果。
- 故障执行开始/完成事件。
- 测试开始和结束时间。

每个场景至少验证：

1. 无故障基线交易成功。
2. 故障规则只命中目标终端。
3. Server是否收到请求。
4. Proxy是否收到完整响应。
5. App是否收到任何响应字节。
6. Payment是否进入预期超时、自动取消或错误路径。
7. 停用规则后下一次交易恢复。
8. Proxy重启后命中计数按需求重置。

### 20.8 D48验收

D48成功必须同时满足：

```text
真实设备调用经过Proxy
AND Proxy会话完整
AND Server响应被Proxy完整读取
AND Shift-JIS解码成功
AND 业务报文解析成功
AND 指定返回码字段严格等于D48
AND 会话/日志保存对应证据
```

如果使用Mock D48，还必须分别标记：

```text
result_source=mock
```

真实Server返回则标记：

```text
result_source=upstream
```

二者不得混为同一验收结论。

## 21. 需求追踪矩阵

| 需求 | UI | Rust领域/应用 | Proxy执行 | IPC | 测试 |
| --- | --- | --- | --- | --- | --- |
| WN-001 流量必须经过Proxy | 设置/帮助 | Settings校验 | Listener | settings_* | 集成、实机 |
| WN-002 请求延迟 | 故障模拟 | RuleAction::Delay | request pipeline | fault_* | 领域、时间、集成 |
| WN-003 响应延迟 | 故障模拟 | RuleAction::Delay | response pipeline | fault_* | 领域、时间、集成 |
| WN-004 上行限速 | 故障模拟 | RuleAction::Throttle | PacedBody | fault_* / events | 时间、集成 |
| WN-005 下行限速 | 故障模拟 | RuleAction::Throttle | PacedBody | fault_* / events | 时间、集成 |
| WN-006 抖动 | 故障模拟 | RuleAction::Jitter | TrafficSchedule | fault_* / events | 确定性、时间 |
| WN-007 间歇通断 | 故障模拟 | RuleAction::Intermittent | TrafficSchedule | fault_* / events | 时间、集成 |
| WN-008 请求前断开 | 故障模拟 | TerminalAction | transport | fault_* | 集成、实机 |
| WN-009 上游超时 | 故障模拟 | TerminalAction | connector | fault_* | 时间、集成 |
| WN-010 丢弃响应 | 故障模拟 | DropResponseMode | connector/response | fault_* | 集成、实机 |
| WN-011 中途断开 | 故障模拟 | TerminalAction | PacedBody | fault_* / events | 集成 |
| WN-012 第N次命中 | 故障模拟/规则 | MatchCondition::NthHit | adapter | rule_*/fault_* | 领域、并发 |
| WN-013 一次性规则 | 故障模拟/规则 | Rule.one_shot | CAS事务 | rule_*/fault_* | 并发、IPC |
| WN-014 D48模拟 | 故障模拟 | MockResponse | codec/response | fault_* | codec、实机 |
| WN-015 D48确认 | 会话详情 | BusinessResult | response parse | session_* | codec、实机 |
| WN-016 取消与恢复 | 活动故障 | execution state | CancellationToken | fault_stop/events | 时间、实机 |
| WN-017 跨平台一致 | 无专属UI | 共享Rust | Tokio/Hyper | 相同IPC | Windows/macOS |

## 22. 推荐实施顺序

实现Agent应按以下顺序提交，每一步独立可测试：

1. 将本设计中的需求编号合并进 `docs/requirements.md` 和正式追踪矩阵。
2. 补齐领域模型、校验和错误码，不接传输层。
3. 为现有固定延迟、超时、丢弃、截断动作补齐会话终态。
4. 增加 `FaultExecutionContext`、确定性随机数和内存执行状态。
5. 实现 `TrafficSchedule` 和Tokio暂停时间单元测试。
6. 实现下行 `PacedBody`，先支持限速。
7. 为下行增加抖动、间歇暂停和中途断开。
8. 实现上行可控Body，并重复相同能力。
9. 将新动作映射进普通Rule和现有Fault模板。
10. 增加Rust校验、行为摘要和ViewModel。
11. 增加IPC Commands和限频Channel事件。
12. 更新故障模拟HeroUI页面。
13. 完成Windows和macOS本地集成测试。
14. 使用真实Payment设备完成无故障基线。
15. 验证请求前断开、响应丢弃、响应延迟和DLL通道。
16. 最后验证真实Server D48与Mock D48，并严格分开证据。

不得在领域校验、时间测试和传输测试未完成时直接以UI演示作为完成证据。

## 23. 完成定义

本功能只有满足以下条件才可标记完成：

- 不需要Android端VPN、root或额外APK。
- Payment和DLL通过Proxy完成无故障基线。
- 所有故障由普通规则生成并执行。
- 没有第二套规则或命中计数系统。
- 延迟、限速、抖动和间歇动作可取消。
- Proxy停止后没有残留任务或监听。
- Windows与macOS测试通过。
- UI只展示Rust ViewModel。
- 真实设备至少完成请求前断开、响应丢弃、响应延迟和DLL场景。
- D48结论具有字段级响应证据。
- Mock D48与真实Server D48明确区分。
- 文档、Rust类型、TypeScript生成绑定和追踪矩阵一致。

## 24. 非目标

本期明确不实现：

- Android `VpnService`。
- Android端Agent APK。
- Android root命令。
- `tc netem`、iptables、nftables。
- Windows网络过滤驱动。
- macOS Network Extension。
- 系统全局透明代理。
- 其他App的弱网控制。
- Wi-Fi、移动网络和DNS开关。
- IP包级乱序、复制或真实重传模拟。

如未来必须验证上述网络层行为，应建立独立设备/路由器弱网测试方案，不得将其悄悄塞入应用层Proxy规则语义。
