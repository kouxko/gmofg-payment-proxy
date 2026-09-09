# 修复 HTTP Exchange 非 UTF-8 Body 预览乱码

- 任务 ID：TASK-20260909-001
- 状态：已完成
- 优先级：高；修改跨 crate 观测合同，需保护字节与容量边界。
- 任务日期：2026-09-09
- 创建时间：2026-09-09 11:58:49 +08:00
- 开始时间：2026-09-09 11:58:49 +08:00
- 最后更新时间：2026-09-09 12:33:05 +08:00
- 完成时间：2026-09-09 12:33:05 +08:00
- 创建路径：`docs/tasks/pending/2026-09-09/fix-http-body-charset-preview.md`
- 归档路径：`docs/tasks/completed/2026-09-09/fix-http-body-charset-preview.md`
- 关键词：HTTP、Shift_JIS、Exchange、Body、charset、观测预算
- 分支：`hotfix/fix-http-body-charset-preview`
- 基线：远端 `gitlab/develop`，`1c88aeb8`；独立工作区避免覆盖原工作区未提交 QV 档案。

## 背景、目标与范围

用户截图中 Server → Proxy 响应声明 `text/csv;charset=Shift_JIS`，Body 日文显示替换字符，Original Decode Document 日文正常。用户授权修复，并要求基于 develop 新建 hotfix 分支。
目标：修复该非 UTF-8 Body 的观测预览，保留网络字节、业务 Decode/Rules/Encode 与 UTF-8 规则草稿门禁。
范围：Exchange tracing 传递非 UTF-8 HTTP 原始正文；桌面观测 consumer 复用既有字符集解析与 codec 生成展示文本；相关回归和文档。
不在范围：修改协议包、Listener 配置、业务错误/编码合同、持久化、安装、推送、PR、CI、发布；不扩展 charset 支持集。

## 需求确认记录与就绪检查

- 用户：能处理则基于 develop 新建 hotfix 分支处理此问题。
- 既有权威：`body_codecs/content_type.rs` 支持 UTF-8、Shift-JIS 及现有别名；`HttpContext.wire_body` 为权威字节；`exchange_mock.rs` 禁止非 UTF-8 body 生成规则草稿。
- 修复仅覆盖当前 lossy HTTP 观测投影，原本 UTF-8 投影保持；纯文本缺失 charset、不支持 charset 与 Raw 保留原有观测行为；JSON/XML 的默认 UTF-8 沿用既有 Auto 合同，不猜测编码。
- 识别到支持的编码但字节非法时必须显式保持失败语义，不允许把替换字符当成功解码；不得影响转发结果。
- 示例：Shift_JIS 编码的 `売上票（署名）保管正常完了。` 应在 Body 中正确显示，原始字节不变且 `body_is_utf8=false`。
- 问题、目标、范围、输入输出、错误边界、示例、可判定验收：全部就绪；未确认事项：无。
- 进入实现时间：2026-09-09 11:58:49 +08:00

## 根因分析

- 现象/环境：用户截图，HTTP 200，Shift_JIS 日文 Body 乱码，下方 Document 正常。
- 当前已验证：`proxy/http/exchange_runtime/endpoints.rs::message_context` 用 `from_utf8_lossy` 生成 body；`ObservedContext` 只投影文字，未传 HTTP 原字节；前端直接展示 context.body；external package 使用原始字节与 codec 独立解码。
- 历史已验证：TASK-20260907-002 电子小票包本地 Shift-JIS 验证；本轮不据此宣称真实服务已验收。
- 候选原因：字体问题由相同页面 Document 日文正常排除；服务报文字节损坏无直接证据；错误字符集投影由当前源码确认。
- 根因/因果链：Shift_JIS 原字节 → UTF-8 lossy 字符串 → tracing 只保留有损文字 → Body 直接显示。
- 复现：用明确 Shift_JIS 日文原字节驱动观测 producer/consumer，断言展示文本与 byte/flag；用户真实链路不可从截图重放。
- 推断：截图运行版本使用同一投影实现；未知：该次完整原始响应与终端业务结果。
- 修复边界：业务 HttpContext 不变，在只读观测链路传字节并正确投影。

## 最小改动与最优设计

| 方案 | 分析 |
| --- | --- |
| 直接改业务 context.body | 行数少但可能影响 Plain JSON 与规则行为，不采用 |
| tracing 原始字节 + consumer 复用 codec | 保持业务依赖和所有权；使用已有 hex 与编码能力，无新依赖/新配置/双业务路径；采用 |
| 新增 PipelinePorts 预览端口及独立 DTO | 扩大公共接口和所有 context 构造器；此 hotfix 无需采用 |

## 小任务与验证计划

| ID | 内容 | 依赖 | 可并行 | 负责人 | 状态 | 验收 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | tracing 字节与预算回归 | 固定观测合同 | 是 | 子 Agent | 已完成 | received/encoded/sent/failed 字节及总预算正确 |
| 2 | consumer 预览与 codec 复用 | 同一合同 | 是 | 主 Agent | 已完成 | Shift-JIS 正常、门禁不变、错误可见 |
| 3 | 集成、文档、独立审查、归档 | 1、2 | 否 | 主 Agent | 已完成 | 回归与静态检查通过，APPROVE |

