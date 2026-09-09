# 抓包列表显示 HTTP 完整 URL 路径

- 任务 ID：TASK-20260909-001
- 状态：已完成
- 任务日期：2026-09-09
- 创建时间：2026-09-09 11:43:49 +08:00
- 开始时间：2026-09-09 11:43:49 +08:00
- 最后更新时间：2026-09-09 11:57:43 +08:00
- 完成时间：2026-09-09 11:57:43 +08:00
- 创建路径：`docs/tasks/pending/2026-09-09/show-http-url-in-capture-list.md`
- 归档路径：`docs/tasks/completed/2026-09-09/show-http-url-in-capture-list.md`
- 优先级：低；局部前端展示，不修改公共合同与传输行为。
- 关键词：HTTP、抓包、URL、单行、省略号、develop

## 背景与目标

用户要求分析抓包页显示完整 URL 路径的可行性，单行展示，超出区域省略；可实现时从 develop 创建 feature 分支修改。
当前已验证：列表按连接展示，只有时间、协议、对端、计数、状态、Exchange ID；HTTP received/sent 事件包含请求 Header。
历史已验证：早期抓包从 ExchangeObservation 读取连接事件；本次源码已重新确认。历史 Mock 方案不构成本任务需求。
分支：`feature/capture-http-url`，基于 freshly fetched `gitlab/develop` 的 `1c88aeb8`；独立 worktree 保留原工作区未提交档案。

## 范围与非范围

范围：HTTP URL 列、单行溢出省略、对应组件验证与操作文档。
非范围：Socket 行语义、规则与代理转发、外部推送、CI、发布、原工作区无关修改。

## 需求确认记录与就绪检查

已确认：完整 URL 路径、单行、区域外省略、develop 新 feature 分支。
待确认：连接内多请求显示最新还是首次；URL 指路径与参数还是始终包含 scheme/host 的绝对地址。
示例：`/api/pay?id=1` 或 `https://example.com/api/pay?id=1`，等待用户选择。
就绪检查：目标与视觉验收明确；数据选择尚未确认，保持待确认，不实施依赖该结论的源码。
进入实现时间：2026-09-09 11:47:05 +08:00。

用户后续确认：显示 App 原始请求路径，取最新请求的完整路径和参数，单行展示，超宽省略。该确认替代上述待确认项；未确认事项清零，就绪检查 PASS。示例：`/api/pay?id=1&token=a%2Fb+c` 原样展示。
验收：最新 upstream received 优先；后续 sent/encoded/response 不覆盖；空连接与 Socket 无请求路径；不修改输入，长路径一行省略。

## 最小改动与最优设计

最小方案：在现有 observation model 提取最后一条 upstream received HTTP 请求行中的原始 request-target；列表增加有界宽度列，CSS truncate。
最优方案：展示投影留在前端模型，保留 Rust 观测合同，不新增公共字段或协议处理路径。与最小方案一致，采用。完整 Header 仍由详情提供。
这是新展示需求，根因分析与架构迁移 N/A。空连接、Socket 或缺失请求行显示无数据标记，不伪造 URL，不回退较旧请求。

## 小任务

| ID | 内容 | 依赖 | 可并行 | 负责人 | 状态 | 验收 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 数据与范围调查 | 无 | 是 | 主 Agent | 已完成 | 明确数据来源 |
| 2 | URL 展示 | 1、需求确认 | 否 | 主 Agent | 已完成 | 选择语义与省略展示正确 |
| 3 | 验证、文档、归档 | 2 | 否 | 主 Agent | 已完成 | UI/组件、静态验证与档案一致 |

## 文档影响与测试计划

操作指南抓包章节需要更新；根 README、架构、ADR、MCP、包、Android、示例及脚本说明若仅展示变化则无需更新。
测试：最新/首次选择、保留 query 与编码文本、HTTP/Socket、未收到请求、列表选择交互；实际浏览器单行、长 URL 省略与宽度检查；typecheck、targeted lint、frontend boundary。
网络、Frame、业务、真实设备与 CI：展示任务不适用，不作为验收结论。
审查：需求确认后按风险决定；纯展示低优先级可由主 Agent 审查，公共合同扩展执行独立整体审查。

## 实施、验收与完成记录

仅只读调查与创建隔离分支。未修改源码；测试未执行；CI 未触发。附加文件与证据待执行后记录。

小任务与整体独立对抗审查均不执行：低风险展示投影，主 Agent 审查数据选择、输入不变、布局及现有交互，配合针对性测试和浏览器验证。

## 最终实施与验收

实施：`latestAppRequestTarget` 从事件末尾查找 HTTP upstream received，取原始请求行 request-target；
不读 encoded/sent/response，不解码、不重建 URL。增加“App 原始请求路径（最新）”列，以 256px 宽、
单行省略和 title 完整文本显示。Socket 与无原始请求时显示 —。旧请求不会在最新请求缺失时冒充最新请求。

修改文件：
- `src/features/capture/exchange-observation-model.ts`
- `src/features/capture/exchange-observation-list.tsx`
- 上述两者的 `.test.ts` / `.test.tsx`
- `docs/user-operation-guide.md`：已同步连接列表真实列、最新原始请求与省略查看方式。
- 当前任务档案、任务完成索引和测试证据索引。

根 README、需求基线、架构、ADR、MCP、包、Android、示例和脚本文档：无需更新，公共合同无变化。
根因分析、架构重构和迁移：N/A，新展示需求，无此类变更。
小任务和整体独立审查：不执行，低风险局部展示；主 Agent 对最终变更审查 APPROVE。

验收：抓包 6 文件 30/30；typecheck、目标 ESLint、前端边界、diff check PASS；Webpack 生产构建 PASS。
实际 Edge 中 1280/960 视口路径宽256px、高16px、单行省略 PASS；完整路径 title/DOM 保留，点击选中 PASS。
默认 Turbopack 因 worktree node_modules 指向外部目录失败，改用 Webpack 完成构建验证。
未修改构建配置；真实 Tauri WebView/设备/业务 NOT_RUN，不作为本任务验收声明。

全部正式证据：[capture-url](../../../testing/evidence/2026-09-09/TASK-20260909-001/capture-url/README.md)。
资源、输入、输出、命令、复测入口、截图与环境摘要均已归档。临时 UI harness、layout 变更、浏览器和端口均清理。
使用技能：本机 Playwright SKILL.md（CLI 工作流），用于实际浏览器视觉与交互验证，保留项目任务与证据归档规则。
CI、推送、部署和发布均未执行。完成本地实现、验证、文档同步与日期归档。
