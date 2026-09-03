# TASK-20260903-009：协议 Display 宽表横向滚动

- 任务 ID：TASK-20260903-009
- 状态：已完成
- 任务日期：2026-09-03
- 创建时间：2026-09-03 16:33:01 +08:00
- 开始时间：2026-09-03 16:33:01 +08:00
- 最后更新时间：2026-09-03 16:48:57 +08:00
- 完成时间：2026-09-03 16:48:57 +08:00
- 创建路径：`docs/tasks/pending/2026-09-03/make-protocol-display-tables-horizontally-scrollable.md`
- 归档路径：`docs/tasks/completed/2026-09-03/make-protocol-display-tables-horizontally-scrollable.md`
- 关键词：`抓包`、`Exchange`、`Display`、`table`、`横向滚动`、`iframe`
- 任务优先级：高（修改所有协议包共用的不可信 HTML 安全渲染边界与密集表格布局）

## 背景与目标

用户在抓包详情的 Payment DLL Display 中观察到宽表无法横向滚动，几十列表头逐字符换行并挤在一起。

目标：每张协议 Display 表格保留可读的字段列宽，宽度超过 iframe 视口时在表格自身出现横向滚动；
不改变协议包数据、HTML 安全清洗、CSP、节点上限或非表格内容行为。

## 范围、不在范围与确认记录

- 在 `ProtocolSafeDisplay` 生成的隔离文档内，为每张安全 table 增加独立横向滚动容器。
- 表头和单元格覆盖协议包祖先节点继承的任意断行规则，保留原始换行但不按字符挤压列宽。
- 增加安全清洗输出合同测试，证明 wrapper、滚动样式和 table 语义同时保留。
- 更新抓包操作说明并保存测试证据。
- 不修改 Payment DLL Decode/Display 字段、业务数据、iframe sandbox/CSP 或 Modal 尺寸。
- 2026-09-03：用户提供实际抓包详情截图并要求解决 Display 宽表无法横向滚动、文本挤压问题。
- 2026-09-03：用户明确自行完成 UI 验证，自动化验收不等待浏览器截图。
- 未确认事项：零；期望行为由截图和用户描述明确。

## 需求就绪与问题分析

- 实际现象：宽表被压缩到 iframe 可视宽度，字段名逐字符换行，底部没有横向滚动条。
- 预期行为：字段和数据保持可读列宽，超出可视宽度后通过表格内横向滚动查看。
- 最小复现：打开包含 Payment DLL 多列表格 Display 的 Exchange 详情。
- 当前已验证：Host CSS 将 table 固定为 `width:100%` 且无滚动 wrapper；Payment DLL 顶层样式为 `white-space:pre-wrap;overflow-wrap:anywhere`，安全清洗器允许并保留这两个属性。
- 已确认根因：可继承的任意断行策略降低了 table 的最小内容宽度，浏览器把列压缩后逐字符换行，因此没有产生可供横向滚动的真实溢出；iframe 内也没有逐表滚动容器。
- 影响范围：所有通过 `ProtocolSafeDisplay` 输出宽 table 的协议包；普通短表和非表格 Display 保持现状。
- 需求就绪：目标、范围、输入输出、复现和 PASS/FAIL 标准明确；进入实现时间 2026-09-03 16:33:01 +08:00。

## 方案、任务与验收

- 最小改动：直接把 table 改为 block scroll box。风险是改变原生 table 外层布局和 caption/border 表现。
- 最优设计：清洗时为安全 table 插入独立 scroll wrapper，table 保持原生语义；Host CSS 只在 wrapper 上处理 overflow，并在单元格覆盖继承断行策略。
- 采用最优设计；不增加依赖，不扩大 iframe 权限。

验收标准：

1. 每张清洗后的 table 外层存在独立的横向滚动容器：PASS。
2. table 使用内容宽度且至少占满容器；表头和单元格不再按字符强制换行：PASS（DOM/CSS 合同）；实际 UI 由用户验证。
3. table、caption、thead、tbody、tr、th、td 语义和安全样式仍保留：PASS。
4. ProtocolSafeDisplay 定向测试、typecheck、lint 和正式前端构建通过：PASS。

| ID | 内容 | 状态 | 验收 |
| --- | --- | --- | --- |
| T01 | 增加 table 滚动 wrapper 和防挤压样式 | 已完成 | 宽表产生独立横向滚动 |
| T02 | 增加安全清洗与布局合同回归 | 已完成 | wrapper 与 table 语义均保留 |
| T03 | 文档、构建、证据与归档 | 已完成 | 验收材料完整 |

- 对抗审查：按用户在当前连续交付中明确要求跳过；以定向回归、静态门禁和构建验证替代。

## 实施、测试与完成总结

- `src/features/shared/protocol-safe-display.tsx`：为安全 table 生成 `.protocol-display-scroll` 外层；table 使用 `width:max-content` 与 `min-width:100%`；表头/单元格使用 `white-space:pre`、`overflow-wrap:normal`、`word-break:normal`。
- `src/features/shared/protocol-safe-display.test.tsx`：验证逐表 wrapper、横向 overflow、内容宽度与原生 table 结构。
- `docs/user-operation-guide.md`：补充宽表在表格内部横向滚动的操作说明。
- 定向前端测试：4 文件、35 项全部通过。
- `deno task typecheck`、`deno task lint`、`cargo fmt --check`、`git diff --check`：全部通过。
- `deno task build`：Next.js 16.2.12 正式构建成功，13 个静态页面生成。
- UI 人工验收：由用户自行执行，本任务不等待截图。
- CI：NOT_RUN，用户未要求触发外部 CI。
- 测试证据：[PROTOCOL-DISPLAY-TABLE-SCROLL-001](../../../testing/evidence/2026-09-03/TASK-20260903-009/PROTOCOL-DISPLAY-TABLE-SCROLL-001/README.md)。

完成结果：宽表不再被宿主的任意断行规则逐字符压缩，每张表通过自身滚动容器查看溢出列；iframe sandbox、CSP 和允许元素边界保持不变。
