# ADR-001：HTTP 与 Socket 只共享中立内核

- Status: Accepted
- Date: 2026-08-17
- Scope: R01 architecture baseline

## Context

HTTP 与 Socket 都需要 Listener、TCP/TLS、取消、事件和页面外壳，但 HTTP 拥有 parser、CONNECT、MITM、
Header/Status；Socket 拥有 Frame、half-close、双向独立处理和 LocalResponder。用一个大量 nullable 字段的
模型统一二者，会让非法组合变得可表达。

## Decision

接受“共享中立设施、隔离协议语义”：可共享 Listener lifecycle、transport/TLS 原语、分页、事件、package
registry、Schema/Document 值对象和 UI shell。HTTP 与 Socket 保持独立 data plane、错误、规则和 capture
证据模型。依赖不得从中立 transport/runtime 反向指向 application/UI。

## Alternatives

- Rejected：完全分离，包括重复 transport/lifecycle。它会复制资源所有权与关闭语义。
- Rejected：统一万能 pipeline/DTO。它会把 CONNECT/Header/Status 与 Frame/LocalResponder 混在同一模型。
- Accepted：中立内核 + 两个协议专属 data plane。

## Consequences

- 跨平面复用必须先证明抽取内容不含 HTTP 或 Socket 词汇/DTO。
- UI 可共享导航、Tab 和布局，但协议内容条件挂载并消费各自 Rust ViewModel。
- `scripts/check-architecture-boundaries.mjs` 对四条依赖禁令 fail-closed。

## Open items

- 完整规则、capture 和 Android lifecycle 图按追踪矩阵回填。Owner: R04、R07e、R10。
