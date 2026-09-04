# TASK-20260904-005：修复分块 HTTP 响应转发时的长度头冲突

- 任务 ID：`TASK-20260904-005`
- 状态：`已完成`
- 任务日期：`2026-09-04`
- 创建时间：`2026-09-04 17:33:01 +08:00`
- 开始时间：`2026-09-04 17:33:01 +08:00`
- 最后更新时间：`2026-09-04 18:13:45 +08:00`
- 完成时间：`2026-09-04 18:13:45 +08:00`
- 创建路径：`docs/tasks/pending/2026-09-04/fix-http-chunked-response-forwarding.md`
- 归档路径：`docs/tasks/completed/2026-09-04/fix-http-chunked-response-forwarding.md`
- 关键词：`HTTP/1.1`、`Keep-Alive`、`Transfer-Encoding`、`Content-Length`、`chunked`、`404`、`user sent unexpected header`
- 任务优先级：`高`
- 优先级理由：问题位于 HTTP 报文分帧公共合同；失败会把已收到的真实业务响应误报为代理内部错误并中止客户端连接。

## 背景、目标与需求确认

- 背景：真实 Exchange `0e49990e528e414d9b4c7184ddbdc846` 中，App 使用 `Connection: Keep-Alive`，上游成功返回带 `Transfer-Encoding: chunked` 的 HTTP 404 及业务 Body；代理随后记录 `IO_ERROR HTTP/1.1 connection failed: user sent unexpected header`。
- 目标：代理保留上游 `Transfer-Encoding: chunked` 语义，由 Hyper 为已处理 Body 重新生成合法 chunk framing，不再添加 `Content-Length`；将 404 状态和完整业务 Body 正常转发给 App，不产生代理内部连接错误。
- 范围：HTTP `Message` 长度头不变量、下游响应构造、真实形状的回归测试、协议数据流文档和测试证据。
- 不在范围：把业务 404 改为成功、忽略真实网络错误、改变规则处理结果、增加重试、修改上游服务、强制覆盖上游明确返回的 `Connection: close`。
- 需求确认记录：`2026-09-04 17:27:43 +08:00` 用户提供 Exchange 与运行日志截图并明确要求 Keep-Alive 场景不能误报错；`2026-09-04 17:30:00 +08:00` 用户补充上游 404/分块响应及 `Content-Length + Transfer-Encoding` 冲突分析；`2026-09-04 17:54:00 +08:00` 用户明确选择保留 chunked、有 chunked 时不设置 Content-Length，不把分块响应统一改为固定长度。
- 未确认事项：零；本任务只修复协议头冲突，真实业务 404 继续按原样透传。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`
- 范围与不在范围：`PASS`
- 输入、输出和状态变化：`PASS`；输入为已去除原 chunk framing 的 Body 加原 `Transfer-Encoding: chunked`，输出仍只保留该长度表达，由 Hyper 重新分块，不添加 Content-Length。
- 错误行为：`PASS`；真实解析、读写、规则或连接错误继续失败，不把失败改成成功。
- 具体示例：`PASS`；上游 `HTTP/1.1 404`、`Transfer-Encoding: chunked`、`Connection: close` 和 JSON Body 应转发为 404 与同一 Body，且不出现 `user sent unexpected header`。
- 可重复 PASS/FAIL 验收：`PASS`；真实 TCP 上游回归在修复前复现连接失败，修复后验证响应状态、唯一 chunked 长度头、实际 chunk wire、Body 和连接终态。
- 改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-09-04 17:33:01 +08:00`

## 问题与根因分析

- 实际现象：代理已记录上游 404 响应和 Body，但向 App 写响应时产生 `IO_ERROR`，同一 Exchange 标记异常结束。
- 预期行为：404 是上游业务响应，不是代理传输失败；代理应发送合法响应并按响应连接语义正常结束。
- 最小复现：真实 TCP 上游发送包含 `Transfer-Encoding: chunked`、无 `Content-Length` 的分块响应，经 `HyperUpstreamConnector` 读取、Pipeline 处理和 `response_from_disposition` 写入 App 侧 Hyper HTTP/1.1 Server 连接。
- 当前已验证：`response_from_disposition` 在未发现 `Content-Length` 时调用 `set_content_length(body.len())`，但 `set_content_length` 只移除旧 `Content-Length`，不会移除 `Transfer-Encoding`。
- 当前已验证：最终交给 Hyper 的 Header 同时包含 `Transfer-Encoding` 与 `Content-Length`；本机当前 Hyper 1.11 源码明确将该组合归类为 `user sent unexpected header`。
- 当前已验证：上游请求、响应读取和业务 404 已完成，失败发生在 Proxy 向 App 编码/发送响应的阶段。
- 推断：所有无 `Content-Length` 的分块上游响应都可能触发同一故障，不局限于截图中的接口或 404 状态。
- 未知：线上其他接口是否也返回相同组合；不影响公共协议根因。
- 候选原因排除：TLS、客户端证书和上游连接失败已由成功收到完整 404 响应排除；Keep-Alive Header 本身由现有多请求连接测试覆盖，不是触发 Hyper 错误的非法头。
- 已确认根因：代理在 Hyper 已去除 chunk framing、保留原 `Transfer-Encoding` 元数据后再次补 `Content-Length`，没有先清除互斥的分帧头。
- 影响范围：普通分块上游响应，以及规则改写分块响应 Body 后重新发送的路径；无 Transfer-Encoding 的固定长度响应和显式 Content-Length 故障注入保持原合同。

