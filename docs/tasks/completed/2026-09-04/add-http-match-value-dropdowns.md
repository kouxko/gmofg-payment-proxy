# TASK-20260904-002：简化规则来源并为 HTTP Method 提供常用值下拉

- 任务 ID：`TASK-20260904-002`
- 状态：`已完成`
- 任务日期：`2026-09-04`
- 创建时间：`2026-09-04 11:53:10 +08:00`
- 开始时间：`2026-09-04 11:53:10 +08:00`
- 最后更新时间：`2026-09-04 12:51:07 +08:00`
- 完成时间：`2026-09-04 12:51:07 +08:00`
- 创建路径：`docs/tasks/pending/2026-09-04/add-http-match-value-dropdowns.md`
- 归档路径：`docs/tasks/completed/2026-09-04/add-http-match-value-dropdowns.md`
- 关键词：`规则页面`、`HTTP`、`Method`、`Header`、`通用动作`、`SetJsonField`、`SetHeader`、`MockResponse`、`ReplaceBodyText`、`Select`、`能力合同`
- 任务优先级：`高`
- 优先级理由：用户追加移除 Header 匹配和通用动作入口，涉及 Rust 声明的规则编辑能力与前端消费合同；底层旧类型继续兼容已保存数据，需执行跨层回归。

## 背景、目标与需求确认

用户最初希望 Method 和 Header 使用有限、易懂的常用值下拉，随后明确 Header 不需要参与规则匹配、“通用”动作没有必要存在，并要求收敛 HTTP 动作及使用中文名称。

