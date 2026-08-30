# 嵌套 Document、统一规则与 JavaScript WebSocket 软件包

## 任务信息

- 任务 ID：`TASK-20260829-002`
- 状态：`进行中`
- 任务日期：`2026-08-29`
- 创建时间：`2026-08-29 20:28:30 +08:00`
- 开始时间：`2026-08-29 22:55:17 +08:00`
- 最后更新时间：`2026-08-31 06:43:40 +08:00`
- 完成时间：`N/A`
- 创建路径：`docs/tasks/pending/2026-08-29/nested-document-rules-javascript-websocket-packages.md`
- 归档路径：`docs/tasks/completed/2026-08-29/nested-document-rules-javascript-websocket-packages.md`
- 关键词：`recursive Document`、`RFC 6901`、`unified rules`、`JavaScript`、`Boa`、`Sidecar`、`WebSocket JSON-RPC`、`manifest.json`、`HTTP Body`、`Socket`
- 任务优先级：`高`
- 优先级理由：涉及数据库 100 最终基线、Document 与规则公共领域模型、HTTP/Socket 编排、协议包 Manifest/RPC 公共合同、Sidecar 进程生命周期、持久化、UI 和跨平台打包；失败可能造成错误报文、错误业务结果、端口或进程泄漏及不可恢复状态。
- 前置任务：`TASK-20260829-001`。本任务保留数据库版本 100，但替代该任务把旧结构 100 直接作为正式兼容基线的结论。

## 背景与目标

当前代码仍包含扁平 Document、旧四阶段消息规则、Rhai/TOML 本地包及本地/远程不同运行路径。它们无法直接承载已经确认的递归数据、统一 HTTP/Document 规则、顺序可见的多动作规则、方向级原子执行以及统一外部软件包协议。

本任务在产品 1.00 发布前直接替换旧合同，形成一套协议中立的递归 Document、一套应用层统一规则编排和一套 WebSocket 外部软件包协议。本地 Sidecar 只是由 App 管理并在本机运行的外部软件包；本地 Sidecar 与远程软件包的方法名、调用方式、注册方式、结果及错误合同完全一致。Sidecar 内部 Boa 与 `Uint8Array`/Base64 转换只属于内部适配，不形成第二套 API。

## 范围与最终合同

### 1. 数据库 100 发布基线

- 最终数据库版本保持 `100`，不增加 `101`。
- 当前 `<100` 与旧结构 `100` 数据均为本地开发数据，不迁移、不兼容、不保留。
- 当前开发切换阶段每次 App 启动直接清空数据库并按最终 Schema 100 重建。
- 正式发布前必须删除“每次启动清空”逻辑，并以源码检查、自动化测试和真实 Release App 重启证明数据正常持久化。
- 不保留旧规则、旧 Rhai/TOML、旧 API 1 或旧四阶段兼容读写路径。

### 2. Recursive Document

- 值只包含 `string`、`number`、`boolean`、`null`、`object`、`array`；删除旧 `int` 与 `blob`。Socket Frame 字节位于 Document 外部。
- number 使用有限 IEEE-754 binary64 / JavaScript Number 语义；解析、编码遵循标准 `JSON.parse`/`JSON.stringify`。
- 解析后为整数的值必须位于 JavaScript safe integer 范围；拒绝 `NaN`、正负无穷，不扫描数字原文，`1e-400` 可成为 `0`。
- object 字段顺序不属于语义；重复 JSON key 按标准 `JSON.parse` 最后值覆盖，不增加检测器。
- 完整 Document 未发生语义变化时转发原始输入字节；发生语义修改后调用 Encode，不保证字段顺序、数字文本或空白格式。
- 权威路径为 RFC 6901 JSON Pointer；内部根路径为空字符串，UI 显示 `/`。覆盖 `~0`、`~1`、空字段名、Unicode 和 Array index。

### 3. Schema 与规则本地元数据

- Schema 是 Manifest 内联的 UI/path/type 元数据，不执行 Decode 后完整 Document 校验。
- Schema 类型只有 `string/number/boolean/object/array`，`null` 不是 Schema 类型；object 使用 `properties`，array 使用必填 `items`，节点可选 `title`。
- HTTP 两方向 Schema 可省略；Socket 两方向 Schema 均必填。Schema 可不完整，包 Schema 只读。
- 规则可保存 Schema 未声明路径和规则本地预期类型，不修改包 Schema。
- 无 Schema 时每条规则独立拥有自己的元数据树；普通新建规则从空树开始。
- 只有用户显式复制规则时才复制元数据、条件、动作及适用生命周期字段；副本随后独立。
- 条件与动作保存 RFC 6901 path string，不增加稳定节点 ID、自动引用更新或依赖分析。

### 4. 统一规则系统

- HTTP 上下文条件与 Decode 后 Document 条件/动作属于同一条规则，可组合 URL、Header、Method、Status 与 Document path。
- Document 领域层不依赖 HTTP；应用层组合类型化上下文和 Document 能力。Socket 复用规则生命周期和执行骨架，但不获得 HTTP 能力。TLS handshake 规则保持独立。
- 消息规则只保留 `ProxyToUpstream` 和 `ProxyToApp` 两个写出阶段，删除 `AppToProxy` 与 `UpstreamToProxy` 消息规则阶段。
- 每条规则包含一个非空条件树和一个非空有序动作列表；条件树支持任意嵌套 AND/OR，不支持 NOT 或空组。
- 规则按 priority 升序、相同 priority 按 Rule ID 升序执行；后一规则读取前面动作产生的当前 working state。
- 条件命中后按声明顺序执行动作；Mock、主动断连、模拟网络故障等终止动作必须是该规则最后一个动作，并停止后续规则。
- 整个方向规则链在私有 working Exchange 上执行。任何条件、动作、Encode 或提交校验失败都回滚 HTTP/Document 修改、控制效果和命中状态，并明确终止 Exchange；不得透传、部分提交、转成 no-match、重试或回退。
- 保留 `enabled`、`priority`、Nth-hit、one-shot、hit count、last hit time 和 revision 并发保护；仅整条规则链成功后提交生命周期状态。

### 5. 条件与动作

- string：`EQUAL`、`CONTAINS`、`STARTS WITH`、`ENDS WITH`。
- number：`EQUAL`、`LESS`、`LESS EQUAL`、`GREATER`、`GREATER EQUAL`。
- boolean/null：`EQUAL`。不做 string/number/boolean 隐式转换。
- 条件路径缺失或实际类型不匹配时为 false。
- Set 可替换已有节点、替换根 Document，或在已存在 object 父节点下增加最后一级字段；不创建中间节点、不扩展 Array、不填 null 空洞。缺失父节点、类型冲突和无效/越界 index 均失败。
- Clear 删除 object 属性或 array 元素并左移；禁止根 Clear；路径缺失或越界必须失败，不是 no-op。
- Insert/Append 的目标必须是已存在 array；Insert 接受 `0..=length`，超过 length 失败；不自动创建 array。Schema-bound 值必须符合目标 items 元数据合同。

### 6. HTTP Body 编码边界

- HTTP 协议包只处理 Body；transport 负责 framing 与 Header。
- 保留现有严格 UTF-8、Shift-JIS 及已支持别名；严格解码后调用包，修改后按原解析 codec 严格重编码。
- 非法字节、未知 charset、输出不可表示或非 identity Content-Encoding 均明确失败，不进行有损替换或原样绕过。
- 未绑定协议包时 HTTP 上下文规则仍可执行，但不能配置或执行 Document 条件/动作。

### 7. Manifest API 1

本地 `manifest.json` 与远程 `package.register.params` 完全同形：

```json
{
  "api": 1,
  "kind": "http",
  "package": {
    "id": "com.example.payment",
    "version": "1.0.0",
    "name": "Payment JSON",
    "description": ""
  },
  "document": {
    "upstream": {},
    "downstream": {}
  }
}
```

- 顶层只允许 `api/kind/package/document`，未知字段严格拒绝。
- `kind` 只允许 `http/socket`；一个包只属于一种 kind。
- `package` 必填 `id/version/name/description`；description 可为空。
- HTTP direction object 可为空或只含 schema；Socket 两个 direction object 均必须且只能含 schema。
- Manifest 不配置 Hook 名称、入口路径、Display 映射或 format version。
- 本地 ZIP 根固定包含 `manifest.json`、`protocol.js`、`display.js` 及其包内相对引用的 `.js` module；不支持目录加载、热更新、TypeScript 或任意入口配置。

### 8. 唯一 WebSocket JSON-RPC 软件包合同

- 软件包主动连接 `/packages`，主动发送无 `id`、无回复的 JSON-RPC notification `package.register`，`params` 为完整 Manifest。
- 首个合法注册的精确 package ID + version 立即成为 online + enabled。非法 envelope/Manifest/API/kind/Schema 或重复在线身份产生稳定诊断并立即关闭 WebSocket，不 takeover。
- 不增加 `/packages` 认证、token、来源证明或冒充防护。
- 固定方法与调用形状：

```text
hooks.upstream.frame / hooks.downstream.frame
params: { "buffer": "<Base64>" }
result: FrameResult

hooks.upstream.decode / hooks.downstream.decode
params: { "input": "<HTTP string 或 Socket Base64>" }
result: <Document JSON>

hooks.upstream.encode / hooks.downstream.encode
params: { "originalInput": "...", "document": <Document JSON> }
result: "<HTTP string 或 Socket Base64>"

document.upstream.display / document.downstream.display
params: { "document": <Document JSON> }
result: "<HTML string>"
```

- Hook 调用是带 `id` 的 JSON-RPC request，严格返回 result 或 error；params 拒绝未知或缺失字段。
- HTTP input/result 为 Unicode string；Socket wire 为 canonical padded Base64；Document 为自然递归 JSON tree。
- FrameResult 为 closed union：`need_more`（可选 `requiredBytes`）、`complete`（必填合法 `consumedBytes`）或 `reject`（必填 reason）。reject 与 Hook error 终止 Exchange。
- Domain 使用类型化错误；JSON-RPC 在 `error.data.code` 暴露稳定 code；Tauri/UI 使用相同 code。不得依赖 message 判断类型。

### 9. Local Sidecar 与 Boa

- 使用一个通用 Rust Sidecar executable；每个 enabled 精确本地包版本对应一个独立进程、一个执行线程和一个 Boa Context。
- `protocol.js` 固定 exports：Socket 的 `upstreamFrame/downstreamFrame`，以及 `upstreamDecode/downstreamDecode/upstreamEncode/downstreamEncode`。
- `display.js` 固定 exports：`upstreamDisplay/downstreamDisplay`。
- 每个固定 export 接收与公共 JSON-RPC 方法同形的单一 params object，返回同形结果。Sidecar 在内部完成固定 RPC method 到固定 export 的映射。
- Socket Base64 在 Sidecar 内转换为 `Uint8Array`，结果反向转换为 canonical Base64；这是内部实现细节。
- 入口 Module 及静态相对依赖在启动时 load/link/evaluate，固定 exports 缓存到进程退出；合法 `dynamic import()` 由 Boa 原生 module loader 在 Hook 调用时加载和求值。注册前只验证所需 export 存在且 callable，不试运行、不探测业务、不自动修复。
- 不人为限制 Boa 自身提供的 ECMAScript 能力或 default features；Proxy 不额外注入 Boa 本身没有的 Node.js、fs、process、Buffer、fetch、timer、WebSocket 等 Host bindings。
- 不设置 Hook timeout、应用层队列上限、busy 拒绝、自动限流、自动中断、自动重试或恢复；脚本长期执行和队列占用由包作者修复脚本。

### 10. 安装与生命周期

- 本地唯一交付单元为 ZIP；内置 JSON、内置 ISO8583 和第三方本地包走同一 ZIP 导入与 Sidecar 链。
- 本地 ZIP 静态校验并提交后立即 enabled，启动 Sidecar 并等待最多 10 秒注册。失败保留包和 enabled 状态，显示 failed，不自动重试。
- App 启动只并行后台启动 enabled 本地精确包版本，不阻塞主 UI。disabled 包不运行 Sidecar；引用 disabled/offline 包的 Listener 不允许启动。
- 手动重启强制终止旧 Sidecar；当前与排队 RPC 失败且不重放，再启动新进程。
- 异常断连使当前/排队 RPC 失败，立即停止所有引用该精确版本的 Listener 并释放端口；恢复后 Listener 不自动启动。
- 删除被 Listener 引用的包时拒绝并列出引用；无引用时停止进程并确认退出后删除。
- App 关闭时强制终止全部本地 Sidecar，不等待 Hook 完成，并验证无孤儿进程。
- 远程包首次合法注册立即 online + enabled；断连后保留 enabled 元数据但标为 offline。

### 11. UI 与观察边界

- 统一规则使用 modal，不保留固定右侧编辑栏。
- 上部为 Document/metadata tree，下部为递归 AND/OR condition tree，动作使用有序列表编辑器。
- Document tree 只显示每节点条件数量；Schema tree 只读，Schema-free tree 属于当前规则并可编辑。
- 普通新建规则为空树；显式 copy 复制完整规则。Array 显示 items 类型及用户创建的具体 index，不显示 Item template row。
- UI 只消费 Rust 返回的能力和稳定 error code，最终保存由 Rust Domain 再校验。
- Capture/Session 展示原始 Decode Document、逐规则变化/最终 working Document、Encode/Sent 结果和稳定失败阶段。
- Display 在 Decode 后读取原始 Document，只产生观察 HTML。Display 失败不阻断业务，显示稳定错误并回退 HTTP 原始 Body 或 Socket Hex；Frame、Decode、Rules、Encode 或 transport 失败仍终止 Exchange。

### 12. 发布目标与交付边界

- 正式发布目标：Windows x64 MSI/NSIS，以及 macOS Universal App/DMG。Linux 只保持编译与测试。
- 本次执行边界为实现、本地提交、本地测试、macOS 实际安装包和审查报告。
- 未经用户后续明确批准，不 push、不触发远程 CI、不创建 Release。
- Windows 真实 runner/安装包验证保持 `NOT_RUN`，直至用户批准推送和远程 CI。

## 不在范围与已接受风险

