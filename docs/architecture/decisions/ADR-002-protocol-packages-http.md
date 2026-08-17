# ADR-002：当前协议包 ABI 不扩展到 HTTP

- Status: Accepted
- Rejected option: HTTP Body 复用 Socket Document ABI
- Date: 2026-08-17
- Scope: R01 architecture baseline

## Context

当前协议包的 Frame/Decode/Encode/Display 以原始 Socket 字节、方向和每连接上下文为契约。HTTP 已有
method/URI/header/status/body、CONNECT/MITM 和内容编码语义。两者仅因都可能有“body/document”而统一，
会丢失 HTTP 的结构和失败边界。

## Decision

协议包当前只服务 Socket Scripted/LocalResponder。HTTP 不导入 Socket package runtime/contracts，也不把
HTTP Body 映射为现有 Document。package registry 和 Schema/Document 值对象可以保持中立，但 ABI 与执行器
必须按协议独立。

## Alternatives

- Rejected：HTTP 与 Socket 完全各自复制 package management；registry/identity/安装生命周期值得共享。
- Rejected：共享管理但立即增加 HTTP ABI；R01 没有已批准的 HTTP 用户场景与行为测试。
- Rejected：HTTP Body 直接使用 Socket Document；无法表达 Header/Status/streaming/content encoding。
- Accepted：保持 Socket-only ABI；未来以新 ADR 和独立 HTTP ABI 重新评估。

## Consequences

- 任何 HTTP package 提案必须先给出用户场景、独立 ABI、迁移与 HTTP 回归证据。
- 当前协议包 ZIP、编译缓存和 host API 不得被描述为通用 HTTP 扩展平台。

## Open items

- 若出现已批准的 HTTP package 用户场景，另立 ADR 和独立 ABI，不修改当前 Socket ABI。Owner: R01 follow-up。
