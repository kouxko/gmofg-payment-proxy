# Intercept Proxy 架构文档

状态：当前实现基线（2026-08-31）。本目录以源码为事实来源，描述已经落地的模块边界、
Exchange/Pipeline 模型、HTTP/Socket 数据流和验证逻辑。历史讨论稿不属于当前架构契约。

第一次接手项目时，先阅读[新人接手与项目全景指南](../onboarding-guide.md)，完成首次运行和术语学习后，
再按下面顺序深入架构。

## 阅读顺序

1. [模块与代码组织](modules.md)：Rust crate、Tauri 组合根和前端 feature 的所有权。
2. [Exchange 与 Pipeline](exchange-pipeline.md)：协议无关核心、强类型方向和可替换能力。
3. [数据流、错误与验证](data-flow.md)：真实连接如何按顺序转发、观察和失败。
4. [规则、Document 与协议包](rules-and-protocol-packages.md)：统一规则聚合、Schema 与 Sidecar 包。
5. [运行时观测与诊断](runtime-observability.md)：内存证据、日志、UI 实时刷新、MCP 和复现报告。
6. [安全、TLS 与持久化](security-and-persistence.md)：双连接 TLS/mTLS、SQLite 与应用导入导出。
7. [Android VPN 透明路由](android-vpn-transparent-routing.md)：设备侧 TUN、SOCKS5 与流量恢复。
8. [开发与维护指南](development-guide.md)：代码放置、测试层级、门禁和排障入口。

```mermaid
flowchart LR
    MODULES[模块与依赖] --> EXCHANGE[Exchange 与 Pipeline]
    EXCHANGE --> FLOW[真实数据流]
    FLOW --> RULES[规则与协议包]
    RULES --> OBSERVE[观测与诊断]
    OBSERVE --> VERIFY[验证与测试锚点]
```

## 架构结论

- Rust 是业务真相源；Next.js 只负责展示 ViewModel、收集操作意图和维护纯视觉状态。
- `domain` 保存纯领域不变量，`application` 编排用例，`infrastructure` 实现端口并装配
  持久化和系统能力，`proxy` 处理 HTTP/Socket/TLS transport。
- `exchange` 是连接级数据交换核心，只依赖 `domain`，不依赖 Tauri、SQLite、Sidecar 或具体网络库。
- 每个已接受的 App connection 对应一个 `Exchange<P>`；协议模式严格执行一问一答，
  Socket 透明模式只转发真实读取到的字节。
- HTTP 与 Socket 共享 `Exchange/Pipeline/Envelope` 模型，但各自保留不同 Context、Reader、
  Frame 和 transport 语义。
- LocalServer 与 RemoteServer 实现同一个 Server 端口；本地回环不是第二套旁路流程。
- 观察链路与业务链路隔离：观察失败不能影响交易，业务阶段失败必须终止当前 Exchange。
- HTTP 与 Socket 共用 `RuleDefinition`、`Proxy -> Server` / `Proxy -> App` 两个写出阶段、单 Listener
  绑定、单一持久化集合和 CRUD；
  内容差异由 `RuleContent::Http` 与 `RuleContent::Socket` 保持类型隔离。
- 规则编辑能力由 Rust `rule_editor_context` 提供，领域保存时再次校验；前端不复制能力矩阵。

## 代码入口

| 目标 | 权威入口 |
| --- | --- |
| 桌面组合根 | `src-tauri/src/lib.rs` |
| 无 UI Host 组合根 | `src-tauri/crates/host/src/lib.rs` |
| 用例门面 | `src-tauri/crates/application/src/facade.rs` |
| 领域模型 | `src-tauri/crates/domain/src/lib.rs` |
| Exchange 核心 | `src-tauri/crates/exchange/src/lib.rs` |
| HTTP/Socket/TLS runtime | `src-tauri/crates/proxy/src/lib.rs` |
| 系统与持久化适配器 | `src-tauri/crates/infrastructure/src/lib.rs` |
| 协议包合同与严格 ZIP | `src-tauri/crates/package-contract/src/lib.rs`、`src-tauri/crates/package-runtime/src/lib.rs` |
| 前端持久外壳 | `src/features/shell/app-runtime.tsx` |
| Rust 生成的 IPC 类型 | `src/generated/rust-types.ts` |

## 维护规则

- 新业务判断先放进 Rust，并通过生成的 IPC 类型暴露；不要在前端复制校验。
- 新协议阶段必须实现 `exchange` 已有 capability trait；不要创建第二套组合处理器。
- 新 transport 适配器只实现 Reader、Writer、Connection 或 Server；不要拥有交易顺序状态机。
- 修改依赖方向、Exchange 顺序、规则能力或错误语义时，必须同步对应详细文档及相应测试。
- 运行 `pnpm scan:architecture` 检查架构边界和文档链接，运行 `pnpm check` 执行完整质量门禁。

## 决策与验证

- 当前递归 Document、两写出阶段和 JavaScript package API 1 边界以
  [ADR-009](decisions/ADR-009-nested-document-javascript-package-runtime.md) 为准。
- [ADR-002](decisions/ADR-002-protocol-packages-http.md)、
  [ADR-006](decisions/ADR-006-unified-exchange-observation.md) 和
  [ADR-007](decisions/ADR-007-exchange-pipeline-runtime-boundary.md) 已被替代，仅保留历史原因和被否决方案。
- 全部 ADR 入口见 [文档总览](../README.md#架构决策)。
- 固定验收合同见 [发布级验证矩阵](../testing/release-validation-matrix.md)。
- 最新 release App 测试用例与结果见 [2026-08-25 App 测试结果](../testing/release-validation-results-20260825.md)；
  完整真实 App 与外部包证据见 [2026-08-24 发布验证结果](../testing/release-validation-results-20260824.md)。