- XML、TypeScript、完整 JSON Schema、Node/Web Host API、HTTP 二进制 Body、内容编码自动解压和按 Content-Type 自动选包。
- 旧数据库、旧规则、旧 Rhai/TOML、旧 API 1、旧四阶段规则或本地进程内包兼容。
- `/packages` 身份认证与来源安全。
- Hook timeout、应用层队列容量、busy 拒绝、自动限流、自动重试、自动恢复、自动 takeover。
- 规则路径稳定节点 ID、自动引用修复、依赖分析和用户可自行修正问题的额外 Proxy 判断。
- Linux 正式安装包与未经授权的 push、远程 CI、Release。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-08-29 20:28:30 +08:00` | 建立初始 TASK-002，但当时的单动作、原始快照匹配、自动复制树、四阶段残留和可配置 Hook 等结论尚未完成最终访谈。 |
| `2026-08-29 20:30:00 +08:00` 至 `2026-08-29 21:55:00 +08:00` | 用户逐项确认数据库 100 开发清空、递归 Document、Number/JSON、Schema、统一规则、多动作顺序、方向级原子性、严格 Set/Clear、HTTP codec、Manifest、RPC、生命周期、UI、跨平台与交付边界。 |
| `2026-08-29 21:55:00 +08:00` | 用户确认 `package.register` 为无 id、无回复的单向 notification。 |
| `2026-08-29 22:00:00 +08:00` | 用户最终确认本地 Sidecar 是本机运行的外部软件包，与远程包的方法名、调用方式、注册方式完全相同；内部 Boa 适配不构成第二套协议。 |
| `2026-08-30 22:35:02 +08:00` | 用户明确覆盖 Phase8 旧安全限制：不考虑安全问题，不人为限制 Boa 自身能力；旧“不提供 Host API”“启动时一次图冻结”“错误脱敏”验收失效。新验收为 Boa default features/原生能力不受 Proxy 限制、允许合法 `dynamic import()`、错误无需安全脱敏；Proxy 仍不额外发明 Boa 本身没有的 Node/fs/process 等 bindings。受影响的 Host API absence、dynamic-import reject、secret-redaction tests/checker 必须删除或反转；固定八 exports、单 Context、ESM、HTTP string、Socket `Uint8Array`/Base64 和公共 JSON-RPC 合同不变。 |

旧任务正文中与本节最终合同冲突的初始结论全部失效，不得作为实现依据。

## 未确认事项

无会改变实现方向的未确认事项。具体 Rust 类型名、文件拆分和模块内部私有接口在架构计划中确定，但不得改变本任务公共合同或增加新业务语义。

## 需求就绪检查

- 问题、目标与成功结果：`PASS`
- 范围与不在范围：`PASS`
- 输入、输出、状态变化和错误行为：`PASS`
- 具体 Manifest、注册、Hook、Frame、规则、Document、生命周期示例：`PASS`
- 可重复 PASS/FAIL 验收：`PASS`
- 会改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-08-29 22:55:17 +08:00`；Planner → Architect → Critic 共识规划已通过，现有 Ultragoal 质量基线已纳入本任务并开始执行。

## 问题与根因分析

本任务是发布前合同替换，不是单一缺陷修复。

- 实际现象：当前模型与已确认发布合同在数据模型、规则阶段、执行顺序、包运行路径和持久化结构上不一致。
- 预期依据：本任务“范围与最终合同”以及用户逐项确认记录。
- 当前已验证：旧代码/文档仍含 flat Document、Rhai/TOML、本地/远程不同执行入口和旧消息规则阶段；实现开始前须再次按当前 HEAD 精确映射。
- 推断：在旧抽象上继续增加兼容分支会扩大双路径与测试矩阵。
- 未知：无产品合同未知；具体代码影响面由架构规划和实施前源码映射确认。
- 根因：旧发布前原型的领域边界与已确认最终合同不同，不存在保持旧模型的局部修复。
- 正确边界：保留稳定的 Exchange/Pipeline 分层意图，直接替换旧领域模型、应用编排和包基础设施，不保留兼容路径。

## 最小改动与最优设计比较

| 方案 | 结论 |
| --- | --- |
| 在旧 flat Document、Rhai、旧 API 1 和四阶段规则上增加兼容分支 | 修改表面较少，但长期保留双数据模型、双执行路径和不确定迁移；违反明确的不兼容替换合同，拒绝。 |
| 只把 Rhai 语法换成 JavaScript，继续本地进程内执行 | 无法统一本地/远程注册、方法、身份和生命周期，拒绝。 |
| 保留领域/应用/基础设施依赖方向，整体替换 Document、规则、Manifest、RPC、Sidecar 和 UI | 能删除旧路径、统一合同并形成可演进边界，采用。 |

采用方案必须遵循：领域层不依赖 HTTP、Socket、Tauri、WebSocket、数据库或 Boa；应用层拥有统一规则用例和端口；基础设施实现 codec、协议包 RPC、ZIP、Sidecar、存储与 transport；UI 只消费应用层状态和能力。

## 小任务与依赖

| ID | 任务 | 依赖 | 可并行 | 状态 | 验收 |
| --- | --- | --- | --- | --- | --- |
| NDR-JS-01 | 映射当前源码，锁定旧行为与新合同 RED 测试；加入开发期 DB100 每启动重建及正式发布移除门禁 | TASK-20260829-001 | 否 | 进行中 | G042 / Phase 1 已 VERIFIED；G043 / Phase 2 独立 delta 复审 APPROVE，P0/P1/P2=0，可创建 rollback checkpoint |
| NDR-JS-02 | 实现递归 Document、Number、RFC6901、Schema 和规则本地元数据 | NDR-JS-01 | 否 | 待实现 | 全类型、JSON、路径、数组、Schema/no-Schema 测试通过 |
| NDR-JS-03 | 实现统一 HTTP/Document/Socket 规则、两写出阶段、多动作顺序、终止动作和方向级原子提交 | NDR-JS-02 | 否 | 待实现 | 条件/动作矩阵、前序可见、失败全回滚及生命周期提交通过 |
| NDR-JS-04 | 定义严格 Manifest、稳定错误和唯一 JSON-RPC wire | NDR-JS-01 | 否 | 待实现 | 本地/远程逐字段同形，注册与 Hook contract tests 通过 |
| NDR-JS-05 | 实现通用 Boa Sidecar、固定 exports、ESM 加载和内部字节适配 | NDR-JS-04 | 否 | 已完成 | G049 / Phase 8 已 `VERIFIED / APPROVED / CHECKPOINT READY`；P0/P1/P2=0，`blockers=[]` |
| NDR-JS-06 | 实现 ZIP 安装、包注册表和 enabled/online/failed/disabled 生命周期 | NDR-JS-05 | 否 | 待实现 | 10 秒注册、冲突、启停、断连停 Listener、删除与无孤儿进程通过 |
| NDR-JS-07 | 将 HTTP/Socket Pipeline 切换到统一规则和唯一 WebSocket 包端口，删除旧执行路径 | NDR-JS-03、NDR-JS-06 | 否 | 待实现 | 两方向完整 Pipeline、codec、失败边界与原始字节保留通过 |
| NDR-JS-08 | 将内置 JSON、ISO8583 和第三方本地包迁移为同一 ZIP/Sidecar；保留远程同协议接入 | NDR-JS-07 | 可按包并行 | 待实现 | 四类包共享同一注册/RPC/capability 链，测试向量一致 |
| NDR-JS-09 | 实现统一规则 modal、元数据树、条件树、多动作、Capture/Session 状态与 stable error 展示 | NDR-JS-02、NDR-JS-03、NDR-JS-06 | 否，共享合同稳定后 | 待实现 | Schema/no-Schema、copy、可访问性、错误和过程展示通过 |
| NDR-JS-10 | 删除开发期 DB 清空逻辑及所有旧 Rhai/TOML/flat/four-stage/API1 路径，固化正式 Schema100 | NDR-JS-07、NDR-JS-09 | 否 | 待实现 | Release 重启持久化、旧入口搜索为零、数据结构一致 |
| NDR-JS-11 | 同步架构、ADR、用户、协议包、MCP、测试矩阵和模板文档 | NDR-JS-08、NDR-JS-09 | 合同冻结后可并行 | 待实现 | 文档与生产 JSON、调用链和发布边界一致 |
| NDR-JS-12 | 全层验收、macOS Universal App/DMG、真实 HTTP/Socket E2E、对抗审查和本地提交 | 前述全部 | 否 | 待实现 | 本地门禁通过，无 P0/P1/P2；Windows 真实 CI 明确 NOT_RUN |

共享 Document、规则、Manifest、RPC、包身份、生命周期和持久化合同稳定前不得并行修改。主 Agent 在实施批次开始前确定文件所有权和集成顺序。

## 测试计划

### 领域、序列化与持久化

- Document 六类值、深层 object/array、Unicode、空键、`/`、`~`、RFC6901 根与转义。
- finite number、安全整数、NaN/Infinity 拒绝、`1e-400`、重复 key last-wins、字段顺序非语义。
- Schema 递归、title、HTTP 可省略、Socket 必填、null runtime、额外/缺失字段及无 Schema 独立树/显式复制。
- 统一条件树、多动作顺序、前序修改可见、终止动作位置、严格 Set/Clear/Insert/Append、方向级失败回滚。
- enabled/priority/Nth-hit/one-shot/count/last-hit/revision 只在整体成功后提交。
- 开发期每启动重建；正式门禁删除该逻辑后，Release App 多次重启数据仍存在。

### Manifest、RPC 与 JavaScript

- Manifest 严格字段、HTTP/Socket Schema 差异、未知/缺失字段拒绝及本地/远程同形。
- 软件包主动 notification 注册，无 id、无 response；合法 online+enabled，非法或重复记录 code 后立即断开。
- 固定 RPC methods、params 严格字段、Document result、HTTP string 和 Socket canonical Base64 逐字节往返。
- Frame closed union、requiredBytes、consumedBytes、reject、JSON-RPC error 和稳定 `error.data.code`。
- 固定 exports 注册前 existence/callable 预检并缓存；入口和静态相对 ESM 正常链接求值，合法 dynamic import 由 Boa 原生 loader 执行；不限制 Boa 自身能力，也不额外注入 Node/fs/process 等 bindings。
- 同一 Boa Context 串行执行；WebSocket I/O 与 heartbeat 不由 Boa Hook 线程阻塞；无 timeout/队列上限合同不被隐藏限额改变。

### 生命周期、数据面与 UI

- ZIP 导入、静态验证、enabled 提交、10 秒注册、失败保留、disabled 不启动、无自动重试。
- 手动重启、异常断连、pending RPC 失败、不重放、停精确 Listener、释放端口、恢复不自动启 Listener。
- 引用删除拒绝、无引用停止后删除、App 关闭强杀及无孤儿进程。
- HTTP UTF-8/Shift-JIS、未知/非法/不可表示 charset、非 identity content-encoding；Socket Frame/Base64/Decode/Rules/Encode 双端字节。
- 未修改完整 Document 原始字节转发；修改后 Encode；Display 观察失败不阻断，其他阶段失败阻断。
- 内置 JSON、ISO8583、本地第三方与远程软件包走同一注册、方法和能力链。
- modal、Document/metadata tree、条件树、有序动作列表、Schema 只读、无 Schema 独立/显式 copy、Capture/Session、键盘/ARIA/截图。

### 构建、安装包与证据

- Rust 目标测试、workspace test、strict Clippy、fmt、架构边界和 source-size。
- Frontend typecheck、lint、规则 UI 测试、生产 build 和可访问性。
- 保存 Manifest、注册、RPC、Frame、Decode、原始/逐步/最终 Document、Encode、Server/App 实际字节、进程和 UI 证据。
- macOS Universal Release App/DMG 本机真实启动、导入、Sidecar E2E、重启持久化与退出清理。
- Windows 源码/交叉静态验证；真实 runner、MSI/NSIS 和远程 CI 在未获批准前记为 `NOT_RUN`。

## 对抗审查计划

- 独立 Architect 审查领域/应用/基础设施依赖方向、规则事务边界、资源所有权和长期演进性。
- 独立 Code Reviewer 检查残留 Rhai/TOML/flat/int/blob/四阶段/旧注册路径、隐性双 API、部分提交、隐藏 timeout/队列上限和消息字符串错误判断。
- 检查本地/远程是否真正共用相同 WebSocket method/params/result/error/register，而不只是外观相同。
- 检查开发期 DB 清空逻辑是否在正式构建前被删除并有防回归门禁。
- 检查 Sidecar 启停、断连、删除、App 关闭是否残留进程、端口、任务或 pending RPC。
- 完成前不得存在未处理 P0/P1/P2；无法执行的高层验证必须明确 `NOT_RUN`，不得由低层 PASS 替代。

## 文档影响

- 重写 `docs/architecture/rules-and-protocol-packages.md` 和 Exchange/Pipeline 规则阶段说明。
- 新增或替换 JavaScript 软件包模板、Manifest/RPC 规范、远程接入、ZIP 导入与生命周期指南。
- 更新用户操作、需求基线、模块说明、开发指南、MCP 文档、测试矩阵和发布检查表。
- 以新 ADR 记录 API 1 发布前重定义、统一 WebSocket 边界、Boa Sidecar 所有权和 Schema100 切换；保留历史 ADR 的替代关系。

## 实施记录

