# TASK-20260904-001：恢复新建规则三段式 Tab

- 任务 ID：`TASK-20260904-001`
- 状态：`已完成`
- 任务日期：`2026-09-04`
- 创建时间：`2026-09-04 10:14:02 +08:00`
- 开始时间：`2026-09-04 10:14:02 +08:00`
- 最后更新时间：`2026-09-04 11:44:12 +08:00`
- 完成时间：`2026-09-04 11:44:12 +08:00`
- 创建路径：`docs/tasks/pending/2026-09-04/restore-rule-creation-tabs.md`
- 归档路径：`docs/tasks/completed/2026-09-04/restore-rule-creation-tabs.md`
- 关键词：`规则页面`、`新建规则`、`基本信息`、`匹配条件`、`执行动作`、`Tabs`、`DocumentMatchPath`、`通配符动作`
- 任务优先级：`高`
- 优先级理由：用户将范围扩大为 Document 动作路径支持 `*`，会改变统一规则公共合同、Schema 能力、Rust Domain 执行与持久化反序列化边界，需要完整回归和安装验收。

## 背景、目标与需求确认

用户要求规则页面的新建规则继续使用此前的三段式 Tab，并要求参考之前实现。

当前 Git 历史表明，`60dfaae` 之前的正式 `rule-editor-panel.tsx` 使用“基本信息 / 匹配条件 / 执行动作”
三个 Tab；`60dfaae` 统一规则重构以新的 `rule-definition-editor.tsx` 替换旧编辑器时没有迁移该布局。
当前创建态由 `RuleCreationEditor` 连续渲染元数据、单条件和单动作表单，但帮助内容仍描述三段式 Tab。

目标：仅在“新建规则”状态恢复“基本信息 / 匹配条件 / 执行动作”三段式 Tab，同时保留当前
“一条规则一个条件、一个动作”的模型、Listener/阶段能力读取和保存校验。