- 目标：Method 匹配值提供 `GET`、`POST`、`PUT`、`PATCH`、`DELETE`；HTTP 匹配字段不再提供 Header；HTTP 动作来源仅提供 HTTP、Document，Socket 动作来源仅提供 Document；HTTP 动作不再提供 SetJsonField、SetHeader、MockResponse，并全部使用中文显示名称。
- 范围：Rust 规则编辑能力、HTTP 条件编辑器、HTTP/Socket 动作来源、抓包响应生成规则草稿、对应跨层回归、用户操作说明和本地 App 验证。
- 不在范围：删除底层 Header/RecordMatch/SetJsonField/SetHeader/MockResponse 类型、迁移或删除既有规则、HTTP 匹配操作符、自定义候选配置。
- 需求确认记录：`2026-09-04 11:53:10 +08:00` 用户明确要求 Method 和 Header 提供有限集合的下拉框，且“不需要太专业”。
- 需求变更记录：`2026-09-04 11:55:58 +08:00` 用户明确 Header 不再进行匹配、不放入规则，并继续要求移除“通用”动作入口；原 Header 下拉验收失效。
- 需求变更记录：`2026-09-04 12:00:27 +08:00` 用户要求减少 HTTP 动作：SetJsonField 与 Document Set 重复，SetHeader 随 Header 能力移除；LocalHttpServer 配合 Proxy → App 的 ReplaceBodyText 已覆盖手工 MockResponse 用途，因此 MockResponse 也不再作为可选动作。HTTP 动作名称统一中文显示。
- 需求变更记录：`2026-09-04 12:27:51 +08:00` 用户要求 Document Schema 条件路径过滤不能进行条件匹配的节点；条件路径下拉和值类型下拉只展示 Rust 能力中 `predicates` 非空的节点/类型，Document 动作路径仍保留容器节点。
- 需求一致性补充：`2026-09-04 12:38:40 +08:00` 复核发现抓包页仍生成旧 MockResponse 草稿，与用户确认的 LocalHttpServer + ReplaceBodyText 模型冲突；该入口改为生成 Proxy → App ReplaceBodyText，并在按钮/说明中明确只复用服务器响应 Body、需搭配 LocalHttpServer。
- 未确认事项：零；底层旧类型仅保留反序列化与运行兼容，不再由新建/编辑能力提供选择入口。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`
- 范围与不在范围：`PASS`
- 输入、输出和状态变化：`PASS`；选择 Method 时匹配值改为有限下拉；能力列表不再返回 Header 匹配、RecordMatch 通用动作、SetJsonField、SetHeader 或 MockResponse；现有持久化类型不删除。
- 错误行为：`PASS`；未选择候选时保持不可保存，不生成默认值。
- 具体示例：`PASS`；选择 `Method` 后可选择 `POST` 并保存为现有 Method/Equals 合同；匹配字段列表没有 Header，动作来源列表没有“通用”，动作类型以中文显示且不包含“设置 JSON 字段”“设置 Header”“模拟响应”；`/GBRD_01/*` 若为 Object/Array 且无谓词能力，不出现在 Schema 条件路径下拉中，但仍可按动作能力出现在动作路径中。
- 可重复 PASS/FAIL 验收：`PASS`
- 改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-09-04 11:53:10 +08:00`；最终需求变更重新通过门禁时间：`2026-09-04 12:38:40 +08:00`

## 问题与根因分析

- 实际现象：Method 的匹配值为自由文本；Rust 能力仍暴露 Header 匹配、RecordMatch、SetJsonField、SetHeader 和 MockResponse；前端动作名称为英文；Document 条件与动作共用完整 Schema 字段列表，导致 `predicates` 为空的 Object/Array 节点也出现在条件路径下拉中。
- 预期依据：用户本次明确要求有限、易懂的下拉集合。
- 最小复现：规则页新建 HTTP 规则，进入“匹配条件”打开 HTTP 匹配字段；进入“执行动作”打开动作来源。
- 当前已验证：`RuleSinglePairEditor` 对所有 HTTP 字段统一渲染文本“HTTP 匹配值”；Rust stage capability 同时暴露上述匹配/动作；`RULE_ACTION_LABELS` 使用英文硬编码。
- 当前已验证：ReplaceBodyText 修改当前阶段正在传递的报文 Body；MockResponse 是终止动作。用户当前采用 LocalHttpServer，由 Proxy → App ReplaceBodyText 完成本机响应替换，不需要手工 MockResponse 入口。
- 已确认根因：Method 值未做字段级控件差异化；不再需要的能力仍存在于 Rust 权威编辑集合；动作显示名称未本地化；前端没有按 Rust 返回的 `predicates` 能力区分条件路径与动作路径候选。
- 推断：无。
- 未知：无影响实现方向的未知。
- 影响范围：Application 规则编辑能力与抓包草稿工厂、规则/抓包 UI、Application/UI 测试和操作说明；Domain 持久化与运行时实现保持不变。

## 最小改动与最优设计

| 方案 | 分析 |
| --- | --- |
| 在 HTTP 条件表单内按 Method 渲染有限 Select | 不改 Method wire/Rust 匹配合同，状态仍复用现有 `value`，采用。 |
| 仅在前端过滤 Header 与通用动作 | 会让前端复制并覆盖 Rust 权威能力，拒绝。 |
| 从 Rust 编辑能力集合移除 Header、RecordMatch、SetJsonField、SetHeader 与 MockResponse | 新建/编辑入口按同一能力合同收敛，底层类型继续兼容既有数据，采用。 |
| 抓包入口继续生成旧 MockResponse | 会立即生成编辑器不再支持的草稿，拒绝。改为 Proxy → App ReplaceBodyText，只保留 Body，并明确依赖 LocalHttpServer。 |
| 使用可编辑 ComboBox | 会继续允许集合外值，与“有限集合”目标不一致，拒绝。 |

## 小任务、测试与审查

| ID | 内容 | 状态 | 验收 |
| --- | --- | --- | --- |
| T01 | 增加 Method 匹配值有限下拉 | 已完成 | 候选完整、切换字段清空旧值且不自动默认 |
| T02 | 从 Rust 编辑能力移除 Header、RecordMatch 和三个重复 HTTP 动作 | 已完成 | 对应能力不再暴露，底层旧数据类型保留 |
| T03 | 将保留的 HTTP 动作名称改为中文 | 已完成 | 每个能力项都有明确中文名称 |
| T04 | 增加 Application/UI 回归并更新说明 | 已完成 | Method 按现有 wire 保存，被移除入口不可见 |
| T05 | 构建、安装和实际界面验证 | 已完成 | 正式 App 构建、签名、安装、哈希和进程通过；点击式自动化因连接超时记为 NOT_RUN |
| T06 | 过滤不可匹配的 Document 条件候选 | 已完成 | 条件 Schema 路径和值类型不出现 `predicates` 为空的节点/类型，动作候选不受影响 |
| T07 | 将抓包 Mock 草稿统一为 LocalHttpServer 响应 Body 替换 | 已完成 | 草稿阶段为 Proxy → App，动作为 ReplaceBodyText，不再生成 MockResponse/status/Header |

测试计划：先补 Application 能力合同和规则编辑器失败测试，再运行相关 Rust crate、规则编辑器定向 Vitest、完整 UI 合同、typecheck、lint、前端/Tauri 构建、`git diff --check`；构建后安装本地 App 验证候选和被移除入口。高优先级任务执行跨层差异审查。

## 实施记录、修改文件与验收结果

- `2026-09-04 11:53:10 +08:00`：核对现有 Method 精确匹配和 Header `/name` 合同，登记任务并确定有限候选。
- `2026-09-04 11:55:58 +08:00`：用户撤销 Header 下拉并要求 Header 不进入规则，同时移除“通用”动作；任务升为高优先级并重新通过需求就绪门禁。
- `2026-09-04 12:00:27 +08:00`：用户确认 LocalHttpServer + Proxy → App ReplaceBodyText 覆盖手工 MockResponse，用同一需求变更移除 SetJsonField、SetHeader、MockResponse 并中文化剩余动作名称。
- `2026-09-04 12:27:51 +08:00`：用户指出条件路径仍展示不可匹配节点；确认以前端消费 Rust `predicates` 能力过滤条件候选，动作路径保持完整能力。
- `2026-09-04 12:38:40 +08:00`：复核抓包生成规则入口，发现其仍创建旧 MockResponse；纳入同一任务改为 LocalHttpServer 下行 Body 替换草稿，避免入口生成编辑器已禁止的新规则。
- `2026-09-04 12:51:07 +08:00`：完成能力收敛、条件候选过滤、抓包 Body 替换草稿、文档与跨层回归；正式 macOS App 重新构建、签名并覆盖安装。

修改文件：

- `src-tauri/crates/application/src/facade/rule_capabilities.rs`
- `src-tauri/crates/application/src/facade/unified_rule_editor.rs`
- `src-tauri/crates/application/src/facade/rules/exchange_mock.rs`
- `src-tauri/crates/application/src/requirements_tests/settings_lifecycle.rs`
- `src-tauri/crates/application/src/requirements_tests/unified_rules.rs`
- `src/features/rules/rule-single-pair-editor.tsx`
- `src/features/rules/rule-definition-model.ts`
- `src/features/rules/rule-definition-editor.test.tsx`
- `src/features/rules/rule-definition-model.test.ts`
- `src/features/capture/exchange-observation-detail.tsx`
- `src/features/capture/exchange-observation-detail.test.tsx`
- `src/features/help/page-help-content.ts`
- `docs/user-operation-guide.md`
- `docs/architecture/rules-and-protocol-packages.md`
- `docs/architecture/data-flow.md`
- `docs/architecture/runtime-observability.md`
- `docs/mcp/validation-playbook.md`
- 任务、完成索引与测试证据文件。

附加文件：[HTTP-RULE-EDITOR-SIMPLIFICATION-001](../../../testing/evidence/2026-09-04/TASK-20260904-002/HTTP-RULE-EDITOR-SIMPLIFICATION-001/README.md)。

- 验收结果：`PASS_WITH_INSTALLED_UI_AUTOMATION_NOT_RUN_AND_KNOWN_UNRELATED_SOURCE_SIZE_FAILURE`。
- Application 全量：417/417 PASS；新增抓包草稿 1/1 PASS。
- 前端定向：抓包与规则编辑器 17/17 PASS；完整前端 64 文件 554/554 PASS。
- 静态与构建：typecheck、lint、Rust fmt、strict Clippy、架构扫描、绑定确定性、`git diff --check` 和 Tauri macOS build PASS；Next 生成 13 个静态页面。
- 源码尺寸：仍只被 7 个既有无关文件阻断，本任务修改文件均低于 500 行。
- 本机安装：`/Applications/Intercept Proxy.app`，bundle id `com.interceptproxy.desktop`，版本 `1.0.0`，严格签名 PASS，构建/安装 SHA-256 均为 `ff21f80b323c901241db65459d705894b13b825d9b2e129e1b2c2d04dd56d824`，PID `87690`；上一版可从 `/Users/codin/.Trash/Intercept Proxy.app.codex-old-20260904-124937` 恢复。
- 安装版 UI：Computer Use 确认 App 正在运行，但连接其可访问性界面超时，因此点击式下拉复验 `NOT_RUN`；最终 jsdom 交互回归已验证条件过滤与动作保留。
- CI、push、发布：`NOT_RUN`；不在当前授权范围。

完成总结：HTTP 规则编辑器现在只提供 Method/Path 条件、有限 Method 候选和中文精简动作；不可匹配的 Document 容器不进入条件候选但仍可作为动作目标。抓包响应规则只复制 Body，生成 Proxy → App ReplaceBodyText 并明确搭配 LocalHttpServer，不再创建新的 MockResponse。