- `2026-08-29 20:28:30 +08:00`：建立初始任务；任务正文保留了多项未最终确认的原型结论。
- `2026-08-29 20:30:00 +08:00` 至 `2026-08-29 22:03:35 +08:00`：完成逐项深度访谈，形成最终合同；尚未修改产品代码。
- `2026-08-29 22:42:45 +08:00`：完成 `$ralplan` deliberate 共识规划。Planner 经三轮 Critic 修订形成 18 个可编译检查点；Architect 与 Critic 最终均为 `APPROVE`。本结论只批准执行计划，产品代码、测试和验收仍为 `NOT_RUN`。
- `2026-08-29 22:47:35 +08:00`：用户确认把现有全仓质量重构 Ultragoal 纳入本任务，并要求在新分支完成。创建分支 `codex/task-20260829-002`；通过结构化 steering 把本任务拆为 G042–G059 十八个顺序故事，保留既有完成证据和共享基线门禁。
- `2026-08-29 22:47:35 +08:00`：将旧 G040 自动推送目标修订为“等待用户后续明确批准”；未经批准继续禁止 push、远程 CI 和 Release，Windows runner/MSI/NSIS 保持 `NOT_RUN`。
- `2026-08-29 22:55:17 +08:00`：独立 Verifier 对现有 G036 共享基线执行 fresh 验证，功能、Rust/前端/架构门禁全部 PASS，但发现写能力 MCP 服务仍命名 `ReadOnlyMcpServer`，定级 P2；在行为不变重命名并复验前禁止 checkpoint。
- `2026-08-29 23:30:18 +08:00`：形成 G042 / Phase 1 初版基线。新增机器可读 current-contract inventory，登记旧 Schema100、flat Document、四阶段规则、Rhai/TOML、Socket 专用 RPC、generated types 和可复用 Rust/Vitest harness；未加入 ignored/disabled 或故意失败测试，未修改产品运行行为。该初版随后未通过独立 Verifier，不作为合格 checkpoint。
- `2026-08-29 23:30:18 +08:00`：初版统一 phase checkpoint 只包含 Phase 1 清单、generated bindings、`pnpm typecheck` 与普通 Rust workspace tests；独立 Verifier 后续判定命令集不完整。checkpoint 仍遵守：临时编译适配器不得形成第二 runtime path，下一阶段开始前必须存在本地 Git rollback point。
- `2026-08-29 23:30:18 +08:00`：针对性测试审查首次发现 generated bindings freshness/determinism 缺口并加入检查；当时记录的“P1 已修复”和失败路径恢复结论撤回，因为后续独立 Verifier 证明 generator 删除输出后抛错会在 `finally` 读取时产生 `ENOENT`，覆盖原 generator 错误且未恢复文件。
- `2026-08-29 23:44:04 +08:00`：G042 独立 Verifier 结论为 `FAILED`，禁止 checkpoint。除 bindings 恢复缺陷外，还发现 checkpoint 缺少架构、源码大小、lint、完整前端测试、Rust fmt/clippy 与 all-target/all-feature workspace tests；four-stage ownership 错归 Phase 5，前端 rule-definition harness 也未拆分 Phase 5 `created_order` 与 Phase 12 四阶段删除责任。
- `2026-08-29 23:44:04 +08:00`：完成上述 Verifier 问题修复。bindings `finally` 改为不依赖输出存在性的无条件原字节写回，并新增 unlink-then-throw 回归，验证写回 checked-in bytes 且传播同一个 generator error；checkpoint、inventory 与 validator 统一为十条严格有序命令并 fail-closed；four-stage 统一归 Phase 12，前端同一测试文件按 Phase 5 rule order 与 Phase 12 four-stage 两项 harness 分拆。fresh 本地 Node tests、真实 bindings 和完整 checkpoint 均 `PASS`；G042 仍等待独立 Verifier 复验、正式证据和本地 rollback commit，主 Agent 复验前不得 checkpoint。
- `2026-08-29 23:54:37 +08:00`：G042 独立复验完成，结论 `VERIFIED`，P0/P1/P2 均为 0。首次完整 checkpoint 的前九项 PASS，但最后 Rust workspace 中既有 ADB deadline 测试偶发失败一次（647 passed / 1 failed）；同一测试随后定向连续 3/3 PASS，完整十门禁复跑 exit 0。首次失败、精确测试名、panic、定向复跑及最终结果已完整保存到正式证据，不把首次失败隐藏或改写为成功。
- `2026-08-29 23:54:37 +08:00`：正式证据归档完成：[phase1-green-contract-baseline](../../../testing/evidence/2026-08-29/TASK-20260829-002/phase1-green-contract-baseline/README.md)。G042 / Phase 1 标记已完成；NDR-JS-01 仍需下一阶段完成开发期 DB100 启动重建门禁，因此子任务和任务总体均保持进行中。
- `2026-08-30 00:34:03 +08:00`：G043 / Phase 2 按独立测试预检重塑为明确分层：Tauri debug composition root 显式 opt-in `RecreateCurrent`，Host builder 默认 `Preserve`，Tauri Release 显式 `Preserve`，Infrastructure 现有唯一异步 SQLite open 入口接收 policy；`AppState` 与 `ProductProfile` 不参与。初始把 Host 所有 debug 启动默认清空并用 `cfg!(debug_assertions)` 合并测试的错误 RED 已撤回，未作为 checkpoint。
- `2026-08-30 00:34:03 +08:00`：Infrastructure `RecreateCurrent` 在打开连接后关闭 FK、使用一个 `BEGIN IMMEDIATE` 事务删除全部非 `sqlite_%` trigger/view/table、初始化当前 Schema100 并提交，成功或失败均恢复 FK；不先解析、接受或迁移旧版本。覆盖 `<100`、旧 layout100、当前100、committed WAL、失败全回滚/FK 恢复及 Host 构建失败传播。Schema 版本仍为 `100`，未引入迁移或第二 runtime path。
- `2026-08-30 00:34:03 +08:00`：同一双启动 helper 的第一次 Host 通过公开 Application/Host 能力创建唯一 Workspace、disabled Listener、带真实 revision/hit_count/last_hit_at/one_shot 的统一 Rule，并通过真实本地 ZIP `prepare/commit` 导入 Package，关闭前逐字段 readback；第二次显式 Recreate 按唯一 identity 验证全部不存在但不要求表空，默认 Preserve 分支逐字段验证保留，供 Phase 17 原 helper 反转复用。未使用 raw SQL 伪造 Package 或 lifecycle sentinel。
- `2026-08-30 00:34:03 +08:00`：加入紧贴 Tauri debug opt-in 的唯一 marker `TASK_20260829_002_PRE_RELEASE_DATABASE_RESET`、fail-closed release checker 与初版 Node 自测。当前 release scan 按设计输出 `NOT_RELEASE_READY` 并 exit 1，不纳入日常 checkpoint；Phase 17 必须删除整个临时 startup policy/runtime branch 与 marker，并使同一 checker PASS。
- `2026-08-30 00:34:03 +08:00`：正确 Infrastructure RED 以缺少 `recreate_current_schema*` 的 `E0425`、exit 101 失败；GREEN 后 targeted tests 为 Infrastructure core 6/6、Host policy 3/3。首次 source-size 因 Host tests 文件 696 行失败，拆分 Phase 2 专用测试模块后在不放宽 500 行阈值的情况下通过。受影响 crates 全目标/全特性、Release Tauri compile、strict Clippy、Phase 1 十门禁 checkpoint 与 `git diff --check` 均 fresh PASS；正式证据：[phase2-development-database-recreate](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase2-development-database-recreate/README.md)。结论为本地 `GREEN / RECHECK PENDING`，任务总体继续进行中。
- `2026-08-30 00:50:42 +08:00`：G043 第一次独立 Verifier 结论为 `FAILED`：checker 只计数 marker，删除 marker/日志但保留实际 `RecreateCurrent` 会假 PASS。Reviewer 同时发现 checker 未接入 `tauri:build`、成功日志发生在 tracing subscriber 安装前而不可观察、双启动 absence 仅用 `is_err()` 无法证明目标 identity 不存在。以上结论未隐藏或改写为成功。
- `2026-08-30 00:50:42 +08:00`：修复后 checker 只扫描六个显式生产 Rust 文件，独立阻断 marker 与 `SqliteStartupPolicy` / `DatabaseStartupPolicy` / `RecreateCurrent` / policy 注入及 open branch；只删除 marker、opt-in 或 policy 任一部分仍失败，全部删除才 PASS。`tauri:build` 现先串行执行同一 checker，当前在 Android companion build 与 `tauri build` 前预期阻断，不影响 `tauri:dev` 或普通 Cargo check。删除了 subscriber 安装前的无效日志；Workspace 通过公开 list 精确 ID、Package 通过公开 list 精确版本 identity 断言不存在，Listener/Rule 作为 Workspace 聚合成员随聚合消失。
- `2026-08-30 00:50:42 +08:00`：fresh 修复验证为 Node 8/8、Infrastructure core 6/6、Host policy 3/3；当前 release scan exit 1，独立报告 1 个 marker 与 32 个临时 reset contract 引用；`pnpm tauri:build` 预期 exit 1；Release Cargo check、完整十门禁 checkpoint（前端 61/531、Rust workspace 0 failed）与 `git diff --check` 均 PASS。当前仍为 `GREEN / RECHECK PENDING`，不得创建 checkpoint。
- `2026-08-30 00:59:11 +08:00`：复审新增 P1：通用 package script `"tauri": "tauri"` 允许 `pnpm tauri build` 绕过只挂在 `tauri:build` alias 的早期 gate，因此上一轮仍不可 checkpoint。修复把同一只读 checker 接入 `src-tauri/tauri.conf.json` 的 `build.beforeBuildCommand` 并保留后续 `pnpm build`；`beforeDevCommand` 仍为 `pnpm dev`。package alias 的早期 gate 保留，用于在 Android companion build 前阻断；Tauri 配置 gate 覆盖通用 package script 与直接 CLI。双调用可接受，因为 checker 只读且确定性：第一层避免 companion 副作用，第二层封闭所有 Tauri build 入口。
- `2026-08-30 00:59:11 +08:00`：配置回归先 RED（实际 `beforeBuildCommand` 仅为 `pnpm build`），修复后 Node 8/8；`pnpm tauri:build` 在 Android companion 前预期 exit 1，`pnpm tauri build` 明确进入 `beforeBuildCommand` 并在 `pnpm build` 前预期 exit 1。Phase 2 8/8+6/6+3/3、Release Cargo check、完整十门禁 checkpoint（前端 61/531、Rust workspace 0 failed）及 `git diff --check` fresh PASS；状态继续 `GREEN / RECHECK PENDING`，等待新一轮独立复验。
- `2026-08-30 01:03:11 +08:00`：G043 修复后的独立 delta 复审结论为 `APPROVE`，P0=0、P1=0、P2=0。复审确认 package `tauri:build` 与通用 `pnpm tauri build` 两入口均由同一只读 checker fresh 预期阻断，Node 8/8 与 `git diff --check` PASS；G043 现在可创建 Phase 2 rollback checkpoint。任务总体仍为进行中，当前 `NOT_RELEASE_READY` 是 Phase 2 明确保留至 Phase 17 的发布阻断合同。
- `2026-08-30 03:01:55 +08:00`：G044 / Phase 3 将 Document 替换为无 identity 的递归 owned value object：String、finite JavaScript Number、Boolean、Null、Object、Array；JSON integer 超 safe range、NaN/Infinity fail-closed，标准 JSON 保留 `1e-400 => 0` 与 duplicate-key last-wins。新增唯一 RFC6901 pointer 与 Set/Clear/Insert/Append 语义；Schema 改为无 identity/version 的递归 `DocumentSchemaNode`，只含 string/number/boolean/object/array、optional title、object properties 与 required array items。
- `2026-08-30 03:01:55 +08:00`：同阶段迁移 Domain、Application、Exchange、protocol-scripting、Infrastructure、Tauri commands/MCP、generated bindings、即时前端 guards/tests、Nuvei 与 ISO template；删除 Int/Blob/flat slots/field schema、`ProtocolPackageSchemaViewModel.version` 与 rule `schema_version`，未增加 compatibility alias、第二模型或硬编码 `1`。Phase 4 package contract 与 Phase 5 condition/action execution replacement 未提前实施。
- `2026-08-30 03:01:55 +08:00`：活动 Nuvei source 和 deterministic ZIP 因 recursive Document 更新为 SHA-256 `047fe2701973d860d40fe30f5c74a735e46934d808ffb7dd1f16bf404460e30b`；Phase 2 evidence 中 SHA `0595af...` 的旧资源保持不可变。本次正式证据按 AGENTS 10.4 记录 `derived_from` 父任务、父用例、父证据与父资源。
- `2026-08-30 03:01:55 +08:00`：首次 checkpoint 因 active Phase 1 inventory 仍要求已删除 flat fragments 失败，更新 current-state fragments 后 Node 4/4；第二次因 `registration.rs` 502 行触发 source-size，仅压缩空行至 498 行且不放宽门禁；第三次前端 1/531 失败定位到 Socket package dialog 仍使用旧 flat fixture，frontend owner 迁移后定向 7/7、全量 531/531。最终十门禁 exit 0，workspace all-target/all-feature、strict Clippy、bindings/typecheck 与四类旧合同零残留扫描均 PASS。正式证据：[phase3-recursive-document-contract](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase3-recursive-document-contract/README.md)。当前为 `LOCAL GREEN / RECHECK PENDING`，任务总体保持进行中。
- `2026-08-30 03:57:42 +08:00`：G044 独立 Verifier 判定初版 `FAILED`，撤回此前“旧合同零残留且可 checkpoint”的过强结论。finding 包括 integral `f64` safe-integer 漏检、Decode/transform 后错误执行完整 Document-vs-Schema 校验、Schema 未声明 rule path 被拒绝、generated `DocumentNumber` 为 `number | null`、Null 错导出为字符串 `"Null"`、旧 `ClearDocument`/`clear_document` 仍暴露、public Program 未校验 Schema definition，以及过时注释/Blob limit 残留。
- `2026-08-30 03:57:42 +08:00`：完成上述修复：所有 integral `f64` 统一 safe-integer 边界；Schema 只验证自身递归 definition 并作为可不完整 metadata；未声明 path 依规则本地类型合同保存；真实 null 与字符串 `"Null"` 分离；删除旧 ClearDocument capability/generated/MCP 合同；public Program 构造 fail-closed 校验 Schema definition。fresh Phase 3 Node 11/11、Domain 87/87 + integrations 14/14 + 7/7、protocol-scripting 160/160、前端 62 files/534 tests、精确旧合同扫描 0、`git diff --check` PASS。
- `2026-08-30 03:57:42 +08:00`：修复后的第一次完整 checkpoint 在第 8 门仅因 Rust 测试排版不符合 rustfmt 停止；标准格式化后完整十门 checkpoint fresh exit 0，bindings/architecture/source-size/lint/typecheck/frontend/fmt/strict clippy/workspace all-target/all-feature 全部 PASS。更早一次全量前端 focus restore 偶发 1/534，定向 1/1 与后续完整 534/534 PASS，未隐藏。当前为 `修复完成 / LOCAL GREEN / RECHECK PENDING`，等待独立复验；G044 仍不可 checkpoint，任务总体继续进行中。
- `2026-08-30 04:09:47 +08:00`：最终复审新增 P2：Application facade `protocol_rule_values.rs` 仍保留过时 Int/Bool/Blob Hex 文本合同、`MAX_PROTOCOL_RULE_INT_TEXT_BYTES` 和整数文本错误。最小修复删除旧预算/文案，Number 与 Boolean 统一走标准 JSON + Domain recursive value 校验；非法 JSON 与合法但类型错误分别保持现有 typed `JSON_INVALID` / `PROTOCOL_RULE_VALUE_INVALID`，不增加兼容、抽象或错误映射。fresh Application 定向 4/4、Phase 3 Node 14/14、workspace strict Clippy、bindings freshness/determinism、精确旧 scalar-text 扫描 0、`git diff --check` PASS。状态回到 `修复完成 / LOCAL GREEN / RECHECK PENDING`，等待最终独立复验。
- `2026-08-30 04:13:38 +08:00`：最终独立 Verifier fresh 重跑精确十门 checkpoint 全部 PASS；新增 facade 回归使 Application unit 实际计数为 459/459。修正 README、metadata、verification summary 与任务测试记录中不再准确的旧计数；不改产品与测试。正式 verdict 尚未到达，继续保持 `RECHECK PENDING`、`checkpoint_ready=false`。
- `2026-08-30 04:16:20 +08:00`：G044 正式独立 Verdict 为 `VERIFIED / APPROVED / CHECKPOINT READY`，P0=0、P1=0、P2=0，无剩余阻断；确认独立 fresh 精确十门 checkpoint 全部 PASS。G044 可创建 Phase 3 rollback checkpoint；TASK-20260829-002 总体状态保持进行中。
- `2026-08-30 04:52:48 +08:00`：G045 / Phase 4 新增唯一 `intercept-proxy-package-contract` crate，唯一内部依赖为 Domain；权威拥有 final API1 Manifest、无 id 单向 `package.register`、八个固定 Hook/Display request、严格 result/error envelope、closed FrameResult、canonical Base64 与 Domain stable-code wire。复用 Domain Document/Schema/Package identity/ErrorCode，未切换 WebSocket/Sidecar 生命周期或旧 protocol-scripting import。
- `2026-08-30 04:52:48 +08:00`：TDD RED 为五个 Cargo 自动发现 targets 因合同类型不存在触发 `E0432`、exit 101；GREEN 为 Rust 10/10、fail-closed checker 6/6、TS unknown-boundary guard 4/4、MCP 独立 snapshot mutation、bindings deterministic、typecheck/lint/architecture/source-size/fmt/target strict Clippy/diff PASS。删除两个从未被 Cargo 挂载且锁定旧动态 hook 合同的 domain coverage 文件；旧运行 actor wire 仅保留 Phase7 精确 allowlist。
- `2026-08-30 04:52:48 +08:00`：首次完整十门 checkpoint 前九门与前端 63 files/538 tests PASS，workspace strict Clippy PASS；最后 workspace tests 在既有 MCP non-loopback HTTP 环境测试中 132/133 后 10 秒超时，定向复测同样超时。该失败与 Phase4 crate 无依赖/调用关系，但未把全仓 checkpoint 记为 PASS。正式证据：[phase4-package-contract](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase4-package-contract/README.md)。当前为 `LOCAL CONTRACT GREEN / GLOBAL CHECKPOINT ENVIRONMENT BLOCKED / RECHECK PENDING`，G045 不可 checkpoint。
- `2026-08-30 05:23:47 +08:00`：G045 独立 review 判定 `REQUEST CHANGES`（P0=0/P1=3/P2=1）：checker 可被注释测试名、替代命名 wire owner、generated/MCP 局部漂移及宽泛 legacy allowlist 绕过；golden/TS 未覆盖完整响应与 Domain identity/Schema invariant；`consumedBytes>0` 仍可由 Rust 直接构造，且 evidence 资源不是活动 fixture 的逐字节副本。所有 finding 均先增加 mutation/合同 RED 再修复。
- `2026-08-30 05:23:47 +08:00`：修复后 checker 以 Cargo `--list` 实际发现 5 targets/12 tests，结构扫描任意 Manifest-shaped owner，generated 与 MCP 全文件 SHA，stale generated type，精确 file+symbol Phase7 allowlist及 evidence SHA/byte copy 全部 fail-closed，Node 10/10。单一 golden 覆盖 Manifest/register/8 requests/所有 success/error/FrameResult 变体并由 Rust 全量 round-trip、TS 全量消费；TS 校验参数由 Rust 生成。`ConsumedBytes(NonZeroUsize)` 私有构造令 0 不可表达，合同新增 adapter-context buffer length 校验但不切换 Phase7 runtime。
- `2026-08-30 05:23:47 +08:00`：fresh Phase4 Rust 12/12、TS 6/6、Domain 108/108、bindings deterministic、前九门复验（前端 63 files/540 tests、workspace strict Clippy）与 diff PASS。按已知 macOS ALF 外部阻断不重复执行第十个 non-loopback workspace test；状态为 `REVIEW FINDINGS FIXED / LOCAL GREEN / GLOBAL CHECKPOINT ENVIRONMENT BLOCKED / RECHECK PENDING`，等待独立复审。
- `2026-08-30 05:29:54 +08:00`：最终差异自审进一步关闭 TS stable-code 边界：Domain 新增完整 `ErrorCode::ALL`，与 identity/Schema 参数一并由 Rust bindings 生成；TS error guard 只消费该集合并新增未知 code 拒绝。fresh bindings、TS 6/6、Rust 12/12、target strict Clippy、checker 10/10、typecheck 与 diff PASS；generated SHA-256 为 `a0b1b96d65d50c6fb5f9de792807905d36134baa257bf4336a9dbef4699344ed`。
- `2026-08-30 05:44:03 +08:00`：最终 delta review 仍为 P1：原 MCP snapshot 仅摘要，若同步修改 snapshot/evidence/hash 可夹带 `legacy_retry`；结构 owner 扫描可由 `#[serde(rename="package")] metadata` 绕过。精确 mutation 先 RED 后修复：MCP fixture 现包含完整 executable schema 与 canonical golden，覆盖 Manifest/registration/八个 params 和 method result/success/failure/FrameResult/stable-code enum，checker 独立核对全语义，coherent hash mutation 仍失败；owner 扫描按 serialize/deserialize wire field set 识别 `rename`、`alias`、deserialize-only variants。fresh checker 14/14、Rust 12/12、TS 6/6、bindings deterministic、target strict Clippy、fmt/typecheck/diff PASS；runtime、ALF blocker 与 `RECHECK PENDING` 不变。
- `2026-08-30 05:55:48 +08:00`：同轮 final review 的 P2 已显式关闭。TS decode success 改用真实 `isPackageDocument` 并回归 unsafe integer/非法递归值。共享 ID/SemVer corpus 由 Domain constructors 与 generated metadata 双向执行；RED 发现 core number `18446744073709551616` 被正则接受但被 Rust `semver` 拒绝，现 Rust 生成 `u64` core numeric max，TS/MCP 共同执行。`ErrorCode` 改为单一宏表生成 enum、serde wire、`ALL` 与 `as_str`，行为不变。fresh checker 15/15、Domain 108/108、Rust contract 12/12、TS 6/6、bindings deterministic、lint/typecheck/source-size/fmt、Domain+contract strict Clippy 与 diff PASS；状态仍 `RECHECK PENDING / ALF blocked`。
- `2026-08-30 06:15:08 +08:00`：最终复审 P1 先以精确 mutation 复现私有 serde struct、`#[serde(untagged)]` enum struct variant 与 `serde(flatten)` owner 绕过；checker 现不依赖 `pub`/类型名，解析 struct/variant wire shape，并对合同 owner 或 Phase7 精确 file+symbol allowlist 外的 flatten fail-closed。任务权威 Manifest 示例 `com.example.payment` 不改；唯一 Domain `ProtocolPackageId` 最小扩展点分段，保留既有 ID，拒绝前导/尾随/重复点，并由 Domain、contract、TS 与 MCP 共用 pattern/corpus。fresh checker 17/17、Domain 108/108、Rust contract 13/13、TS 7/7、MCP/golden、bindings deterministic、typecheck/fmt/Domain+contract strict Clippy/diff PASS；状态仍 `RECHECK_PENDING / ALF blocked`，未切换 runtime、未跑 full checkpoint。
- `2026-08-30 06:30:07 +08:00`：checker 正控继续发现过宽误报：无关 filters flatten、注释/字符串 Phantom Manifest、非 Serde 内部同字段 struct 均不应成为 wire owner。精确 RED 后，扫描先词法屏蔽 Rust comments/string literals，只接受 derive/manual impl 证明的 Serialize/Deserialize eligibility，按方向解析 rename/alias/skip，并以 visited set 递归合并本地 flattened type 的有效字段；仅真实形成 `api/kind/package/document` 才拒绝。非 Serde `ProtocolManifest` 的旧 Phase7 Manifest allowlist 被判 stale 并删除，不改变旧 TOML runtime。fresh checker 18/18、Phase4 Rust 13/13、TS 7/7、lint/source-size/typecheck/fmt/diff PASS；状态仍 `RECHECK_PENDING / ALF blocked`。
- `2026-08-30 06:39:29 +08:00`：最终 manual Serde P1 以三个 RED/正控关闭：声明 `metadata/payload` 但 custom Serialize 发出四个 Manifest key、custom Deserialize/Visitor 接受四个 key 均须拒绝；声明字段看似 Manifest 但 manual Serialize 只输出 harmless string 必须通过。checker 使用去字符串结构视图定位 impl，并从保留字面量的同索引视图提取 `serialize_field/entry`、Deserialize fields const 与关联 Visitor match keys；manual owner 只按实际 wire key 集合判定，同时修复 lifetime `'de` 与 char literal 词法区分。fresh checker 21/21、Phase4 Rust 13/13、TS 7/7、lint/source-size/typecheck/fmt/diff PASS；状态保持 `RECHECK_PENDING / ALF blocked`。
- `2026-08-30 08:49:12 +08:00`：G045 最终独立 Verifier 结论为 `VERIFIED / APPROVED / CHECKPOINT READY`，P0=0、P1=0、P2=0。历史 132/133 ALF timeout 保留；用户授权 exact b171 test binary（SHA-256 `c7dc870daca6f4f86eeebe29270ef65d4f61eab70b943b55cd994527544143aa`）并允许 firewall 后，短函数名 `--exact` 首次发现 0 tests，明确为 `NOT EVIDENCE`；完整模块名、`--all-features`、`--test-threads=1` 定向 1/1 PASS。最终独立十门 checkpoint exit 0，前端 63 files/541 tests、顶层 Rust 133/133、workspace all-target/all-feature exit 0；Phase4 21/21+13/13+7/7、Domain 108/108、generated SHA `897edb991e8bd7efc6d114ca4eb1c6b67eb162574e0bb764ebed7a93e39c3c9e`、七组 evidence byte copies 与 diff-check PASS。G045 可创建 rollback checkpoint；TASK 总体仍为进行中。
- `2026-08-30 11:04:03 +08:00`：G046 / Phase 5 以真实 Cargo-discovered RED 开始，随后建立唯一 `ConditionTree` / `DocumentPredicate` / `UnifiedAction` / `UnifiedRuleProgram` owner。权威 `RuleContent` 切换为非空递归 AND/OR tree 与单一有序 actions；Document 严格 mutation 留在 Domain，HTTP 组合不污染 Document。新保存消息规则只允许 `ProxyToUpstream` / `ProxyToApp`，旧 enum/runtime 仅按 Phase 12 restore 边界保留，不增加兼容 alias 或双模型。
- `2026-08-30 11:04:03 +08:00`：Phase5 checker 真实发现 Rust7/TS4，mutation/正控 8/8；Domain 115/115、Application 497/497、Infrastructure 690/690、Host 33/33、Tauri 133/133、bindings deterministic、typecheck/lint/architecture/source-size/fmt/workspace strict Clippy 与 `git diff --check` 全部 fresh PASS。完整十门 checkpoint exit 0，前端 63 files/542 tests、Rust workspace all-target/all-feature 0 failed。正式证据：[phase5-unified-rule-domain](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase5-unified-rule-domain/README.md)。当前仅为 `LOCAL GREEN / RECHECK PENDING`，`checkpoint_ready=false`；未提前实现 Phase6 transaction/lifecycle commit、Phase10/11 pipeline/codec、Phase12 删除旧 runtime/enum或 Phase15 完整 editor。
- `2026-08-30 11:57:30 +08:00`：G046 初始独立 Verifier 为 `FAILED`（P0=0/P1=3/P2=1），独立 review 为 `REQUEST CHANGES`（P1=6/P2=1）。修复 RED/变异覆盖 JS Number `-0 == +0`、Socket 无 terminal capability、HTTP 无 Document binding、删除未确认 64/1024/64 规则内硬上限、`priority + rule_id` 唯一排序，以及实际 Cargo/Vitest discovery、全 production comparator helper、真实 serde/Specta owner、generated 完整 golden/SHA 与精确 Phase12 allowlist。fresh checker15/15、Cargo9/TS4、Domain119/119、Application497/497、Infrastructure690/690、Host33/33、Tauri133/133，bindings/typecheck/lint/architecture/source-size/fmt/strict Clippy/diff 均 PASS。
- `2026-08-30 11:57:30 +08:00`：首次十门在第7门出现既有 `protocol-package-dialog` 焦点时序失败（541/542），后3门未运行；完整名定向 3/3 PASS，未改产品/断言/超时/重试。第二次完整十门 exit 0，前端542/542、Rust workspace 0 failed。Phase5 HTTP action仍仅收集，Phase6 transaction保持 `NOT_RUN`。当前为 `REPAIR LOCAL GREEN / RECHECK PENDING`，`checkpoint_ready=false`。
- `2026-08-30 12:16:01 +08:00`：G046 最终独立 reviewer 结论为 `APPROVE`，最终独立 verifier 结论为 `VERIFIED / APPROVED / CHECKPOINT READY`；P0=0、P1=0、P2=0，`blockers=[]`。初版 `FAILED` / `REQUEST CHANGES`、全部 findings/repairs、首次焦点 flake 与后续 PASS、完整十门 exit 0 均保留。Phase6/10/11/12/15、push/CI/Release 继续 `NOT_RUN`；TASK 总体仍为进行中。
- `2026-08-30 13:45:11 +08:00`：G047 / Phase 6 将 `one_shot/hit_count/last_hit_at` 提升为 RuleDefinition 顶层唯一 lifecycle，HTTP 与 Socket 共用；`NthHit` 从 HTTP `MatchCondition` 提升为 common `Condition` leaf，Socket 不获得 HTTP 能力。Application 新增唯一 `RuleChainTransaction`，私有持有 working HTTP message、Document、trace、pending control 和 lifecycle deltas；前序 typed HTTP/Document mutation 对后序条件可见，整链只 encode 一次、delta commit 一次，成功提交前不发布任何输出或 terminal control。
- `2026-08-30 13:45:11 +08:00`：Infrastructure repository port 收窄为 lifecycle delta CAS commit，actor 删除旧 `(0..=3)` conflict retry；冲突 commit attempts=1、joint 原消息不变、NthHit/one-shot 不消费。caller 在 actor-owned commit 开始后 abort 仍由 actor 完成一次状态机，不取消、不重放。旧 Phase12 enum/runtime 保留但不再拥有 NthHit；Phase10/11 完整 pipeline/codec 未提前切换。
- `2026-08-30 13:45:11 +08:00`：真实 TDD RED 覆盖 Domain/Application owner 缺失、旧 retry 方向与 checker 缺失。Application 首轮全量真实发现 common NthHit `count=0` 漏校验（457 PASS / 2 FAIL），Domain 唯一 owner 修复后 exact 2/2 与全量 503/503 PASS。source-size 首轮发现 `unified_rule.rs` 580 行，按职责拆分为 492 行 + lifecycle module 98 行且不放宽门禁；strict Clippy findings 修复后 fresh PASS。
- `2026-08-30 13:45:11 +08:00`：Phase6 checker mutation/正控 19/19、Cargo 实际发现 Domain4/Application6/Infrastructure3；Domain123/123、Application503/503、Infrastructure690/690、Host33/33、Tauri133/133、bindings deterministic、TS focused21/21、architecture/source-size/lint/typecheck/fmt/workspace strict Clippy/diff 全部 fresh PASS。完整十门单进程 checkpoint exit0，前端63 files/543 tests、Rust workspace 0 failed/0 ignored。正式证据：[phase6-rule-chain-transaction](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase6-rule-chain-transaction/README.md)。当前为 `LOCAL GREEN / RECHECK PENDING`、`checkpoint_ready=false`；Phase7+、push/CI/Release 为 `NOT_RUN`。
- `2026-08-30 15:05:13 +08:00`：G047 初版独立 Reviewer 结论为 `REQUEST CHANGES`（P1=4、P2=1），独立 Verifier 结论为 `FAILED`（P1=2），撤回初版完成判断。反例覆盖：NthHit 丢失既有 `(rule_id, terminal IP, certificate fingerprint)` identity 且误用 `hit_count`；公开 program/lifecycle tuple 可错配或重复；普通 save wire 混入 server-owned runtime stats；HTTP condition 降级丢失完整 `AppError`；repository delta subtraction 与 duplicate/zero/oversized/decrease/wrong-id 校验不够 fail-closed。每项先加入永久 RED/变异后修复，初版 finding 保留且未改写为成功。
- `2026-08-30 15:05:13 +08:00`：Repair 建立 transaction-private terminal-scoped Nth snapshot/delta；成功 no-match 原子推进 Nth，任意 condition/action/encode/commit-validation/cancel/revision conflict 均不消费。私有 validated plan 在任何 port 前拒绝 rule/lifecycle mismatch、terminal mismatch 与重复 ID；repository 对全部 delta 先完整校验后单次 commit。HTTP condition 原样传播全部 `AppError` 字段。普通 create/update save 不接受 runtime stats；create 强制初始 stats，update 保留 server stats；copy 创建新 rule identity、revision=`INITIAL`、stats reset，只复制已确认的可编辑 metadata、conditions/actions 与 `one_shot`。
- `2026-08-30 15:05:13 +08:00`：Repair checker mutation/正控 26/26，Cargo 实际发现 Domain8/Application11/Infrastructure6；Domain127/127、Application508/508、Infrastructure693/693、Host33/33、Tauri133/133、bindings deterministic、TS focused21/21、architecture/source-size/lint/typecheck/fmt/workspace strict Clippy/diff 全部 fresh PASS。最终完整十门仍为单一 PTY/session、exit0，前端63 files/543 tests、Rust workspace 0 failed/0 ignored。状态为 `REPAIR GREEN / RECHECK PENDING`、`checkpoint_ready=false`；Phase7/10/11/12/15、push/CI/Release 均 `NOT_RUN`。
- `2026-08-30 15:38:58 +08:00`：第二轮独立复审再次 `REQUEST CHANGES`（P1=2）：crafted Nth-only delta 可绕过 adapter 禁用 one-shot；actor 在 Nth/runtime delta 校验失败前未恢复 checkpoint，可能消费内存状态。两项均先取得公开 adapter/actor 永久 RED，再以 Domain `RuleLifecycleDelta::validate_against` 统一校验 owner，并让 prepare、Nth validation、runtime validation、commit 任一失败均恢复 actor engine checkpoint、且不发布 message/control/trace/lifecycle。新增 checker mutations 后 28/28，Cargo 发现 Domain9/Application11/Infrastructure8；Domain128/128、Application508/508、Infrastructure695/695、Host33/33、Tauri133/133 与全部静态门 fresh PASS；单一 PTY 十门 exit0、前端63 files/543 tests、Rust workspace 0 failed/0 ignored。状态继续 `REPAIR GREEN / RECHECK PENDING`、`checkpoint_ready=false`。
- `2026-08-30 15:52:01 +08:00`：G047 最终 Reviewer 结论为 `APPROVE`，Verifier 结论为 `VERIFIED / APPROVED / CHECKPOINT READY`；P0=0、P1=0、P2=0、`blockers=[]`。最终计数为 checker28/28、Domain9/Application11/Infrastructure8 focused、Domain128/Application508/Infrastructure695/Host33/Tauri133 affected full、前端543/543、workspace 0 failed/0 ignored；初版 findings 与第二轮新增2个P1均已关闭。TASK 总体仍为进行中，Phase7/10/11/12/15、push/CI/Release 保持 `NOT_RUN`。
- `2026-08-30 17:24:55 +08:00`：G048 / Phase 7 新增严格根 ZIP reader 并直接消费 Phase4 唯一 `PackageManifest`；活动 `/packages` 服务、registry、Socket binding 与 relay 全部切换到唯一 `PackageTransportClient`。package 主动发起无 id `package.register` 且 Proxy 不回复；八个固定 typed methods、stable Domain code、canonical Base64、FrameResult buffer 校验落地。保留 registration deadline、heartbeat、wire-size、shutdown/backpressure；删除活动 public TOML/Rhai/dynamic DTO、旧 dynamic actor 及 Hook timeout/max-in-flight/Busy/retry/replay。唯一 legacy allowlist 精确为 `protocol-scripting/src/lib.rs#parse_protocol_manifest`、owner Phase13。
- `2026-08-30 17:24:55 +08:00`：真实 RED 为 archive reader `E0432`、checker 13 failures、旧 transport integration 编译失败；GREEN 为 checker mutation/正控18/18、Cargo discovered ZIP4/transport4、active E2E4/4、Infrastructure full 590+4+24+7+8 全通过。source-size 首次拒绝 transport 561 行，拆为公共 client + 私有 driver 后在不放宽500行门禁下通过。十门前九项最终 PASS（前端63 files/543 tests、strict Clippy PASS），最后既有 MCP non-loopback HTTP 用例 132/133 后10秒超时，独立精确复测仍超时；不修改其超时/重试/网络合同，不把全仓 checkpoint 记为 PASS。正式证据：[phase7-package-runtime](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase7-package-runtime/README.md)。当前为 `LOCAL GREEN / GLOBAL CHECKPOINT ENVIRONMENT BLOCKED / RECHECK PENDING`、`checkpoint_ready=false`。
- `2026-08-30 19:43:37 +08:00`：G048 review repair 已关闭全部 finding：活动 importer 真实消费 strict ZIP/shared Manifest 并在 Phase8 编译边界 fail-closed；复用 archive/entry/file/total/ratio/path-depth 产品限制；stable Domain code 贯穿 registry/relay/Application/Tauri/capture；预注册取消 owned/joined；删除永久 completed-ID 集合；raw logical frame 与 encoded wire budget 分离。checker 扩至20/20，Cargo发现 ZIP5/transport7，active E2E4/4；Phase6 精确 SHA 临时干净 worktree 重放 Cargo exit101、checker exit1 的真实 RED 原文与 SHA 已归档。
- `2026-08-30 19:43:37 +08:00`：旧 Tauri/E2E/Host Rhai 成功夹具迁为严格 JS ZIP + Phase8 fail-closed，保留真实 IPC/Application 覆盖。fresh affected full 为 Tauri130/130、Application458/458、Host12/12、Infrastructure583/583；bindings deterministic、architecture/source-size/lint/typecheck/fmt/strict Clippy 全部 PASS。第三轮单一 PTY 完整十门 exit0，前端63 files/543 tests、workspace all-target/all-feature 0 failed；此前 non-loopback 环境失败本轮已通过并撤销 blocker。状态为 `REPAIR LOCAL GREEN / RECHECK PENDING`、`checkpoint_ready=false`，等待独立复审；Phase8+ 不提前实施。
- `2026-08-30 20:39:34 +08:00`：第二轮 review repair 以 forged ZIP declared/actual mismatch 真实 RED 开始；修复 bounded actual-byte accounting、stable Domain code 经 Exchange/SocketProcessingFailure/terminal observer 到 `external_package_call.stable_code` 的活动链、production WebSocket 三类 wire ceiling 与 `/packages` RPC B/B+1，并修正 import Phase8/Rhai 边界注释。checker23/23、ZIP6/6、transport7/7、active E2E4/4、WS1/1、diagnostic1/1；affected full、bindings、architecture/source-size/lint/typecheck/fmt/workspace strict Clippy、diff 与唯一完整十门均 fresh PASS，Infrastructure585/585、前端63 files/543 tests、non-loopback通过。状态继续 `REPAIR LOCAL GREEN / RECHECK PENDING`、`checkpoint_ready=false`，等待独立复审；Phase8+、提交与推送未执行。
- `2026-08-30 21:09:06 +08:00`：最终 cross-phase repair 删除活动 Phase4 inventory 中 18 条已完成迁移的 `phase7_legacy_wire_allowlist` 并以 checker 永久约束为空；活动 generated SHA 按 fresh deterministic bindings 复算为 `413e42788f02a616b18141bf9e7bbcc5217f775fc636b53a3f2d4bdd3b144123`，Phase4 历史 evidence snapshot 未改。新增精确旧 wire reallow 与 stale SHA mutation，Phase4 23/23；Phase7 聚合真实纳入 Phase4 contract checker并 fresh 全部通过。bindings/static gates 与唯一完整十门 session51772 exit0，前端63 files/543 tests、workspace all-target/all-feature 0 failed/0 ignored。状态继续 `REPAIR LOCAL GREEN / RECHECK PENDING`、`checkpoint_ready=false`，等待独立复审；未提交、未推送。
- `2026-08-30 21:21:22 +08:00`：G048 最终独立 Reviewer 结论为 `APPROVE`，Verifier 结论为 `VERIFIED / APPROVED / CHECKPOINT READY`；P0=0、P1=0、P2=0、`blockers=[]`、`checkpoint_ready=true`。全部历史 findings、repairs、RED/GREEN 与 `NOT_RUN` 边界保留，Phase 7 可创建 rollback checkpoint；TASK 总体仍为进行中，Phase8+、提交、推送、CI 与 Release 未执行。
- `2026-08-30 22:22:53 +08:00`：G049 / Phase 8 在精确 HEAD `03144593ca929379fdb848516c35fcd92743106c` detached worktree 真实取得缺少 Sidecar binary/`LocalSidecarRuntime` 的 Cargo exit101 RED；ordinary Array encode 另取得 0/1 RED。实现单 Boa Context 串行、package-relative `.js` ESM 一次 parse/load/link/evaluate、固定八 callable export 预检缓存、HTTP string 与 Socket 严格 `Uint8Array`/canonical Base64；不注入 Host API，不提供 Rhai 回退。generic Sidecar executable 仅为 compile marker，未实现 Phase9 参数/启动/注册/lifecycle/timeout/queue/retry/recovery，未改 Tauri `externalBin`。
- `2026-08-30 22:22:53 +08:00`：Phase8 checker mutation/正控18/18、Cargo实际发现10/10，Phase7 ZIP6/6、package-contract13/13、protocol-scripting160/160、Infrastructure585/585及相关 suites fresh PASS；bindings deterministic、architecture/source-size/lint/typecheck/fmt/package-runtime strict Clippy/diff PASS。唯一完整十门 PID18568 已结束且全程未观察到失败，但原调用通道未保存最终 shell exit code；证据按 `LOCAL GREEN / RECHECK PENDING`、`checkpoint_ready=false` 收口，未把观察结果升级为 checkpoint PASS。正式证据：[phase8-boa-sidecar-runtime](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase8-boa-sidecar-runtime/README.md)。
- `2026-08-30 22:49:48 +08:00`：用户 22:35 明确覆盖旧安全限制后，删除 Host API absence/default-features-disabled checker 与测试，不添加 dynamic-import reject 或错误脱敏。真实 RED 证明 async dynamic import Hook Promise 曾错误成为空 Document；修复为持续驱动 Boa jobs 直到 fulfilled/rejected，Pending 不设 timeout/Busy，永久 pending 占用单 Context。Boa default features 实际启用；合法 lazy import 两次调用只求值一次，nested `../`、static cycle once、越根拒绝均通过。
- `2026-08-30 22:49:48 +08:00`：repair checker18/18、Phase8 runtime9/9+review3/3、Phase7 ZIP6/6、fmt/diff/package-runtime strict Clippy 与全部静态门 fresh PASS。唯一最终十门 session47419 终态 exit0，前端63 files/543 tests、workspace all-target/all-feature零失败；旧 PID18568/26450 缺 exit 的观察不再作为当前验收。证据保持 `LOCAL GREEN / RECHECK PENDING`、`checkpoint_ready=false`，等待独立复验。
- `2026-08-30 22:57:57 +08:00`：最终 P2 checker 先以 `register_global_*`、`NativeFunction`、custom `HostHooks` 三类 mutation 取得18/21 RED；加入去注释/字符串后的精确结构 token 扫描，禁止 Proxy 侧注入非 Boa Host binding，不恢复运行时 Host absence 测试、不扫描或限制 Boa 自身 globals/default features。fresh checker21/21、Phase8 aggregate（ZIP6+runtime9+review3）、lint、diff PASS；按要求未重跑 workspace/十门，session47419 exit0 仍为最近唯一全量。状态保持 `LOCAL GREEN / RECHECK PENDING`、`checkpoint_ready=false`。
- `2026-08-30 23:03:28 +08:00`：G049 最终独立 Reviewer 结论为 `APPROVE`，Verifier 结论为 `VERIFIED / APPROVED / CHECKPOINT READY`；P0=0、P1=0、P2=0、`blockers=[]`、`checkpoint_ready=true`。全部需求变更、历史 findings、RED/GREEN 与 `NOT_RUN` 边界保留，Phase 8 可创建 rollback checkpoint；TASK 总体仍为进行中。
- `2026-08-30 23:53:04 +08:00`：G050 / Phase 9 将 Phase8 Boa runtime 接入真实本地 Sidecar process；私有 launch spec 只有 ZIP path 与统一 `/packages` URL，package 主动注册并处理固定 typed RPC。strict ZIP commit 原子持久化 local archive、enabled 后启动；app-start 只后台启动 enabled exact versions。supervisor 对 exact identity 单一所有，manual restart/disable/disconnect/delete/shutdown 均 kill+wait，最长10秒注册，失败保留 enabled、记录 failed 且无retry/replay/recovery。Listener 保持 enabled+online gate，duplicate exact identity 不接管。
- `2026-08-30 23:53:04 +08:00`：真实 process RED 为 Tokio process feature 缺失 exit101 与 typed dispatch E0308；persistence RED 为 local install outcome 缺失 exit101。GREEN 后 checker canonical+9 mutations 10/10、package-runtime20/20、supervisor2/2、Infrastructure588/588、Application458/458；bindings deterministic、typecheck、architecture、source-size、fmt、affected strict Clippy、diff均PASS。source-size曾拒绝registry596行，拆出local archive职责后不放宽500行门禁并PASS。正式证据：[phase9-local-sidecar-lifecycle](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase9-local-sidecar-lifecycle/README.md)。当前为 `LOCAL GREEN / RECHECK PENDING`、`checkpoint_ready=false`。
- `2026-08-30 23:59:07 +08:00`：最终diff审查修正首次合法注册持久化`enabled=1`但接纳结果仍返回false的不一致；测试显式disable后证明reconnect保留false，delete后新首次注册恢复true。完整名exact 1/1、external affected76/76、strict Clippy与Phase9 checker10/10均PASS；更早短名`--exact`发现0项不作为成功证据。证据哈希与计数已单次同步，Phase9冻结为`LOCAL GREEN / RECHECK PENDING`。
- `2026-08-31 00:49:18 +08:00`：G050 review repair 按 RED→GREEN 关闭 importer Boa ownership、local-vs-remote enable/manual restart、exact identity 串行 gate、Supervisor 唯一 process error owner 与两条 stale IPC 断言。fresh checker14/14、importer3/3、supervisor4/4、Application lifecycle8/8、两条完整名 IPC 各1/1、package-runtime20/20、Infrastructure591/591、Application460/460、协议包前端84/84及全部静态门 PASS。唯一完整 checkpoint 前九门与前端64 files/545 tests PASS，最终 workspace 在 Tauri129/130 后仅既有 non-loopback MCP HTTP 10秒 exchange deadline 超时；同一项 affected full 已复现，按用户指示标记 `GLOBAL CHECKPOINT ENVIRONMENT BLOCKED / RECHECK PENDING`，不修改 timeout/retry、不重跑 checkpoint，等待独立 review/verifier。
- `2026-08-31 01:04:27 +08:00`：最终 review 新 P1 以 Application、UI、真实 Tauri IPC 三层 RED 证明 disabled local package 仍可进入 restart port/UI；最小修复在 Application mutation gate 内先拒绝 `enabled=false` 并返回 `PROTOCOL_PACKAGE_DISABLED`，UI 只对 enabled local process 展示 restart，dialog 同步防御。fresh 完整名 Application1/1、Tauri IPC1/1、UI2/2、checker15/15、Application460/460、Tauri protocol-package3/3、前端84/84、typecheck/lint/source-size/fmt/affected strict Clippy/diff 均 PASS；按要求未重跑 workspace/checkpoint，nonloopback 历史阻塞不变，状态继续 `RECHECK PENDING`。
- `2026-08-31 01:11:14 +08:00`：G050 最终独立 Reviewer 结论为 `APPROVE`，Verifier 结论为 `VERIFIED / APPROVED / CHECKPOINT READY`；P0=0、P1=0、P2=0，`checkpoint_ready=true`。全部 review findings 已闭合；唯一 workspace gate 的既有 nonloopback MCP HTTP 环境超时继续作为历史阻塞保留，真实 macOS bundle/permissions 保持 `NOT_RUN`，不影响该独立代码与合同 verdict。Phase 9 可创建 rollback checkpoint；TASK 总体继续进行中。
- `2026-08-31 02:03:15 +08:00`：G051 / Phase 10 将 HTTP Body 包切换到与 Socket 相同的 package 主动 `/packages` typed RPC。Listener start 冻结统一 registry 的 exact HTTP binding；strict UTF-8/Shift-JIS codec 拒绝未知/非法/不可表示 charset 与 non-identity Content-Encoding。Exchange 保留 authoritative wire bytes；Document unchanged 时0 Encode RPC并原字节转发，changed 时调用 Encode 后在同一 joint transaction 提交。Display 保持 observation-only，其他阶段失败 Exchange；production legacy HTTP executor 全部限制为 `cfg(test)`，未增加timeout/queue/Busy/retry/replay/recovery。
- `2026-08-31 02:03:15 +08:00`：Phase10 checker canonical+12 mutations 13/13、focused4/4、legacy regression12/12、Exchange23/23、Runtime225/225、Infrastructure641/641与全部静态门PASS。唯一完整 checkpoint session3469 的Phase1/bindings/architecture/source-size/lint/typecheck/前端64 files/545 tests/fmt/workspace strict Clippy均PASS；workspace tests在Tauri129/130后仅既有non-loopback MCP HTTP 10秒deadline超时，后续targets因`&&`停止为`NOT_RUN`。按用户指示不改timeout/retry、不重跑完整checkpoint，状态为`LOCAL GREEN / GLOBAL CHECKPOINT ENVIRONMENT BLOCKED / RECHECK_PENDING`、`checkpoint_ready=false`。正式证据：[phase10-http-shared-rpc-pipeline](../../../testing/evidence/2026-08-31/TASK-20260829-002/phase10-http-shared-rpc-pipeline/README.md)。
- `2026-08-31 02:40:42 +08:00`：Phase10 review repair 将 remote JSON-RPC 的 package/direction/stage/method/request_id/data.code/numeric code/message/data shape 从 HTTP Decode/Display/Encode 唯一错误 owner 贯穿 Exchange、Proxy/ChildTask 与 capture typed event，joint actor 在错误时恢复 checkpoint 且不提交 message/lifecycle。production-shape 测试经 fake shared provider→`prepare_async`→双向 capability/joint runtime，覆盖 unchanged 0 Encode；changed与typed failure rollback由focused覆盖。checker新增actor与`legacy_http` cfg(test) containment mutations，15/15；focused6/6、Application460/460、Exchange23/23、Runtime226/226、Infrastructure643/643、bindings deterministic、typecheck/fmt/affected strict Clippy/diff均fresh PASS。按要求未重跑workspace；session3469既有环境阻塞继续保留，状态仍`LOCAL GREEN / GLOBAL CHECKPOINT ENVIRONMENT BLOCKED / RECHECK_PENDING`、`checkpoint_ready=false`。
- `2026-08-31 03:00:41 +08:00`：最终 review repair 以真实 RED 关闭两项 P1 及追加 findings：remote failure 顶层 Proxy code 固定为 `EXTERNAL_PACKAGE_CALL_FAILED` 且保留 package stable code，Display fail-open 将 typed failure 写入 capture observation，endpoint 转换保留 `external_package_call`；production-shape 真实经 provider→`prepare_async`→双向 capability→joint actor，changed Encode 成功仅提交一次，typed Encode 失败保持 message/lifecycle 并零提交。长测试拆分后 strict Clippy PASS。fresh checker19/19、focused6/6、Exchange24/24、Runtime227/227、Infrastructure643/643、Application460/460、fmt/affected strict Clippy/diff均PASS；按要求未重跑workspace，session3469环境阻塞和人工bundle/permissions `NOT_RUN` 保留，状态继续`RECHECK_PENDING`、`checkpoint_ready=false`。
- `2026-08-31 03:12:48 +08:00`：纯结构 repair 将 646 行 `phase10_http_pipeline.rs` 按 production-shape 职责拆为主测试386行与子模块264行，不加 allow、不改六条测试行为。结构 RED 先暴露 `#[path]` 父模块解析和 checker fixture 索引漂移，最小修正后 source-size、focused6/6、checker19/19、fmt、Infrastructure all-target/all-features strict Clippy及diff均fresh PASS；未重跑workspace，状态和既有阻塞不变。
- `2026-08-31 03:16:51 +08:00`：G051 / Phase 10 最终独立 Reviewer 结论为 `APPROVE`，Verifier 结论为 `VERIFIED / APPROVED / CHECKPOINT READY`；P0=0、P1=0、P2=0、`checkpoint_ready=true`。唯一 checkpoint session3469 的既有 non-loopback 环境阻塞、后续 workspace targets 与真实 macOS bundle/permissions `NOT_RUN` 继续作为执行事实保留，不影响最终代码与合同 verdict。Phase 10 可创建 rollback checkpoint；TASK 总体继续进行中。
- `2026-08-31 04:01:25 +08:00`：G052 / Phase 11 将外部 Socket Frame/Decode/Display/Encode 接入统一shared `/packages` RPC，Rules经协议中立joint evaluation进入Phase6 actor，在Encode成功后才提交生命周期；unchanged原字节且0 Encode RPC，changed调用Encode，typed failure回滚one-shot/hit count。consumedBytes zero/oversize仍由Exchange拒绝，Display仍observation-only，generated Socket adapter无HTTP能力；未新增timeout/queue/Busy/retry/replay/recovery，Phase12删除与Phase15 UI未提前实施。
- `2026-08-31 04:01:25 +08:00`：真实RED包括初始compile 2+8 errors，以及architecture发现HTTP子模块持有Socket职责和Proxy→Domain依赖；最小修复迁到`joint_document`并采用UUID/ProxyResult gate。fresh checker14/14、production3/3、actor lifecycle1/1、external affected8/8、Exchange Socket3/3、compile/bindings/architecture/source-size/fmt/affected strict Clippy/diff均PASS。唯一完整checkpoint session24690前九门及frontend64 files/545 tests全部PASS，workspace tests在Tauri129/130后仅既有non-loopback MCP HTTP 10秒超时，后续targets因`&&`为`NOT_RUN`；状态为`LOCAL GREEN / GLOBAL CHECKPOINT ENVIRONMENT BLOCKED / RECHECK_PENDING`、`checkpoint_ready=false`。正式证据：[phase11-socket-shared-rpc-pipeline](../../../testing/evidence/2026-08-31/TASK-20260829-002/phase11-socket-shared-rpc-pipeline/README.md)。
- `2026-08-31 04:51:23 +08:00`：Phase11 review repair 真实复现 production fixture `LISTENER_RUNTIME_NOT_READY`、Workspace actor epoch错配、无变化reset仍写库，以及旧LocalResponder stage断言。最小修复将desktop bundle与真实E2E统一到`ListenerRuntimePipelineAssembly`，Socket handler使用Workspace runtime epoch；RuleRepository snapshot/order/signature/commit/reset统一投影HTTP+Socket，零变化reset不持久化。真实SQLite+真实actor+两阶段Socket Program证明typed Encode失败仅调用一次upstream Encode、0 lifecycle commit且revision/state不变，随后成功只提交一次并将两个one-shot各命中一次。fresh external runtime5/5、LocalResponder exact1/1、checker canonical+19 mutations 20/20、bindings/architecture/source-size/fmt、domain+proxy+infrastructure strict Clippy及diff均PASS。Infrastructure full为600/602：本轮相关stale断言已修并精确通过，另一个未改Android ADB outer-deadline并发测试记录为环境/时序阻塞，未重跑full；按要求未重跑workspace checkpoint，session24690仍为唯一完整checkpoint。状态继续`LOCAL GREEN / GLOBAL CHECKPOINT ENVIRONMENT BLOCKED / RECHECK_PENDING`、`checkpoint_ready=false`，等待独立Reviewer/Verifier。
- `2026-08-31 05:12:41 +08:00`：第二轮review repair先以真实Relay RED证明Socket runtime projection丢失actor-owned NthHit；另一个review建议使用AppToProxy与权威合同冲突，TASK第58行与Phase5记录明确新消息规则只保留ProxyToUpstream/ProxyToApp，故未放开旧阶段。最小修复只从Socket ConditionTree投影NthHit给Phase6 actor，Document条件继续由joint Program gate。production E2E使用真实Relay：上行NthHit(2)首次miss提交advance，第二次匹配后Encode失败保持SQLite revision/lifecycle且不消费counter，重试仍匹配；上行修改`[a,b]→[x,b]`经真实upstream echo后，下行规则观察`x`再改`[x,b]→[x,y]`。暂停echo读取SQLite证明ProxyToUpstream只提交一次，最终ProxyToApp只追加一次提交。fresh exact1/1、Domain87/87、external runtime5/5、checker canonical+21 mutations22/22、architecture/source-size/fmt、Domain+Infrastructure strict Clippy与diff PASS；未重跑workspace，环境阻塞与人工NOT_RUN不变。状态继续`RECHECK_PENDING`、`checkpoint_ready=false`。
- `2026-08-31 05:23:13 +08:00`：G052 / Phase 11 最终独立 Reviewer 结论为 `APPROVE`，Verifier 结论为 `VERIFIED / APPROVED / CHECKPOINT READY`；P0=0、P1=0、P2=0，`checkpoint_ready=true`。唯一 checkpoint session24690 的既有 non-loopback MCP HTTP 环境超时、Infrastructure full 中未修改 Android ADB outer-deadline 环境时序阻塞，以及真实 macOS bundle/permissions `NOT_RUN` 均继续作为执行事实保留，不影响最终代码与合同 verdict。Phase 11 可创建 rollback checkpoint；TASK 总体继续进行中。
- `2026-08-31 06:22:09 +08:00`：G053 / Phase 12 删除AppToProxy/UpstreamToProxy及四阶段runtime factory、generated/UI合同；旧stage wire在Domain Serde边界fail-closed，无alias、迁移或fallback。HTTP field/operator直接由统一Condition owner持有，Phase5 legacy owner allowlist清空。真实RED从Cargo 1/2失败与checker 45残留收敛到checker canonical+9 mutations 10/10、Cargo2/2、affected/static全绿。唯一完整checkpoint session91671在第1门因活动Phase1 current-state inventory仍要求已删除fragments而失败，后9门因&&为NOT_RUN；修复活动inventory及mutation后Phase1 4/4、Phase12 aggregate和静态门全绿，按指令未重跑full。正式证据：[phase12-legacy-stage-deletion](../../../testing/evidence/2026-08-31/TASK-20260829-002/phase12-legacy-stage-deletion/README.md)。当前为`LOCAL GREEN / GLOBAL CHECKPOINT INCOMPLETE / RECHECK PENDING`、`checkpoint_ready=false`。
- `2026-08-31 06:33:43 +08:00`：Phase12 cross-phase repair fresh复现Phase5 checker Vitest expected4/actual5及generated golden仍引用RuleAction。仅更新活动Phase5 inventory/checker/mutations/golden和Phase12 aggregate：discovery改为真实5，allowlist必须空且legacy owner不可回加，golden精确锁定Condition direct HTTP与UnifiedAction HttpAction；Phase5历史evidence snapshot不改。fresh Phase5 15/15、Cargo discovery9/Vitest5，Phase12 combined25/25+Cargo2/2，Domain affected与全部integration、Phase1 4/4及静态门PASS；完整workspace/checkpoint未重跑，状态保持`RECHECK_PENDING`、`checkpoint_ready=false`。
- `2026-08-31 06:36:46 +08:00`：Reviewer发现Phase12 mutation的数组索引漂移使protocol/UI/generated用例未修改其命名目标文件。测试fixture改为具名path，三项mutation现分别真实修改protocol enum、UI model与generated bindings并fail-closed；同时修正`restore`仍接受legacy stage直到Phase12的过时注释。fresh combined25/25、Cargo2/2、fmt/source-size/diff PASS；evidence SHA同步，状态不变。
- `2026-08-31 06:43:40 +08:00`：G053 / Phase 12 最终 Reviewer `APPROVE`、Verifier `VERIFIED`，P0=P1=P2=0。`code_checkpoint_ready=true`；唯一完整checkpoint session91671仍保留gate1历史失败、focused修复PASS及gates2-10因`&&`为`NOT_RUN`，未重跑full，因此`global_checkpoint_complete=false`，不得写为全局PASS。Phase13+与人工/发布项继续`NOT_RUN`。
- 下一步：进入 Phase 13；真实macOS bundle/权限弹窗为人工 `NOT_RUN`。提交、推送、CI 与 Release 保持 `NOT_RUN`。

