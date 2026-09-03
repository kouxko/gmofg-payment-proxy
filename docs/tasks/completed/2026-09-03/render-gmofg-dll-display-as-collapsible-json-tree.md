# TASK-20260903-010：GMO-FG DLL Display 按 JSON 树折叠

- 任务 ID：TASK-20260903-010
- 状态：已完成
- 任务日期：2026-09-03
- 创建时间：2026-09-03 16:35:01 +08:00
- 开始时间：2026-09-03 16:35:01 +08:00
- 最后更新时间：2026-09-03 16:44:34 +08:00
- 完成时间：2026-09-03 16:44:34 +08:00
- 创建路径：`docs/tasks/pending/2026-09-03/render-gmofg-dll-display-as-collapsible-json-tree.md`
- 归档路径：`docs/tasks/completed/2026-09-03/render-gmofg-dll-display-as-collapsible-json-tree.md`
- 关键词：`GMO-FG`、`Payment DLL`、`Display`、`JSON tree`、`details`、`summary`、`table`
- 任务优先级：高
- 优先级理由：虽然 DLL Decode、Encode、Document、网络结果、持久化和 Host ABI 不变，但任务调整所有协议包共享的不可信 HTML 安全 allowlist；必须证明新增折叠元素不引入事件或主动内容能力，并执行完整 Host、前端安全与 production build 验证。

## 背景、目标与历史连续性

`TASK-20260903-005` 已交付完整 downstream Decode、Encode 和分色 table Display。实际完整 Credit
Document 包含大量嵌套表，全部平铺后不便定位；根 caption 使用 JSONPath `$`，用户确认该技术标记
不适合作为界面标题，并要求按照 JSON 树形结构展示，每个可展开的节点都能展开或收起。

目标：按 Document 的 Object、Array、字段和数组下标生成嵌套树；每个 Object/Array 使用原生
`details/summary` 控制展开收起，标量作为叶子 table 展示。根节点使用“基本信息”，不显示 `$`；节点
标题保留真实字段名、数组下标、值类型和数量，使折叠状态下仍能看出完整 JSON 结构。

历史连续性：

- 原任务：`TASK-20260903-005`
- 原用例：`gmofg-dll-downstream-package`
- 原证据：`docs/testing/evidence/2026-09-03/TASK-20260903-005/gmofg-dll-downstream-package/`
- 保持行为：DLL 完整解析、自动 Length Encode、HTML 转义、每表稳定分色、1 MiB / 8192 节点和其他
  Display 安全边界继续有效。

## 范围、不在范围与需求确认

范围：

- GMO-FG DLL downstream Display 按 JSON 容器关系递归生成嵌套结构。
- 根 Object 默认展开；其余 Object/Array 节点默认收起，每个节点均可独立展开或收起。
- Object 标题显示字段名与 `Object`，Array 标题显示字段名、`Array` 和元素数量，数组元素显示下标。
- 根标题显示“基本信息”，不显示 `$`；不使用仅供实现定位的 `$.field` caption。
- Proxy 安全清洗明确允许 `details`、`summary` 及受控 `open` 属性，并提供清晰的树缩进和折叠样式。
- 补充协议包输出测试、安全清洗测试、Host 组件测试并重新构建最终 Wasm。

不在范围：

- 不改变 Decode、Encode、Document Schema、规则路径、HTTP Body、DLL 表结构或资源上限。
- 不引入 JavaScript 折叠逻辑、持久化折叠状态、搜索、虚拟列表或新的前端依赖。
- 不修改 `TASK-20260903-009` 的宽表横向滚动需求；共享安全 Display 文件按顺序集成，保留其改动。

需求确认：

- 用户确认去除 `$` 技术标题。
- 用户要求内容太多时支持展开收起，并进一步明确“按照 json 个树形结构 每个都能展开收起”。
- 解释为 Object/Array 容器节点均可折叠，标量是不可再展开的叶子；根节点默认展开以避免初次进入只见空壳，其余节点默认收起以解决内容过多。

## 需求就绪检查

- 目标和成功结果：PASS。
- 范围与不在范围：PASS。
- 输入、输出和状态变化：PASS；只改变 Display HTML，无业务状态变化。
- 具体示例：PASS；根 `基本信息` Object、`KCCI_01` Array、`[0]` Object 和 `card_ranges` Array 构成可重复断言。
- 可判断验收标准：PASS，见下节。
- 会改变实现方向的未确认事项：零。
- 进入实现时间：2026-09-03 16:35:01 +08:00。

## 最小改动与最优设计

