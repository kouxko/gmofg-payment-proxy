# ADR-007：Exchange/Pipeline 运行与观察边界

- Status: Superseded by [ADR-009](ADR-009-nested-document-javascript-package-runtime.md) on 2026-08-31
- 日期：2026-08-24
- Supersedes: [ADR-006](ADR-006-unified-exchange-observation.md)
- Refines: [ADR-001](ADR-001-http-socket-boundary.md)、[ADR-002](ADR-002-protocol-packages-http.md)、[ADR-004](ADR-004-embedded-read-only-mcp.md)、[ADR-005](ADR-005-runtime-evidence-and-reproduction-report.md)

> 当前递归 Document、两写出阶段、统一规则事务和 JavaScript package API 1 合同由 ADR-009 替代。
> 下方内容只保留原始连接级 Exchange 决策语境。

## Context

ADR-006 把 `Exchange` 定义为观察聚合根，并引入 `Flow`、`Interaction`、revision snapshot 和 SQLite
运行报文表。后续实现与逐项评审确定了更小的连接级运行模型：网络交易由外部 poll，观察是旁路事实，
HTTP 与 Socket 只共享协议中立核心。历史设计必须保留，但不能继续指导生产实现。

## Decision

1. 一个 accepted App connection 对应一个 `Exchange<P>`。协议模式直接持有 upstream/downstream 两个
   `Pipeline<P, D>`，不定义 `Flow` 或 `Interaction`。外部 accept loop 只调用 `Exchange::exchange()`。
2. Reader Pipeline 固定生成不可变 `Envelope`：HTTP 为 Decode -> Display；Socket 为
   Frame -> Decode -> Display。Writer Pipeline 固定为 clone Document -> Rules -> Encode -> Writer；
   Rules 不修改 Reader 产生的 context、document 或 display。
3. 协议模式严格按 App read -> Server write -> Server read -> App write 推进。Server 不主动发送消息；
   同一连接处理下一条消息时追加新的观察事件，不覆盖已有证据。
4. `LocalServer` 是同一 Server port 的进程内精确 Echo，仍完整经过 downstream Pipeline。
5. 透明转发只属于 `Exchange<Socket>`：两方向原始字节并发转发并传播 half-close。当前 HTTP
   CONNECT/Upgrade 在创建 Server connection 和 Exchange 前返回 501，不提供隐式 tunnel 或 MITM 兜底。
6. Exchange 阶段事件经 tracing 投影进入 Infrastructure 的有界内存 store，再由 Tauri
   `exchange_ui_layer` 非阻塞发布刷新。队列、投影、存储或 UI 发布失败均 fail-open，不改变交易结果。
   Exchange payload 不写入 SQLite。
7. 进程 `RuntimeLogStore` 是独立的有界 JSONL 诊断日志。当前 reproduction report 组合 Application
   diagnostic report 与 RuntimeLogStore，不直接聚合 Exchange observation store；三者不得混写为同一生命周期。
8. 本决策不改变协议包 wire、Manifest、Rhai Host API 或扁平 Document。`domain::document` 保持协议中立；
   Rhai 名称限制属于 `protocol-scripting`；HTTP 与 Socket 的规则绑定继续隔离。

当时的完整类型和流程约束记录在
[Exchange/Pipeline 历史架构模板](../exchange-pipeline-template/README.md)；当前实现不得以其替代 ADR-009。

## Boundary evidence

- 核心：`src-tauri/crates/exchange/src/exchange.rs`、`src-tauri/crates/exchange/src/pipeline.rs`、
  `src-tauri/crates/exchange/src/envelope.rs`、`src-tauri/crates/exchange/src/transparent.rs`。
- 生产装配：`src-tauri/crates/proxy/src/socket_relay/protocol_exchange.rs`、
  `src-tauri/crates/proxy/src/socket_relay/raw_exchange.rs`、`src-tauri/crates/proxy/src/http/exchange_runtime.rs`。
- 观察：`src-tauri/crates/infrastructure/src/adapters/exchange_observation.rs`、
  `src-tauri/src/runtime_logs/exchange_ui_layer.rs`、`src-tauri/src/runtime_logs/store.rs`。
- 回归：`src-tauri/crates/exchange/src/tests`、`src-tauri/crates/proxy/src/socket_relay/tests`、
  `src-tauri/crates/infrastructure/src/adapters/exchange_observation/tests.rs`、
  `src-tauri/src/runtime_logs/exchange_ui_layer/tests.rs`。

## Consequences

- 协议顺序和非法组合由 Rust 类型与 focused tests 约束，HTTP/Socket 不共享 transport DTO。
- 运行记录在进程退出后消失；容量淘汰与观察丢失必须显式可见。
- 如未来支持 HTTP CONNECT/Upgrade、主动 Server push、持久化 Exchange payload 或让 reproduction report
  聚合 observation，必须先新增或修订 ADR，并补齐相应边界与端到端测试。

## Alternatives

- Rejected：继续使用 ADR-006 的统一观察聚合根和 SQLite snapshot。它把旁路观察变成交易状态所有者。
- Rejected：用兼容 adapter 保留旧 Flow/Interaction。当前仍在开发阶段，没有消费者证明其价值。
- Rejected：让观察失败中断交易。抓包和 UI 可用性不能成为网络业务成功的前置条件。

## Open items

- future HTTP CONNECT/Upgrade、主动 Server push 或持久化 Exchange payload 均需要独立设计与测试，
  当前实现不预留 fallback。
