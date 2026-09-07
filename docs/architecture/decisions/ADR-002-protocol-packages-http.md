# ADR-002：HTTP 与 Socket 使用独立协议包，共享字段处理模型

- Status: Superseded by [ADR-009](ADR-009-nested-document-javascript-package-runtime.md) on 2026-08-31
- Supersedes: 2026-08-17 Socket-only decision
- Refined by: [ADR-007](ADR-007-exchange-pipeline-runtime-boundary.md)
- Date: 2026-08-19
- Scope: R12 protocol platform

> 当前递归 Document、两写出阶段和 JavaScript package API 1 合同由 ADR-009 替代。下方内容只保留
> 原始决策语境，不再指导生产实现。

## Context

HTTP 与 Socket 都需要把线上报文转换为可匹配、可修改的字段集合，再重建报文并生成安全展示；但二者
不能共用同一个包实例。HTTP 拥有 method、URI、Header、Status、CONNECT/MITM 与文本 Body 语义，
Socket 拥有字节分帧、half-close、转发或本机应答语义。共享执行模型不等于混合数据平面。

## Decision

- HTTP 与 Socket 是两类独立协议包，包的 `kind` 是不可变身份的一部分，不能交叉绑定。
- 两类包共享 `Document` 字段类型、双方向 Schema、四阶段字段规则、Decode/Encode/Display host API、
  安全展示和包安装生命周期。
- Manifest 都使用 `document.upstream/downstream` 与 `hooks.upstream/downstream`；Socket 两个方向还必须
  同时声明 `frame`。不声明 `content_types`，不使用 request/response 后缀函数名。
- HTTP 包只处理非空 UTF-8 Body。Header、Status、CONNECT、MITM 和普通 HTTP 规则仍由 HTTP 数据面
  拥有；普通 HTTP 规则先执行，随后 Body 进入 Decode、两个边界规则、条件性 Encode 和 Display。
- Socket 转发和本机应答继续由 Socket runtime 拥有；两类 runtime 不互相导入协议专属 DTO。

## Alternatives

- Rejected：HTTP 与 Socket 完全复制包管理、Schema 与规则引擎；会产生两套不可维护的字段语义。
- Rejected：一个协议包同时服务 HTTP 与 Socket；会混合分帧、文本 Body 和传输生命周期。
- Rejected：按 Content-Type 自动路由多个 HTTP 包；当前单入口精确绑定一个包更可预测。
- Accepted：独立包 kind 与 runtime，共享中立的字段处理模型和注册生命周期。

## Consequences

- HTTP 与 Socket 的入口绑定、编辑字段、运行时和失败诊断语义保持隔离；规则页与协议包列表可以统一展示，
  但每条数据必须保留明确的 `kind`，不得因此允许跨类型绑定。
- 字段规则可以使用同一领域模型，但必须绑定入口、精确包版本、方向 Schema 和四阶段之一。
- HTTP Body 未变化时逐字节保留 Body，并保持 `Content-Length` 的语义数值一致；
  HTTP 库可规范化 Header 的空白和序列化形式。Body 变化时只重建一次并更新 `Content-Length`。
- HTTP 二进制 Body、内容编码解压、按 Content-Type 多包路由需要新的 ADR，不能作为隐式兜底加入。

## Open items

- 二进制 Body、内容编码解压与按 Content-Type 多包路由留给 future ADR；本决策不提供兼容或自动探测路径。