## 修改文件

- `docs/tasks/pending/2026-08-29/nested-document-rules-javascript-websocket-packages.md`：用最终确认合同替换过时的初始任务正文。
- `package.json`：增加 Phase 1 基线、generated bindings 与统一 checkpoint scripts。
- `scripts/check-task-20260829-002-phase-baseline.mjs`、`scripts/check-task-20260829-002-phase-baseline.test.mjs`：校验 current-state inventory、harness 路径、generated-type 旧合同片段及 checkpoint 命令，结构漂移 fail-closed。
- `scripts/check-generated-bindings.mjs`、`scripts/check-generated-bindings.test.mjs`：跨平台验证 generated bindings freshness/determinism；`finally` 无条件写回原字节，回归覆盖 generator 删除输出后抛错并保留原错误。
- `test-support/fixtures/task-20260829-002/phase-1/current-contract-inventory.json`：Phase 1 当前合同、后续 owning phase、测试入口与 checkpoint/rollback 规则。
- `docs/testing/evidence/2026-08-29/TASK-20260829-002/phase1-green-contract-baseline/`：Phase 1 inventory 快照、结构化命令结果、复测入口、ADB 偶发失败与独立复验记录。
- `src-tauri/crates/infrastructure/src/sqlite.rs`、`sqlite/core.rs`、`sqlite/executor.rs`、相关 tests 与 crate export：增加显式 `Preserve/RecreateCurrent` policy 和单事务 Schema100 重建原语，保留原 strict Preserve 合同。
- `src-tauri/crates/host/src/lib.rs`、`src-tauri/crates/host/src/tests/phase2_database_startup.rs`：Host 默认 Preserve，提供显式 policy 注入；加入可由 Phase 17 复用的公开 API 双启动 fixture 与失败传播测试。
- `src-tauri/src/lib.rs`：Tauri debug 显式 Recreate、Release 显式 Preserve；唯一临时 release blocker marker 紧贴 debug opt-in，不含 subscriber 安装前的无效日志。`src-tauri/src/app_state.rs` 无需修改。
- `scripts/check-task-20260829-002-phase2-release-blocker.mjs`、对应 Node tests、`package.json`、`src-tauri/tauri.conf.json`：增加 Phase 2 targeted gate 与独立 release-readiness scan；package alias 在 companion 前早期阻断，Tauri `beforeBuildCommand` 封闭通用/直接 build 入口，dev 路径不挂 gate；不改变 Phase 1 十门禁语义。
- `docs/testing/evidence/2026-08-30/TASK-20260829-002/phase2-development-database-recreate/`：Phase 2 合同、实际 ZIP 资源快照、结构化结果和复测命令。
- `docs/testing/evidence/2026-08-30/TASK-20260829-002/phase3-recursive-document-contract/`：Phase 3 recursive Document/Schema、RFC6901、即时消费者、旧合同零残留、派生 Nuvei ZIP 与完整 checkpoint 证据。
- `src-tauri/crates/package-contract/`、`src-tauri/src/commands/mod.rs`、`src/generated/rust-types.ts`：Phase 4 唯一 Rust package wire crate 与确定性 generated TS。
- `src/lib/package-contract.ts`、对应 tests、Phase 4 scripts/fixtures/MCP snapshot：unknown-boundary guard、五目标精确 checker 与独立 parity fixture。
- `docs/testing/evidence/2026-08-30/TASK-20260829-002/phase4-package-contract/`：Phase 4 RED/GREEN、合同资源、门禁结果与当前非 loopback MCP 环境阻断证据。
- `docs/testing/evidence/2026-08-30/TASK-20260829-002/phase5-unified-rule-domain/`：Phase 5 条件树、谓词、统一动作、排序/working-state/terminal、跨层消费者、mutation checker 与完整十门证据。
- `docs/testing/evidence/2026-08-30/TASK-20260829-002/phase6-rule-chain-transaction/`：Phase 6 共用 lifecycle/NthHit、唯一应用事务、delta no-retry、跨层原子性、checker mutation 与完整十门证据。
- `docs/testing/evidence/2026-08-30/TASK-20260829-002/phase7-package-runtime/`：Phase 7 严格 ZIP、package-initiated registration、固定 typed transport、canonical Base64/FrameResult、旧 dynamic path 删除、checker mutation、活动 E2E、Phase6 真实 RED、affected full 与完整十门 PASS 证据。
- `src-tauri/crates/package-runtime/src/sidecar.rs`、`src/bin/intercept-proxy-package-sidecar.rs`、Phase8 tests 与 Cargo manifests/lock：Phase 8 单 Boa Context、严格 ESM/exports/HTTP/Socket 转换及 compile-only generic Sidecar marker。
- `scripts/check-task-20260829-002-phase8-sidecar.mjs`、对应 mutation tests 与 `package.json`：Phase 8 fail-closed checker、真实 Cargo discovery 和 focused 入口。
- `docs/testing/evidence/2026-08-30/TASK-20260829-002/phase8-boa-sidecar-runtime/`：Phase 8 baseline compile、ordinary Array、dynamic Promise、Host-binding checker 四组真实 RED，focused/affected/static、唯一十门 exit0、SHA 与复测入口。
- `docs/testing/evidence/2026-08-30/TASK-20260829-002/phase9-local-sidecar-lifecycle/`：Phase 9 真实Sidecar process、strict ZIP持久化、enabled/online/failed生命周期、exact process ownership、checker mutations、affected full与静态门证据。
- `src-tauri/crates/exchange/src/protocol.rs`、`proxy/src/http/exchange_runtime/endpoints.rs`：Phase 10 在内部 HTTP Context 保留 authoritative wire bytes，文本只作严格 codec 后的投影。
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline{,/external_http.rs}`、`listener_runtime/joint_document.rs`：Phase 10 统一 HTTP shared RPC、strict codec、unchanged原字节与changed Encode/joint transaction；Phase 11 将joint evaluation提升到协议中立职责，旧 executor只保留`cfg(test)`回归。
- `scripts/check-task-20260829-002-phase10-http-pipeline.mjs`、对应 mutation tests、`package.json`：Phase 10 fail-closed checker、Cargo discovery和focused入口。
- `docs/testing/evidence/2026-08-31/TASK-20260829-002/phase10-http-shared-rpc-pipeline/`：Phase 10 RED/GREEN、checker19/19、focused/affected/static及唯一checkpoint session3469环境阻塞证据。
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/{joint_document.rs,external_relay/}`、`adapters/pipeline/`与Proxy Socket contracts：Phase 11协议中立joint Socket transaction、原字节/changed Encode、typed failure与actor lifecycle rollback/commit。
- `src-tauri/crates/domain/src/workspace/{runtime_projection.rs,unified_projection.rs}`、`infrastructure/adapters/{rules.rs,bundle.rs}`与Socket handler：Phase 11统一HTTP+Socket actor projection/persistence、Socket NthHit actor ownership、单一production pipeline装配及Workspace epoch identity。
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/external_package_runtime{.rs,/support.rs,/support/peer.rs}`：真实SQLite、真实production pipeline、两阶段Socket Program与typed Encode rollback/single-commit回归。
- `scripts/check-task-20260829-002-phase11-socket-pipeline.mjs`、对应mutation tests与`package.json`：Phase 11 fail-closed checker、Cargo discovery和focused入口。
- `docs/testing/evidence/2026-08-31/TASK-20260829-002/phase11-socket-shared-rpc-pipeline/`：Phase 11 RED/GREEN、checker14/14、focused/affected/static及唯一checkpoint session24690环境阻塞证据。
- `scripts/check-task-20260829-002-phase12-legacy-deletion.mjs`、对应mutation tests、Cargo wire test与跨层消费者：Phase 12旧阶段/owner/factory删除、旧wire fail-closed及活动inventory两阶段门禁。
- `docs/testing/evidence/2026-08-31/TASK-20260829-002/phase12-legacy-stage-deletion/`：Phase 12 RED/GREEN、affected/static、唯一checkpoint gate1失败、focused repair及后9门NOT_RUN证据。

## 附加文件

- 访谈上下文：`.omx/context/task-20260829-002-contract-finalization-20260829T130502Z.md`
- 访谈记录：`.omx/interviews/task-20260829-002-contract-finalization-20260829T130502Z.md`
- 规划输入规格：`.omx/specs/task-20260829-002-contract-finalization.md`
- 共识 PRD：`.omx/plans/prd-task-20260829-002.md`
- 测试规格：`.omx/plans/test-spec-task-20260829-002.md`
- Architect 审查：`.omx/plans/review-architect-task-20260829-002.md`（`APPROVE`）
- Critic 审查：`.omx/plans/review-critic-task-20260829-002.md`（`APPROVE`）
- Ralplan 交接记录：`.omx/state/ralplan-task-20260829-002-handoff.json`
- Ultragoal 权威计划：`.omx/ultragoal/goals.json`，本任务执行故事为 G042–G059。
- Ultragoal 审计账本：`.omx/ultragoal/ledger.jsonl`。
- 实现分支：`codex/task-20260829-002`。
- 当前 HTML 原型：`.omx/prototypes/rules-nested-document-prototype.html`，仅作为视觉草稿；冲突处以本任务为准。
- 相关基线：`docs/architecture/exchange-pipeline.md`、`docs/architecture/rules-and-protocol-packages.md`、`docs/architecture/decisions/ADR-002-protocol-packages-http.md`、`docs/architecture/decisions/ADR-007-exchange-pipeline-runtime-boundary.md`。
- Phase 1 活动 fixture：`test-support/fixtures/task-20260829-002/phase-1/current-contract-inventory.json`。
- Phase 1 正式证据：[phase1-green-contract-baseline](../../../testing/evidence/2026-08-29/TASK-20260829-002/phase1-green-contract-baseline/README.md)。
- Phase 2 正式证据：[phase2-development-database-recreate](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase2-development-database-recreate/README.md)。
- Phase 3 正式证据：[phase3-recursive-document-contract](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase3-recursive-document-contract/README.md)。
- Phase 4 正式证据：[phase4-package-contract](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase4-package-contract/README.md)。
- Phase 5 正式证据：[phase5-unified-rule-domain](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase5-unified-rule-domain/README.md)。
- Phase 6 正式证据：[phase6-rule-chain-transaction](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase6-rule-chain-transaction/README.md)。
- Phase 7 正式证据：[phase7-package-runtime](../../../testing/evidence/2026-08-30/TASK-20260829-002/phase7-package-runtime/README.md)。
- Phase 11 正式证据：[phase11-socket-shared-rpc-pipeline](../../../testing/evidence/2026-08-31/TASK-20260829-002/phase11-socket-shared-rpc-pipeline/README.md)。
- Phase 12 正式证据：[phase12-legacy-stage-deletion](../../../testing/evidence/2026-08-31/TASK-20260829-002/phase12-legacy-stage-deletion/README.md)。

## 验收结果

- `VERIFIED`：G042 / Phase 1 current-state inventory、compileable harness 映射、generated bindings 门禁和十命令 checkpoint 已在修复后 fresh 通过；不改变产品运行行为，不包含故意失败测试。独立 Verifier 结论 P0=0、P1=0、P2=0；正式证据见 [phase1-green-contract-baseline](../../../testing/evidence/2026-08-29/TASK-20260829-002/phase1-green-contract-baseline/README.md)。
- `APPROVE / CHECKPOINT READY`：G043 / Phase 2 的显式数据库启动 policy、单事务开发重建、Tauri debug/Release composition、双启动 fixture 和双层 release blocker 已在早期 Verifier FAILED 与 build 绕过 P1 后完成修复；独立 delta 复审 P0/P1/P2=0，可创建 rollback checkpoint。
- `VERIFIED / APPROVED / CHECKPOINT READY`：G044 / Phase 3 初版独立 Verifier findings 与最终 scalar-text P2 均已修复；独立 fresh 十门全部 PASS，P0=0、P1=0、P2=0，无剩余阻断，可创建 rollback checkpoint。
- `VERIFIED / APPROVED / CHECKPOINT READY`：G045 / Phase 4 全部 review findings 已修复；firewall permitted 后完整名定向 1/1 与独立十门 checkpoint 全部 PASS，Verifier P0/P1/P2=0，可创建 rollback checkpoint。历史 132/133 ALF timeout 与短名 `--exact` 0 tests 继续保留且不作为成功证据。
- `VERIFIED / APPROVED / CHECKPOINT READY`：G046 / Phase 5 初始 Verifier/Review findings 已按 RED→GREEN 修复，领域合同、即时跨层消费者、checker 与完整十门均 fresh PASS；最终 reviewer/verifier P0/P1/P2=0、`blockers=[]`，可创建 Phase 5 rollback checkpoint。
- `VERIFIED / APPROVED / CHECKPOINT READY`：G047 / Phase 6 的 terminal-scoped Nth lifecycle、validated Application 唯一事务、save/runtime stats 分离、Infrastructure fail-closed delta no-retry、完整 AppError、即时 generated/TS 消费者与完整十门均 fresh PASS；初版 Reviewer `REQUEST CHANGES`（P1=4/P2=1）、Verifier `FAILED`（P1=2）及第二轮独立复审 `REQUEST CHANGES`（P1=2）均已按 RED→GREEN 修复，最终 Reviewer/Verifier P0/P1/P2=0、`blockers=[]`，可创建 Phase 6 rollback checkpoint。
- `VERIFIED / APPROVED / CHECKPOINT READY`：G048 / Phase 7 两轮 review finding 已按 RED→GREEN 关闭；strict ZIP/shared Manifest 与 actual-byte accounting、唯一 typed transport、package-initiated registration、canonical Base64/FrameResult、stable Domain code 贯穿真实 Socket diagnostic、production WebSocket ceiling、取消/顺序 RPC/raw-vs-wire 边界、活动 runtime 接入及旧 dynamic path 删除均 fresh PASS。完整十门 exit0；最终 Reviewer/Verifier P0/P1/P2=0、`blockers=[]`、`checkpoint_ready=true`，可创建 Phase 7 rollback checkpoint。
- `VERIFIED / APPROVED / CHECKPOINT READY`：G049 / Phase 8 的单 Boa Context 串行、package-relative ESM、dynamic import Promise、固定八 exports、HTTP string、Socket Uint8Array/canonical Base64、generic sidecar marker 与 Proxy Host-binding checker 均已通过；最终 Reviewer/Verifier P0/P1/P2=0、`blockers=[]`、`checkpoint_ready=true`，可创建 Phase 8 rollback checkpoint。需求变更与历史 findings 保留。
- `VERIFIED / APPROVED / CHECKPOINT READY`：G050 / Phase 9 的真实本地Sidecar process、统一主动注册、strict ZIP importer/commit/app-start、exact process ownership、local-vs-remote enable/manual restart、disabled local restart fail-closed、Supervisor唯一错误owner、10秒注册与无retry/replay、enabled+online gate均已通过；最终 Reviewer/Verifier P0/P1/P2=0、`checkpoint_ready=true`。既有non-loopback MCP环境deadline与真实bundle/permissions `NOT_RUN` 继续保留。
- `VERIFIED / APPROVED / CHECKPOINT READY`：G051 / Phase 10 shared HTTP RPC、strict codec、authoritative wire bytes、unchanged/changed Encode、joint transaction与production单路径均已通过focused/affected/static；最终 Reviewer/Verifier P0/P1/P2=0、`checkpoint_ready=true`。唯一checkpoint session3469的既有环境阻塞与人工`NOT_RUN`继续保留。
- `VERIFIED / APPROVED / CHECKPOINT READY`：G052 / Phase 11 shared Socket RPC、Frame consumedBytes gate、unchanged/changed Encode、joint actor lifecycle rollback/commit、typed failure与Socket/HTTP capability隔离，以及review repair后的统一production装配、HTTP+Socket RuleRepository projection/persistence、Socket NthHit actor ownership、真实Relay两权威写出阶段顺序和Encode失败counter rollback均已通过focused/static；checker22/22、Domain87/87、external runtime5/5。最终 Reviewer/Verifier P0/P1/P2=0、`checkpoint_ready=true`；Infrastructure full 600/602中的相关stale断言已修，剩余Android deadline、唯一checkpoint session24690的既有non-loopback环境阻塞与人工`NOT_RUN`继续保留。
- `VERIFIED / APPROVED / CODE CHECKPOINT READY / GLOBAL CHECKPOINT INCOMPLETE`：G053 / Phase 12最终Reviewer/Verifier P0/P1/P2=0，`code_checkpoint_ready=true`；session91671 gate1历史失败、修复focused PASS、gates2-10 `NOT_RUN`均保留，`global_checkpoint_complete=false`。
- `NOT_RUN`：Phase 13 至 Phase 18 产品合同替换、真实macOS bundle/权限弹窗、打包与最终任务验收尚未执行。

## 测试结果

- `PASS`：`pnpm test:task-20260829-002:phase1`，4/4；覆盖当前清单 GREEN、package script 漂移、严格命令序列漂移、重复 ID 与仓库越界路径失败。
- `PASS`：`pnpm test:bindings-check`，5/5；覆盖 fresh、stale、nondeterministic、generator failure，以及 unlink-then-throw 后写回 checked-in bytes 并传播同一个 generator error。
- `PASS`：`pnpm check:bindings`；真实 Release generator 连续运行两次，checked-in = first = second。
- `PASS`：`pnpm scan:architecture`、`pnpm scan:source-size`、`pnpm lint`、`pnpm typecheck`。
- `PASS`：`pnpm test`，61 files / 531 tests。
- `PASS`：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`。
- `PASS`：`cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features -- -D warnings`。
- `PASS`：`cargo test --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features`；全部 workspace targets/features 通过，0 failed、0 ignored。
- `PASS`：`pnpm check:task-20260829-002:checkpoint`；严格依次执行 Phase 1 tests、bindings、architecture、source-size、lint、typecheck、全量前端测试、Rust fmt、Rust clippy、workspace all-target/all-feature tests。
- `OBSERVED THEN PASS`：独立 Verifier 首次完整 checkpoint 的 Rust workspace gate 中，既有 ADB deadline 测试 `cancelled_stalled_response_removes_owned_forward_without_blocking_other_serial` 偶发失败一次，结果 647 passed / 1 failed / 0 ignored；该测试随后定向连续 3/3 PASS，完整十门禁复跑 exit 0。原始失败与复跑摘要已归档到正式证据。
- `PASS`：`git diff --check`。
- `PASS`：`pnpm test:task-20260829-002:phase12`，cross-phase Phase5+Phase12 Node 25/25、Phase5 Cargo/Vitest discovery 9/5、Cargo旧stage fail-closed 2/2。
- `FAILED THEN FOCUSED REPAIRED / NOT RERUN`：唯一完整checkpoint session91671在第1门Phase1 baseline因活动inventory仍要求已删除四阶段fragments失败；后9门NOT_RUN。活动inventory与mutation修复后Phase1 4/4、Phase12和静态门PASS，但完整checkpoint按指令未重跑。
- `PASS`：`pnpm test:task-20260829-002:phase6`；checker mutation/正控 28/28，Cargo 实际发现 Domain9/Application11/Infrastructure8，focused 9/9 + 11/11 + Infrastructure exact 8/8。
- `PASS`：Phase 6 repair 受影响全量 Domain128/128、Application508/508、Infrastructure695/695、Host33/33、Tauri133/133；generated bindings fresh/deterministic、TS focused21/21。
- `PASS`：Phase 6 完整十门单进程 checkpoint exit0；前端63 files/543 tests，Rust workspace all-target/all-feature 0 failed、0 ignored。
- `PASS`：`pnpm test:task-20260829-002:phase7`；checker mutation/正控23/23、Cargo实际发现 ZIP6/transport7、active package peer E2E4/4、production WebSocket ceiling1/1、stable-code diagnostic1/1。
- `PASS`：Phase 7 第二轮 repair 受影响全量 Exchange23/23、Runtime178/178、Infrastructure585/585 及全部 integration targets；bindings deterministic、architecture/source-size/lint/typecheck/fmt/workspace strict Clippy、前端63 files/543 tests 与 `git diff --check`。
- `PASS`：Phase 7 第三轮完整十门单一 PTY exit0；workspace all-target/all-feature 0 failed，既有 MCP non-loopback HTTP 用例本轮通过。最终独立 Reviewer/Verifier 已确认 `VERIFIED / APPROVED / CHECKPOINT READY`，不是环境 blocker。
- `PASS`：精确 Phase6 SHA `56becb38decb5fc836d8274f65cc0a10b0761260` 临时干净 worktree 真实重放 Phase7 transport test/checker，分别以 exit101/exit1 RED；原始输出与 SHA 已进入 Phase7 正式证据。
- `PASS`：`pnpm test:task-20260829-002:phase2`；Node release-blocker tests 8/8、Infrastructure core 6/6、Host policy 3/3。
- `PASS`：`cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure -p intercept-proxy-host --all-targets --all-features`；Host unit 12/12 及全部 integration/architecture tests、Infrastructure unit 651/651 及全部 integration tests 通过。
- `PASS`：`cargo check --release --manifest-path src-tauri/Cargo.toml -p intercept-proxy --lib`；Release composition 编译通过并显式选择 Preserve。
- `PASS`：Phase 3 Domain recursive 6/6、Domain 87/87、Application 459/459、protocol-scripting 160/160、Infrastructure 651/651、Nuvei example 6/6、Socket package dialog 7/7、Phase 3 Node 14/14；最终 P2 Application facade 定向 4/4。
- `PASS`：Phase 3 修复后最终 `pnpm check:task-20260829-002:checkpoint`，前端 62 files / 534 tests，Rust workspace all-target/all-feature 0 failed；bindings freshness/determinism、architecture、source-size、lint、typecheck、fmt、strict clippy 全部通过。
- `PASS`：Phase 3 既有四类旧合同扫描与 `ClearDocument|clear_document|字段值槽|Schema 身份和结构|MAX_PROTOCOL_RULE_INT_TEXT_BYTES|Blob Hex|整数文本不能超过` 精确扫描均为 0；generated bindings SHA-256 `ba0dcb545e4f5c04f381d337a4a11062fef789e8d0b28660f575bff37b7dc356`，Nuvei 派生 ZIP SHA-256 `047fe2701973d860d40fe30f5c74a735e46934d808ffb7dd1f16bf404460e30b`，`git diff --check` PASS。
- `PASS`：Phase4 final manual Serde 修复后 `pnpm test:task-20260829-002:phase4`，checker 21/21、五个 Rust integration targets 13/13、TS guard 7/7；MCP 完整 schema/golden 对语义、SHA 与 evidence 字节漂移均 fail-closed，identity/SemVer metadata 与 Domain constructors corpus parity PASS。
- `PASS`：Phase4 review 修复后 Domain 108/108、`pnpm check:bindings`、typecheck、lint、architecture、source-size、fmt、workspace strict Clippy 与 `git diff --check`；前端既有复验 63 files/540 tests，最终 generated bindings SHA-256 `897edb991e8bd7efc6d114ca4eb1c6b67eb162574e0bb764ebed7a93e39c3c9e`。
- `FAILED / ENVIRONMENT BLOCKED`：Phase4 完整 checkpoint 前九门及前端 63 files/538 tests PASS，Rust workspace 顶层 132/133 后既有 `production_bind_is_reachable_on_current_platform_interfaces_without_false_availability` 在当前 macOS non-loopback MCP HTTP response deadline 超时；定向复测同样超时，未记录为 PASS。
- `NOT EVIDENCE`：firewall permitted 后第一次仅用短函数名配合 `--exact`，实际发现 0 tests；未作为定向验证成功依据。
- `PASS`：完整模块测试名配合 `--all-features -- --exact --nocapture --test-threads=1`，non-loopback 目标用例 1/1 PASS；使用 exact executable `intercept_proxy-b171a3f7a5c9b203`，SHA-256 `c7dc870daca6f4f86eeebe29270ef65d4f61eab70b943b55cd994527544143aa`。
- `PASS`：G045 最终独立 `pnpm check:task-20260829-002:checkpoint` exit 0；前端 63 files/541 tests、顶层 Rust 133/133、workspace all-target/all-feature exit 0，Verifier P0=0、P1=0、P2=0。
- `PASS`：G046 修复后 `pnpm test:task-20260829-002:phase5`；checker mutation/正控 15/15、Cargo 实际发现 Rust9/TS4、Domain 119/119、最小 TS 4/4。
- `PASS`：G046 affected full：Application 497/497、Infrastructure 690/690、Host 33/33、Tauri 133/133；bindings fresh/deterministic、architecture、source-size、lint、typecheck、fmt、workspace strict Clippy 与 `git diff --check` 全部通过。
- `PASS`：G046 `pnpm check:task-20260829-002:checkpoint` exit 0；前端 63 files/542 tests，Rust workspace all-target/all-feature 0 failed。
- `OBSERVED THEN PASS`：G046 修复后首次十门在第7门焦点时序失败（541/542），完整名定向 3/3 PASS；完整十门随后从头复跑 exit 0。失败、未运行后门和复跑均保留在 Phase5 evidence。
- `PASS`：`cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure -p intercept-proxy-host -p intercept-proxy --all-targets --all-features -- -D warnings`。
- `EXPECTED FAIL / NOT_RELEASE_READY`：`pnpm check:task-20260829-002:phase2-release-ready` exit 1，独立发现 1 个临时 reset marker 与 32 个临时 reset contract 引用；该扫描只用于阻止发布，不加入 Phase 2 日常 GREEN checkpoint。
- `EXPECTED FAIL / BUILD BLOCKED`：`pnpm tauri:build` exit 1，在 Android companion build 与 `tauri build` 前由同一 release checker 阻断。
- `EXPECTED FAIL / GENERIC BUILD BLOCKED`：`pnpm tauri build` exit 1，Tauri 明确执行 `build.beforeBuildCommand`，在 `pnpm build` 与打包前由同一 release checker 阻断；`beforeDevCommand` 保持 `pnpm dev`。
- `PASS`：Phase 2 变更后的 `pnpm check:task-20260829-002:checkpoint` fresh exit 0，严格十门禁全部通过；前端仍为 61 files / 531 tests，Rust workspace all-target/all-feature 0 failed。

