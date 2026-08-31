# ADR-009：递归 Document、统一规则与 JavaScript 协议包运行时

- Status: Accepted
- 日期：2026-08-31
- Supersedes: [ADR-002](ADR-002-protocol-packages-http.md)、[ADR-007](ADR-007-exchange-pipeline-runtime-boundary.md)

## Context

ADR-002 和 ADR-007 固化了协议边界与连接级 Exchange 所有权，但其扁平字段、四个规则阶段和
Rhai 包假设已被后续实现替代。继续把这些历史描述当作当前合同，会让编辑器、运行时、MCP 与包作者
依赖不同的数据模型。

## Decision

1. 协议中立数据模型是递归 JSON `Document`，支持 null、boolean、number、string、object 和 array；
   Schema 描述递归能力，规则本地叶子仍由其类型化 condition/action 携带，不另建持久化字段。
2. HTTP 与 Socket 共享 `RuleDefinition`、递归 AND/OR 条件树和有序 action。每个方向只在写出边界
   执行一次规则事务：`Proxy -> Server` 与 `Proxy -> App`。
3. 每个方向开始时创建私有 working Document。规则按确定顺序执行，每条 condition 读取当前 working
   state；命中 action 立即更新它并对后序规则可见。方向完成后只 Encode 一次，成功才提交
   NthHit/one-shot；失败或 Encode 失败回滚 Document 与 actor 生命周期。
4. 本地包是严格 ZIP，根目录使用 `manifest.json`、`protocol.js` 和 `display.js`。独立 Boa Sidecar
   是唯一 JavaScript 执行 owner，不提供进程内、Rhai、Deno 或兼容别名路径。当前 host 不注入
   Node、文件系统、process、Buffer、fetch、timer 或 WebSocket bindings；这不是对 Boa 原生能力的
   一般性限制声明。
5. 本地和远端包都通过 Proxy 拥有的 `/packages` WebSocket 及 API 1 固定 RPC 接入。包主动发送
   `package.register`；Frame/Decode/Encode 使用字节 wire，Document 与 Display 使用类型化 JSON/text。
6. Exchange observation 记录 received Document、类型化 operation summary、final working Document、
   Encode/result 和稳定错误；达到共享观测预算时设置 `changes_truncated`，业务继续且最终输出不受影响。
7. SQLite Schema 100 是产品 1.00 兼容基线。开发启动重建仅是 Phase17 删除前的当前开发边界，
   不能写成发布兼容或数据迁移策略。

## Why

- 两个写出阶段与网络提交点一致，避免 Reader 阶段持有可变规则状态。
- 当前 working state 按序匹配、立即 action 更新与一次提交把 Document 和 NthHit 生命周期放进同一事务。
- Rust capability/factory 是唯一能力所有者，UI 和文档不再复制 predicate/action 默认。
- 单一 API 1 Sidecar 路径让本地 ZIP 与远端进程共享失败、预算、生命周期和观测合同。

## Consequences

- HTTP/Socket transport 仍隔离；共享的是 Document、统一规则与包调用合同。
- 数组 Schema 的 `items` 是类型模板，不代表用户已经创建 index 0。
- Display 是不可信观测输出；Frame、Decode、Rules、Encode 和 Writer 失败仍 fail-closed。
- 历史 ADR 保留原文，只把状态和后继关系标为已替代。

## Alternatives

- Rejected：保留四阶段并把两阶段作为 UI 别名；会形成双路径并继续错置事务边界。
- Rejected：继续扁平字段并把 object/array 存成 blob；会丢失递归 Schema 与条件树语义。
- Rejected：同时保留 Rhai、进程内 JavaScript 或 Deno 默认；没有兼容需求，且会产生多个运行 owner。
