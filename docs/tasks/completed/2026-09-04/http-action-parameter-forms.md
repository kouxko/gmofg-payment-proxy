# TASK-20260904-003：将 HTTP 动作参数改为明确表单

- 任务 ID：`TASK-20260904-003`
- 状态：`已完成`
- 任务日期：`2026-09-04`
- 创建时间：`2026-09-04 14:02:28 +08:00`
- 开始时间：`2026-09-04 14:02:28 +08:00`
- 最后更新时间：`2026-09-04 14:26:46 +08:00`
- 完成时间：`2026-09-04 14:26:46 +08:00`
- 创建路径：`docs/tasks/pending/2026-09-04/http-action-parameter-forms.md`
- 归档路径：`docs/tasks/completed/2026-09-04/http-action-parameter-forms.md`
- 关键词：`规则页面`、`HTTP 动作`、`动作参数`、`固定延迟`、`限速`、`弱网`、`表单`、`JSON`
- 任务优先级：`高`
- 优先级理由：动作参数决定弱网与故障注入的实际行为，前端需要准确映射 Rust 跨层合同；错误序列化会导致错误业务结果。

## 背景、目标与需求确认

当前规则编辑器只根据 Rust `parameters_required` 决定是否显示参数区，但所有有参 HTTP 动作统一要求用户手写 `动作参数 JSON`，没有字段解释和单位。

- 目标：每个仍可选择的 HTTP 动作使用中文标签、明确单位和适合该参数的输入控件；无参动作完全不显示参数区域；保存时继续生成 Rust 现有 `parameters_json` 合同。
- 范围：规则 HTTP 动作草稿状态、参数表单、反填与序列化、对应 UI 回归、操作说明、本地 App 构建安装与界面验证。
- 不在范围：修改 Rust 动作类型或运行语义、增加动作默认值、扩大动作集合、迁移已保存规则、调整弱网运行算法。
- 需求确认记录：`2026-09-04 14:02:28 +08:00` 用户明确指出限速、延迟等动作不知道如何填写，并要求无参数动作不显示动作参数。
- 未确认事项：零；字段、枚举和范围以当前 Rust 合同为准，不自行增加默认值。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`
- 范围与不在范围：`PASS`
- 输入、输出和状态变化：`PASS`；选择动作后按动作结构显示字段，切换动作清空旧参数；无参动作不渲染参数区。
- 错误行为：`PASS`；缺少必填字段时不可保存，Rust 继续作为最终范围与兼容性校验边界。
- 具体示例：`PASS`；固定延迟显示“延迟时间（毫秒）”；限速显示“速率（B/s）”和“分块大小（字节）”；连接上游前断开不显示参数区。
- 可重复 PASS/FAIL 验收：`PASS`
- 改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-09-04 14:02:28 +08:00`

## 问题与根因分析

- 实际现象：选择固定延迟、限速等动作后只显示一个原始 JSON 文本框。
- 预期依据：用户本次明确要求可理解的动作参数输入，并要求无参动作隐藏参数。
- 最小复现：规则页新建 HTTP 规则，进入“执行动作”，选择“限速”或“固定延迟”。
- 当前已验证：前端 `HttpActionDraft` 仅保存 `parameters: string`，`ActionForm` 对所有 `parameters_required` 动作统一渲染 `动作参数 JSON`。
- 当前已验证：Rust 已按动作声明 `milliseconds`、`bytes_per_second`、`chunk_bytes`、枚举模式等强类型参数，并在 Domain 校验范围。
- 已确认根因：前端丢失了 Rust 动作参数结构，只保留“是否需要参数”布尔值，导致 UI 无法按动作解释字段。
- 推断：无。
- 未知：无影响实现方向的未知。
- 影响范围：规则编辑器的 HTTP 动作草稿、反填、保存序列化和说明；Rust 运行合同保持不变。

## 最小改动与最优设计

| 方案 | 分析 |
| --- | --- |
| 继续使用 JSON 文本框，仅增加提示示例 | 用户仍需理解内部字段名，无法避免字段遗漏和动作切换残留，拒绝。 |
| 前端按现有 `RuleActionKind` 提供强类型表单并在保存边界序列化 | 不改 Rust 公共合同，字段含义清晰且可回填，采用。 |
| 扩展 Rust capability 返回完整动态 Schema | 长期可统一元数据，但本次会扩大公共接口和生成绑定范围；当前动作集合固定，暂不采用。 |

