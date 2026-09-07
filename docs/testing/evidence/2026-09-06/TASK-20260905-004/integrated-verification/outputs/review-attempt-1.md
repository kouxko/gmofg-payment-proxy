# 独立集成审查：REQUEST CHANGES

审查者：原生 code-reviewer `/root/integrated_architecture_review`。只读审查已经完成的独立实现范围；Local HTTP 默认响应 Q1 不在本轮实现范围。

1. P1：Socket `BoundedPrepare` 在 Joint Encode 的 lifecycle commit / hit 发布之后检查 empty/max-output。编码返回空或超限时，未发送网络数据但规则计数已提交。要求将同一输出校验移入 actor 持有的 Encode 事务，在提交之前执行；回归需断言 commit_attempts=0、hit_count=0、生命周期不变，合法输出仅提交一次。
2. P2：HTTP external_http 与 joint_document/error 的 external_rpc_error 字段转换完全重复。要求同一个 listener_runtime helper。
3. P2：Socket Raw/Protocol 各自复制 Remote connect、typed Connect 错误、preparation failure 和 opened evidence。要求共享连接准备，保留各自 I/O 包装和 half-close/配对。

此前审查中的 typed error 丢字段已在审查结束前修复：Socket actor/default PipelinePorts 保留 code/stage/direction/external detail；HTTP policy 标 Rules；factory failure/panic 标 CapabilityFactory；TextEncode 使用 HTTP_DOCUMENT_INVALID typed code。未使用独立 LSP 工具；类型证据来自 Cargo check。

后续：本次 REQUEST CHANGES 保留为历史审查结果，三项修复与稳定验证后再集中复核。未修改 MCP 的非 loopback HTTP timeout 原因未知，不据此归因于本次代理变更。
