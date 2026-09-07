# 集中复核：REQUEST CHANGES

审查者：`/root/integrated_architecture_review`。本轮源码只读复核，前一轮的事务 P1 和两项去重 P2 已关闭；未发现其他新的实质 P0/P1/P2，但仍有一项 P1。

- `external_relay/capabilities.rs` 的 `PackageFrameResult::Reject` 使用无 code 的 Error::new。实际协议包 frame Reject 最终丢失 FRAME_REJECTED，转换为 ProcessingFailed/FrameProcess。
- 修复边界：该产生点使用既有 FRAME_REJECTED、Frame stage 和 D::KIND；新增上/下行 capability 回归，并验证 runtime 最终 FrameRejected -> FrameInspect / ServerToClient 分类。保持拒绝行为，不解析错误文本。
- 已关闭：提交前 Socket empty/max-output 校验与唯一 validator、HTTP/Joint 唯一 package_error、Raw/Protocol 唯一 remote endpoint 准备；此前 typed-error 修复与架构文档一致。

全量测试随后完成：1634 passed / 3 failed，其中两项仍检查旧错误文本前缀，需按本任务已经冻结的结构化错误合同更新断言；另一个未修改 MCP 平台接口 HTTP 超时。严格 Clippy 的唯一失败是扩展 Socket scenario 超过 100 行，拆分测试构造函数后复验。这些失败均保留于本目录的完整日志。

Local HTTP Q1 仍未实现，本审查不代表任务整体完成。