## 小任务、测试与审查

| ID | 内容 | 状态 | 验收 |
| --- | --- | --- | --- |
| T01 | 为现有 HTTP 动作建立参数草稿、就绪与序列化映射 | 已完成 | 每个可选有参动作映射现有 Rust 字段；无参动作生成 `null` |
| T02 | 将原始 JSON 替换为中文字段控件 | 已完成 | 不再出现“动作参数 JSON”，标签和单位明确 |
| T03 | 覆盖新建、切换、无参隐藏和既有动作反填 | 已完成 | 定向 UI 回归 PASS |
| T04 | 更新说明并执行全量验证、构建安装和界面复验 | 已完成 | 自动化、静态门禁与正式 App 安装证据完整；WebView 内容自动化 NOT_RUN 已记录 |

测试计划：先修改回归测试形成失败结果，再实现动作参数表单；运行规则编辑器定向测试、完整前端测试、typecheck、lint、构建和 `git diff --check`，随后构建并安装 macOS App。高优先级任务执行跨层差异审查。

## 实施记录、修改文件与验收结果

- `2026-09-04 14:02:28 +08:00`：完成历史、当前前端和 Rust 动作合同核对；确认旧实现同样使用原始 JSON 文本框，没有可复用的规则动作参数表单。
- `2026-09-04 14:06:27 +08:00`：先修改回归测试，确认旧实现仍显示原始 JSON 且缺少延迟字段，得到 2 项预期失败。
- `2026-09-04 14:10:32 +08:00`：完成强类型动作参数草稿、中文参数表单、切换清理、反填和保存序列化；定向回归 22/22 PASS。
- `2026-09-04 14:18:23 +08:00`：架构门禁发现前端编码转换；将非法 JSON 改为明确的 0–255 字节列表输入，移除 WebView 编解码并通过 Rust-only 边界。
- `2026-09-04 14:26:46 +08:00`：完成全量前端、Application factory、静态门禁、正式 Tauri 构建、签名、覆盖安装和进程回读。

修改文件：

- `src/features/rules/rule-http-action-parameters.ts`
- `src/features/rules/rule-http-action-parameters-form.tsx`
- `src/features/rules/rule-single-pair-editor.tsx`
- `src/features/rules/rule-http-action-parameters.test.ts`
- `src/features/rules/rule-definition-editor.test.tsx`
- `src/features/help/page-help-content.ts`
- `docs/user-operation-guide.md`
- 任务、完成索引与测试证据文件。

附加文件：[HTTP-ACTION-PARAMETER-FORMS-001](../../../testing/evidence/2026-09-04/TASK-20260904-003/HTTP-ACTION-PARAMETER-FORMS-001/README.md)。

- 验收结果：`PASS_WITH_INSTALLED_UI_CONTENT_AUTOMATION_NOT_RUN_AND_KNOWN_UNRELATED_SOURCE_SIZE_FAILURE`。
- 定向前端：22/22 PASS；完整前端：65 文件、566/566 PASS。
- 静态与构建：typecheck、lint、Next production build、生成绑定、完整架构扫描、Frontend Rust-only 边界、`git diff --check` 和 Tauri macOS build PASS。
- Application 动作草稿 factory：1/1 PASS；Rust 运行语义未修改。
- 源码尺寸：仍只被 7 个既有无关文件阻断，本任务生产文件均低于 500 行。
- 本机安装：`/Applications/Intercept Proxy.app`，bundle id `com.interceptproxy.desktop`，版本 `1.0.0`，deep/strict 签名 PASS，构建/安装 SHA-256 均为 `9c3af992e43472c095bff14e87a747ca0310483bb6618376d8f54c8341967d23`，PID `62783`；上一版可从 `/Users/codin/.Trash/Intercept Proxy.app.codex-old-20260904-142245` 恢复。
- 安装版 UI：Computer Use 能识别原生 App 窗口，但当前 macOS Space 未暴露 WebView 内容，因此点击式内容复验 `NOT_RUN`；最终 jsdom 交互已覆盖参数字段和无参隐藏。
- CI、push、发布：`NOT_RUN`；不在当前授权范围。

完成总结：HTTP 动作不再暴露原始参数 JSON。有参动作按当前 Rust 合同显示中文字段、单位和枚举下拉；切换动作清空旧参数，无参动作完全隐藏参数区域，并继续通过原有 Rust factory 完成最终校验。