## CI 情况

- `PASS`：Phase10 repair checker mutation/正控19/19、Cargo focused6/6；production prepare_async、typed Decode/Display/Encode、stable top-level external code、Display fail-open observation、Proxy与capture focused均通过。
- `PASS`：Phase10 affected full：Application460/460、Exchange24/24、Runtime227/227、Infrastructure643/643；legacy HTTP regression12/12。
- `PASS`：Phase10 bindings fresh/deterministic、typecheck、architecture、source-size、lint、fmt、workspace strict Clippy与`git diff --check`。
- `ENVIRONMENT BLOCKED`：唯一完整 checkpoint session3469 前九项及前端64 files/545 tests PASS；最终workspace tests在Tauri129/130后既有non-loopback MCP HTTP exchange deadline超时，剩余targets因`&&`未运行。未重跑完整checkpoint。
- `PASS`：Phase11 checker mutation/正控22/22、production focused3/3、actor lifecycle1/1、真实Socket transaction exact1/1、Domain87/87、external runtime5/5及静态门均通过。
- `ENVIRONMENT BLOCKED`：Phase11唯一完整checkpoint session24690前九项及前端64 files/545 tests PASS；workspace tests在Tauri129/130后既有non-loopback MCP HTTP超时，剩余targets因`&&`未运行。Infrastructure full 600/602中的相关LocalResponder断言已修并精确PASS，剩余Android deadline为环境时序阻塞。
- `LOCAL GREEN / RECHECK PENDING`：Phase12 cross-phase aggregate25/25、Cargo2/2、Domain/前端 affected与静态门PASS；唯一checkpoint session91671 gate1因stale活动inventory失败，修复focused通过但完整checkpoint未重跑，后9门NOT_RUN。

- `NOT_RUN`：未推送、未触发远程 CI。

## 完成总结

- `N/A`：TASK 总体状态仍为进行中。
- 阶段总结：G053 / Phase 12 为 `VERIFIED / APPROVED / CODE CHECKPOINT READY / GLOBAL CHECKPOINT INCOMPLETE`；`code_checkpoint_ready=true`、`global_checkpoint_complete=false`。唯一checkpoint gate1历史失败、focused修复PASS、后9门`NOT_RUN`和人工环境项均保留，TASK 总体仍为进行中。