| 方案 | 分析 |
| --- | --- |
| 最小改动 | 在现有每张 table 外简单包一层 `details`，虽能折叠，但仍是带完整 JSONPath 的平铺列表，不能表达父子层级，不满足 JSON 树要求。 |
| 最优设计 | Display renderer 以 Object/Array 为递归容器，每个容器输出嵌套 `details/summary`，标量 table 留在所属 Object 内；安全渲染器仅新增无脚本原生元素和 `open` 属性。 |

采用最优设计；复用浏览器原生 Disclosure，不增加依赖或运行脚本。

## 验收标准

1. 根标题为“基本信息”，HTML 中不出现 `>$</caption>` 或 `>$.` 路径标题。
2. 每个 Object/Array 容器均输出嵌套 `details/summary`；根默认展开，子容器默认收起。
3. `KCCI_01`、`card_ranges`、数组 `[0]` 等字段/下标和 Object/Array 类型、数组数量在折叠标题中可见。
4. 标量值仍以 table 展示，每张 table 的确定性分色和 HTML 转义保持不变。
5. Proxy 清洗保留 `details/summary/open`，仍删除脚本、事件属性和其他主动内容。
6. 完整 Credit Display 仍低于 1 MiB / 8192 节点并通过真实 Wasmtime Host 调用。
7. 包测试、Host 集成测试、前端定向测试、fmt、Clippy、ESLint、TypeScript 和构建校验通过。

## 小任务、测试、文档与审查

| ID | 内容 | 状态 | 验收 |
| --- | --- | --- | --- |
| TREE-01 | 增加 JSON 树与安全清洗 RED 测试 | 已完成 | 旧平铺输出与旧 allowlist 分别按预期失败 |
| TREE-02 | 重构包 Display 为递归 Disclosure 树 | 已完成 | Object/Array 节点均可折叠 |
| TREE-03 | 扩展安全 Display allowlist 与样式 | 已完成 | details/summary/open 被保留，主动内容仍删除 |
| TREE-04 | 构建、Host 回放、证据与归档 | 已完成 | 本地验收全部通过 |

测试计划：先固定根标题、嵌套结构、默认展开状态和字段/下标断言并验证 RED；实现后运行包单测、
Host Component 回放、安全渲染定向测试、格式、strict Clippy、ESLint、TypeScript、Wasm 构建和产物哈希。

文档影响：更新包 README 的 Display 说明；创建派生测试证据并引用 `TASK-20260903-005`。

对抗审查：按高优先级任务检查树结构真实性、默认状态、HTML 注入、安全 allowlist、资源门禁和
`TASK-20260903-009` 共享文件集成，无未解决 P0/P1/P2 后归档。

## 实施、测试与完成总结

- `display.rs` 不再把递归 Document 压平成带 JSONPath caption 的表集合；现在每个 Object/Array 输出
  一个嵌套 `details/summary`，summary 显示字段名或 `[index]`、JSON 类型和成员数量。根节点标题为
  “基本信息”且带 `open`，其余容器默认关闭；标量叶子仍在所属 Object 的 table 中展示。
- 删除 `$`、`$.KCCI_01` 等实现路径 caption；完整 Credit 测试按反序列化后的真实容器数逐一核对
  `details/summary` 开闭标签，证明没有只渲染部分树。
- `ProtocolSafeDisplay` allowlist 增加无脚本原生 `details/summary`，只为 `details` 复制布尔 `open`
  属性；`ontoggle` 与其他事件属性仍被删除。增加树缩进、summary 元数据和暗色样式，并保留
  `TASK-20260903-009` 已加入的逐表横向滚动 wrapper/CSS。
- TDD RED：包内两个新断言在旧平铺输出上 2/2 失败；安全 Display 新测试在旧 allowlist 上 1/1
  失败、其余 12 项通过。实现后包级 14/14、Host Component 1/1、安全 Display 13/13 PASS。
- `cargo fmt`、包与 Host strict Clippy、目标 ESLint、TypeScript typecheck、Wasm release build、
  Next.js production build（13 routes）、`git diff --check` 和 `deno.lock` 无副作用检查全部 PASS。
- 最终 Wasm 为 537608 bytes，SHA-256
  `98f8a15009f52dda74a05c40b802dbe0f1ff0027294da76d67d1511e57df6074`；证据快照与活动产物逐字节一致。
- 对抗检查覆盖容器完整性、默认开闭、HTML 转义、事件属性、CSP/iframe、资源门禁、暗色样式和
  横向滚动共享改动；未发现未解决 P0/P1/P2。
- 运行中桌面 App 的点击式视觉验收、真实 Server、Android 设备和 CI 均 `NOT_RUN`；这些不替代已完成
  的源码、Host 和 production build 验证。
- 测试证据：[`gmofg-dll-json-tree-display`](../../../testing/evidence/2026-09-03/TASK-20260903-010/gmofg-dll-json-tree-display/README.md)。