并行合同：`ObservedContext::Http` 暴露 `wire_body`；非 UTF-8 HTTP 在现有 `context_bytes_hex` 上携带原始 Body hex，UTF-8 保持空字段；所有阶段预算包含 hex。子 Agent 只拥有 exchange crate，主 Agent 拥有 infrastructure、桌面 consumer、文档。共享合同变化必须先上报，禁止相互覆盖。该批全部终态并集成后才做整体审查。
测试：RED/GREEN 日文回归，未知/缺失/非法编码，原文字段与 wire bytes/flag不变，四观测阶段与预算，consumer 到 Store，现有 Mock 拒绝门禁；Rust fmt/Clippy、受影响测试与前端 Body 组件回归。协议证据保存测试输入字节、预期日文与实际 tracing/DTO；网络/设备/截图为 N/A（此次只改观测，真实环境未复测）。
整体对抗审查：高优先级，独立 Agent；解决范围内 P0/P1/P2 后归档。小任务单独审查不执行，使用集成后的整体审查。

## 文档影响

- 需要更新：运行时观测架构中的 lossy Body 说明；本任务、完成与证据索引。
- 无需更新：协议包/规则合同、数据库/配置、用户操作流程、CI/发布文档（未修改这些行为）。
- Skill：rust-router、m06-error-handling、domain-web；仅用于 Rust 边界和错误语义检查，不替代项目任务/证据/审查门禁。

## 实施、验收与完成记录

- 2026-09-09 12:23:41 +08:00：实现及集成检查完成；独立整体审查进行中。
- Exchange producer RED 两例失败，consumer RED 两例失败，均复现错误字符集投影；最终测试 PASS。
- 69 项受影响测试：Exchange 33、body codec 6、Mock 1、consumer 21、前端 Body 8。
- 严格 Clippy（4 个受影响 crate/all-targets）、Rust fmt、TypeScript、ESLint、架构边界、架构文档、diff 检查全部 PASS。测试期间源码 SHA-256 无变化。
- 新增临时 emit 宏在集成时改为 tracing 现有 `Option<Value>` 支持，保持 UTF-8 空字段且减少重复；已用 producer 全回归复验。
- 中间检查：独立工作区缺 APK build-script 前置资源，复用字节相同的已有归档后恢复；测试辅助代码的 AppError 字段访问、hex 字符串拼接 lint 已修正。日志完整保留。
- macOS debug lib-test 链接器提示 `__eh_frame section too large`，测试通过；这不是本次应用发布构建，未隐藏告警。
- [body-charset 测试与复测证据](../../../testing/evidence/2026-09-09/TASK-20260909-001/body-charset/README.md)。所有实际输入、预期、DTO 输出、比较、源码快照与复测入口均位于该目录。
- 文档：已更新运行时观测架构、Rust/生成 TS 注释；类型字段无变化，无新增依赖。其余文档无需更新，理由见文档影响。
- CI、推送、PR、安装、发布、用户真实服务/Windows：NOT_RUN，未授权且不属于本次源码修复。
- 独立工作区：`/Users/codin/Code/gmofg-payment-proxy-hotfix-body-charset`，原工作区 QV 未提交内容保留。

### 修改文件

- `src-tauri/crates/application/src/facade/rules/exchange_mock.rs`
- `src-tauri/crates/application/src/models/exchange_observation.rs`
- `src-tauri/crates/exchange/src/observation.rs`
- `src-tauri/crates/exchange/src/observation/failure.rs`
- `src-tauri/crates/exchange/src/observation/tests.rs`
- `src-tauri/crates/exchange/src/protocol.rs`
- `src-tauri/crates/infrastructure/src/adapters/body_codecs.rs`
- `src-tauri/crates/infrastructure/src/adapters/body_codecs/exchange_preview.rs`
- `src-tauri/crates/infrastructure/src/adapters/mod.rs`
- `src-tauri/crates/infrastructure/src/lib.rs`
- `src-tauri/src/runtime_logs/exchange_ui_layer.rs`
- `src-tauri/src/runtime_logs/exchange_ui_layer/tests.rs`
- `src-tauri/src/runtime_logs/exchange_ui_layer/tests/fixtures/receipt-shift-jis.bin`
- `src-tauri/src/runtime_logs/exchange_ui_layer/tests/http_body.rs`
- `src/features/capture/exchange-observation-detail.test.tsx`
- `src/generated/rust-types.ts`

另包含运行时观测文档、本任务和测试证据及对应索引。### 完成总结

- 2026-09-09 12:33:05 +08:00：整体独立审查 APPROVE，P0/P1/P2 = 0/0/0；[审查报告](../../../testing/evidence/2026-09-09/TASK-20260909-001/body-charset/outputs/review.md)。
- 已完成日期归档、待实现入口移除、完成/测试索引与相对链接检查。
- 验收：本地源码修复 PASS；69 项受影响测试与严格静态检查全部通过，所有输入/结果/复测资源齐全。
- 剩余限制：仅修复非 UTF-8 有损观测，按 Header 而非 Listener 强制 codec；已存在 UTF-8 投影不变。未安装新 App 或复测用户真实服务，需要重建并产生新请求生效。
- 交付：本地 hotfix 分支；不 Push，不创建 PR，不触发 CI。