- 范围：新建规则固定编辑区、三个 Tab、未选择 Listener/阶段时的明确提示、Tab 切换状态保持；Document 动作路径单层 `*` 的 Schema 能力、保存、持久化、Domain 执行和回归测试。
- 不在范围：已有规则编辑布局、旧多条件/多动作模型、默认 Listener/阶段、故障预设、`**` 递归通配符、自动创建缺失父节点。
- `2026-09-04`：用户明确“规则页面新建规则仍然想要三段式 tab”，并要求参考之前实现。
- `2026-09-04 11:07:18 +08:00`：用户先要求动作路径包含 `*` 时提示；该警告方案尚未实现。
- `2026-09-04 11:11:16 +08:00`：用户明确覆盖上一要求：“匹配条件允许为 `*`，动作也要允许”。动作通配符按匹配条件相同的“一层一个 `*`”展开，对动作执行前快照中的全部具体命中节点执行；不自动创建缺失父节点。上一条“不支持通配符动作”的验收失效。
- 未确认事项：零；动作通配符复用现有 `DocumentMatchPath` 语义，不引入第二种通配符语法。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`
- 范围与不在范围：`PASS`
- 输入、输出和状态变化：`PASS`，精确路径保持原行为；动作路径中的每个 `*` 展开恰好一层，并对动作前快照中的全部具体路径执行同一 Set/Clear/Insert/Append。
- 错误行为：`PASS`，能力未就绪时显示提示，不伪造默认能力；通配符没有命中时不执行动作；已命中的任何一次具体动作失败则整体执行返回错误，不报告部分成功；不自动创建缺失父节点。
- 具体示例：`PASS`，`/GICC_01/*/tables/*/merchant_type_code` 的 Set 会修改动作开始时存在的所有对应字段；`/items/*` 的 Clear 会删除动作开始时存在的全部数组元素且不受下标移动影响。
- 可重复 PASS/FAIL 验收：`PASS`
- 改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-09-04 10:14:02 +08:00`；通配符动作变更重新通过门禁时间：`2026-09-04 11:11:16 +08:00`

## 问题与根因分析

- 实际现象：新建规则把基本信息、匹配条件、执行动作纵向连续显示，没有三段式 Tab。
- 预期依据：用户本次明确要求；旧正式实现与当前帮助内容均使用三段式 Tab。
- 最小复现：进入规则页面，点击“新建规则”，观察右侧固定编辑区。
- 当前已验证：当前 `RuleCreationEditor` 直接组合 `RuleMetadataFields` 与 `RuleSinglePairEditor`；后者连续输出条件表单、动作表单和操作按钮。
- 当前已验证：`DocumentMutation` 的 Set/Clear/Insert/Append 路径均为精确 `JsonPointer`，动作草稿也只解析为 `JsonPointer`；Schema 遍历对 `item_template` 明确返回空动作能力，因此带 `*` 的动作下拉为空。
- 已确认根因：统一规则和单条件/单动作重构替换了旧 `RuleEditorPanel`，新组件只迁移了字段和保存能力，没有迁移 Tab 展示结构；同时通配符只被建模为条件路径，动作类型、Schema 校验和运行时均没有展开具体路径的能力。
- 推断：无。
- 未知：无影响实现方向的未知。
- 影响范围：前端规则创建态、Rust Domain 文档动作与 Schema 校验、Application 草稿工厂/能力模型、统一规则序列化兼容、相关基础设施调用方和帮助/架构文档。

## 最小改动与最优设计

| 方案 | 分析 |
| --- | --- |
| 在创建组件外复制条件/动作状态 | 会拆散 `RuleSinglePairEditor` 的物化与校验状态，产生重复状态，拒绝。 |
| 让 `RuleSinglePairEditor` 支持可选创建态 Tab 容器 | 复用同一份条件/动作状态和保存逻辑；已有编辑态继续连续布局，采用。 |
| 恢复整个旧编辑器 | 会带回已删除的多条件、多动作和旧能力模型，违反当前合同，拒绝。 |
| 为每种动作增加独立 Pattern 枚举变体 | 会改变已持久化规则的 tagged JSON 形状并扩大分支数，拒绝。 |
| 将 DocumentMutation 路径统一为 DocumentMatchPath | 精确路径序列化仍是原字符串；通配符复用条件既有解析/Schema 语义，运行时先解析具体路径快照再执行，采用。 |

## 小任务、测试与审查

| ID | 内容 | 状态 | 验收 |
| --- | --- | --- | --- |
| T01 | 增加创建态三段式 Tab，保持现有状态与保存逻辑 | 已完成 | 三个 Tab 分区正确，已有编辑态不变 |
| T02 | 更新/补充规则 UI 回归 | 已完成 | 默认 Tab、切换、未就绪提示、表单状态均通过 |
| T03 | 执行定向测试、类型检查、lint、证据和归档 | 已完成 | 适用门禁通过；既有源码尺寸失败明确归档 |
| T04 | 本地构建安装并复核用户追加布局要求 | 已完成 | 顶部操作、说明首显、控件与双栏顶栏对齐均在安装 App 中通过 |
| T05 | 统一规则页全部 Select 的长文本裁切合同 | 已完成 | 条件/动作长 Schema 路径与其它 Select 均不换行溢出 |
| T06 | 增加 Document 通配符动作 Domain 执行与 Schema 校验 | 已完成 | Set/Clear/Insert/Append 对全部快照命中生效，数组 Clear 顺序正确，失败不伪装成功 |
| T07 | 开放 item-template 动作能力和草稿保存 | 已完成 | Schema/手动 `*` 路径可选择动作并保存，精确规则 wire JSON 兼容 |
| T08 | 完整 Rust/前端回归、文档、证据和本地安装复验 | 已完成 | 受影响层级测试通过，安装 App 可下拉选择并保存通配符动作草稿 |

测试计划：先增加失败的 Domain 单元/合同测试，覆盖通配符 Set、多数组目标 Insert/Append、数组元素 Clear
倒序执行、零命中和错误回滚；补充 Application 能力/草稿测试及精确路径序列化兼容测试。随后运行受影响
Rust crate 测试、规则编辑器 Vitest、TypeScript typecheck、lint、Tauri 构建与 `git diff --check`，保存实际
输入输出并重新安装桌面 App 验证动作下拉。高优先级任务执行完整差异审查和受影响层级验证。

## 实施记录、修改文件与验收结果

- `2026-09-04 10:14:02 +08:00`：定位旧正式实现与删除点，登记任务并锁定只修改新建态。
- `2026-09-04 10:18:13 +08:00`：为 `RuleSinglePairEditor` 增加可选创建态 Tab 容器；元数据、条件和动作仍共享同一组件状态及保存路径。
- `2026-09-04 10:21:14 +08:00`：定向测试、完整 UI 合同、typecheck、lint、Next build 和 diff 检查通过；保存已知无关源码尺寸失败。
- `2026-09-04 10:24:49 +08:00`：用户追加要求安装到本地；原“运行中桌面 App 视觉验收 NOT_RUN”结论失效，任务重新进入进行中。新增验收为构建当前源码 Tauri macOS `.app`、替换 `/Applications/Intercept Proxy.app`、校验 bundle identity/签名/可执行文件并确认启动后稳定存活；源码和三段式 Tab 合同不变。
- `2026-09-04 10:30:07 +08:00`：用户基于已安装 App 的实际界面要求“保存规则”不得显示在 Tab 内容底部，改为位于顶部“取消”旁边。原底部按钮位置验收失效；新增验收为创建态顶部同时显示“保存规则”和“取消”，底部不再重复显示保存按钮，原禁用条件与提交语义不变。
- `2026-09-04 10:38:44 +08:00`：用户补充要求“启用规则”与右侧优先级控件视觉对齐，并要求“说明”在打开新建规则时立即显示。调查确认错位来自短高度 Switch 与 NumberField 控件仅按底边对齐；说明此前被 HTTP 阶段结构条件化。新增验收为两个控件中心线对齐、说明在 Listener/阶段未选择时可见且输入在选择 HTTP 能力后保留；Socket 合同没有说明字段，选定 Socket 阶段后不显示也不写入。
- `2026-09-04 10:46:53 +08:00`：用户进一步指出左右区域顶栏未对齐。调查确认左侧规则列表使用 `p-4`，右侧创建/编辑区域使用 `p-1`，造成右侧标题与按钮整体高约 12px。新增验收为左右工作区使用相同 `p-4` 内边距，标题和操作按钮起始基线一致；不改变两列宽度或内容结构。
- `2026-09-04 10:52:02 +08:00`：最终定向与完整 UI 合同通过；重新构建、签名、安装 macOS App，并通过实际可访问性树和实时截图确认三段式 Tab、顶部保存/取消、说明首显、表单控件中心线及左右顶栏对齐。
- `2026-09-04 10:55:04 +08:00`：用户在安装 App 的匹配条件和执行动作 Tab 发现 Document Schema 长路径换行后越出固定高度 Select。任务重新进入进行中；新增验收为规则创建/编辑器内所有 Select 的触发器统一裁切、已选文本单行省略、指示器不收缩，并逐项覆盖基本信息、匹配条件与执行动作中的 Select，不改变实际选中值和保存路径。
- `2026-09-04 11:04:55 +08:00`：规则目录全部 Select 已统一固定高度、单行省略、触发器裁切和固定指示器；自动化使用 `/KCCI_01/*/kid` 覆盖条件/动作，安装 App 使用两条更长 `/GICC_01/*/tables/*/...` 路径完成实时截图复验，手动路径保持完整值。
- `2026-09-04 11:07:18 +08:00`：用户指出动作路径含 `*` 时应明确提示。任务继续进行；新增验收为 Document 动作的 Schema 或手动路径包含 `*` 时显示“不支持通配符、请使用具体数组索引”的提示，不改变匹配条件允许单层 `*`/多节点 ANY 的现有合同，也不增加通配符动作实现。
- `2026-09-04 11:11:16 +08:00`：用户明确动作也必须允许 `*`，覆盖前述警告方案。源码确认阻断点为 `DocumentMutation` 精确路径类型、动作工厂精确解析及 item-template 空动作能力；任务升为高优先级并扩大到 Domain/Application/持久化合同与运行时验证。
- `2026-09-04 11:44:12 +08:00`：`DocumentMutation` 统一使用 `DocumentMatchPath`；通配符动作基于动作前快照展开具体指针，Clear 反向执行以避免数组下标漂移，多目标失败通过 working clone 原子回滚。Schema item-template 开放对应动作，factory、generated bindings、UI 提示及架构/用户文档同步完成。自动化、正式构建、安装与实际 App 下拉验证通过。

修改文件：

- `src/features/rules/rule-creation-editor.tsx`
- `src/features/rules/rule-definition-editor.tsx`
- `src/features/rules/rule-definition-list.tsx`
- `src/features/rules/rule-metadata-fields.tsx`
- `src/features/rules/rule-single-pair-editor.tsx`
- `src/features/rules/rule-definition-editor.test.tsx`
- `src/features/rules/rules-view.test.tsx`
- `src/features/help/page-help-content.ts`
- `src/generated/rust-types.ts`
- `src-tauri/crates/domain/src/document/model.rs`
- `src-tauri/crates/domain/src/document/pointer.rs`
- `src-tauri/crates/domain/src/document/schema.rs`
- `src-tauri/crates/domain/src/unified_rule_execution.rs`
- `src-tauri/crates/domain/src/unified_rule_execution/mutation.rs`
- `src-tauri/crates/domain/tests/phase5_unified_rule_domain.rs`
- `src-tauri/crates/domain/tests/unified_rule_contract.rs`
- `src-tauri/crates/application/src/facade/unified_rule_editor/document_factory.rs`
- `src-tauri/crates/application/src/models/unified_rule.rs`
- 受 `DocumentMutation` 路径类型影响的 Application/Infrastructure 测试构造器
- `docs/architecture/rules-and-protocol-packages.md`
- `docs/user-operation-guide.md`
- `docs/README.md`
- `docs/tasks/README.md`
- `docs/testing/evidence/README.md`
- 本任务文档、`RULE-CREATION-TABS-001`、`RULE-CREATION-TABS-LOCAL-INSTALL-001`、`RULE-SELECT-OVERFLOW-001` 与 `RULE-WILDCARD-ACTIONS-001` 证据目录。

附加文件：[RULE-CREATION-TABS-001](../../../testing/evidence/2026-09-04/TASK-20260904-001/RULE-CREATION-TABS-001/README.md)、[RULE-CREATION-TABS-LOCAL-INSTALL-001](../../../testing/evidence/2026-09-04/TASK-20260904-001/RULE-CREATION-TABS-LOCAL-INSTALL-001/README.md)、[RULE-SELECT-OVERFLOW-001](../../../testing/evidence/2026-09-04/TASK-20260904-001/RULE-SELECT-OVERFLOW-001/README.md)、[RULE-WILDCARD-ACTIONS-001](../../../testing/evidence/2026-09-04/TASK-20260904-001/RULE-WILDCARD-ACTIONS-001/README.md)。

- 验收结果：`PASS_WITH_KNOWN_UNRELATED_BASELINE_FAILURES`；UI 布局、Select 溢出和通配符动作全部通过。
- Domain：100/100 PASS，其中新增通配符 Set/Clear/Insert/Append、零命中、Schema 与原子失败 5/5 PASS。
- Application 定向：10/10 PASS；全量 414/416，2 项失败在当前 HEAD 可独立复现且分别对应既有 response action 数量断言和 ProxyToApp MockResponse 合同不一致，不由本次 Document 变更引起。
- Infrastructure HTTP Pipeline 15/15、External Relay 4/4 PASS；workspace all-targets check 和 strict Clippy PASS。
- 定向规则前端测试：2 文件、15/15 PASS。
- 完整前端 UI 合同：30 文件、303/303 PASS。
- 完整前端：64 文件、551/551 PASS；`deno task typecheck`、lint、架构扫描、绑定确定性、Rust fmt、strict Clippy、Tauri build、严格签名和 `git diff --check` PASS；Next 生成 13 个静态路由。
- 源码尺寸：本任务修改文件均小于 500 行；全仓门禁仍仅被当前 HEAD 的 7 个无关既有超限文件阻断。
- 本机安装：`/Applications/Intercept Proxy.app`，bundle id `com.interceptproxy.desktop`，版本 `1.0.0`，严格签名校验 PASS，可执行文件 SHA-256 `482084cc5c0515c213e17fe65d344e625722cf8af15baafde72922ab3e09a483`，最终 PID `23649`；旧 App 可从 `/Users/codin/.Trash/Intercept Proxy.app.codex-old-20260904-114006` 恢复。
- 运行中桌面 App 验收：`PASS`；实际 `/GBRD_01/*/aid` Schema 动作路径下拉提供 `set`、`clear`，可选中 `set` 并显示全部命中提示；未保存测试规则。
- CI、push、发布：`NOT_RUN`；不在用户授权范围。
- 对抗审查：完成 Domain/Application/UI/persistence wire/失败原子性及精确路径兼容复核；未引入第二套通配符语法、默认路径、自动创建、静默部分成功或新依赖。

完成总结：新建规则保持三段式 Tab、顶部保存/取消、说明首显和完整布局修复；Document 动作现在与条件一致支持完整路径段 `*`，对动作前快照的全部命中节点执行 Set/Clear/Insert/Append，零命中不修改、失败不保留部分修改、精确路径 wire 兼容。正式 macOS App 已重新构建、签名、安装并通过实际下拉验证。