## 最小改动与最优设计

| 方案 | 分析 |
| --- | --- |
| 删除 Transfer-Encoding、统一改为 Content-Length | 对单一 chunked 可用，但会改变原响应分帧语义；对复合 transfer-coding 还可能丢失必要编码声明，不采用。 |
| 只在最终响应构造时跳过 Content-Length | 普通透传可修复，但规则修改 Body 时 `replace_body` 仍会提前添加 Content-Length，存在遗漏，不采用。 |
| 保留 Transfer-Encoding，固定长度 setter 保持互斥不变量 | 响应构造和 Body 替换见到 Transfer-Encoding 都不添加 Content-Length，由 Hyper 重新分块；显式选择 Content-Length 时仍移除 Transfer-Encoding，职责完整，采用。 |

## 小任务、测试与审查

| ID | 内容 | 状态 | 验收 |
| --- | --- | --- | --- |
| T01 | 添加真实 TCP 404/chunked 响应端到端回归 | 已完成 | 修复前稳定得到连接错误，修复后收到完整分块响应 |
| T02 | 保留 Transfer-Encoding 并阻止自动添加 Content-Length | 已完成 | 普通与 Body 改写分块响应均只使用 chunked |
| T03 | 运行 Proxy 定向与全量测试、fmt、Clippy | 已完成 | 236/236 与静态检查 PASS |
| T04 | 对抗审查、归档任务和证据 | 已完成 | 最终复审 APPROVED，P0/P1/P2=0 |

测试计划：先增加端到端失败回归并保存 RED；再验证 404、Body、保留 `Transfer-Encoding: chunked`、不存在 `Content-Length`、Hyper 重新生成实际 chunk wire、连接无 `IO_ERROR`；增加规则修改 Body 后仍保留 chunked 的单元回归，并运行 Proxy 全量测试、格式、严格 Clippy 和差异检查。

文档影响：在 HTTP 数据流中明确 Hyper 去除原 chunk framing 后，若仍有 Transfer-Encoding，则由下游 Hyper 重新生成 framing 且不添加 Content-Length；不改变外部 API 或持久化 Schema。

对抗审查计划：检查响应原始 Header 展示与实际发送 Header 是否一致、故障注入的错误长度语义是否保留、Keep-Alive/Connection close 是否仍按现有合同工作、真实错误是否仍可观测。

## 实施记录、修改文件与验收结果

- `2026-09-04 17:33:01 +08:00`：完成历史、截图、运行日志、当前源码和 Hyper 1.11 本机源码核对，确认根因并登记高优先级任务。
- `2026-09-04 17:41:00 +08:00`：首版采用“删除 Transfer-Encoding、设置准确 Content-Length”，focused 与 Proxy 230 项通过；独立审查指出复合 transfer-coding 可能被静默破坏且测试未经过真实上游读取链，结论 `REQUEST CHANGES`。
- `2026-09-04 17:54:00 +08:00`：用户明确选择保留 chunked；修订为有 Transfer-Encoding 时不添加 Content-Length，Body 被规则修改时同样保持 chunked。
- `2026-09-04 17:57:38 +08:00`：回归升级为真实 TCP 上游实际发送 chunk frames，经 HyperUpstreamConnector 到 App；focused、Proxy 231/231、fmt、严格 Clippy 和 diff check 全部通过，等待独立复审。
- `2026-09-04 18:13:45 +08:00`：根据复审补齐上游 TE+CL 归一化、`gzip, chunked + CustomStatus`、chunked 截断无结束块、`Transfer-Encoding: gzip` + EOF 向下游 `gzip, chunked` 重分帧四个边界；Proxy 236/236、fmt、严格 Clippy、ESLint 和 diff check 通过，最终独立复审 APPROVED，P0/P1/P2=0。

修改文件：`src-tauri/crates/proxy/src/message.rs`、`src-tauri/crates/proxy/src/message/tests.rs`、`src-tauri/crates/proxy/src/fault/response.rs`、`src-tauri/crates/proxy/src/http/helpers.rs`、`src-tauri/crates/proxy/src/http/wire.rs`、`src-tauri/crates/proxy/tests/fault_semantics.rs`、`src-tauri/crates/proxy/tests/raw_http_proxy/support.rs`、`src-tauri/crates/proxy/tests/raw_http_proxy/lifecycle.rs`、`src-tauri/crates/proxy/tests/raw_http_proxy/capacity_and_faults.rs`、`docs/architecture/data-flow.md`、本任务与测试证据。

附加文件：[HTTP-CHUNKED-RESPONSE-001](../../../testing/evidence/2026-09-04/TASK-20260904-005/HTTP-CHUNKED-RESPONSE-001/README.md)。

- 验收结果：`PASS`
- CI、push、发布：本地协议验收已完成；GitHub push 与 Android/Windows 快速构建由 `TASK-20260904-006` 在后续独立执行。

完成总结：分块响应不再人工添加 Content-Length；代理保留现有 transfer-coding，必要时追加 chunked 使 canonical Header 与 Hyper 实际 wire 一致。404 及业务 Body 仍原样转发，没有固定返回或默认成功。
