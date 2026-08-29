# 嵌套 Document、统一规则与 JavaScript WebSocket 软件包

## 任务信息

- 任务 ID：`TASK-20260829-002`
- 状态：`进行中`
- 任务日期：`2026-08-29`
- 创建时间：`2026-08-29 20:28:30 +08:00`
- 开始时间：`2026-08-29 22:55:17 +08:00`
- 最后更新时间：`2026-08-29 23:56:14 +08:00`
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
- Module 及相对依赖在启动时 load/link/evaluate 一次，exports 缓存到进程退出。注册前只验证所需 export 存在且 callable，不试运行、不探测业务、不自动修复。
- 运行环境只提供 Boa ECMAScript、JSON、`Uint8Array` 和包内相对 ES module import；不提供 Node.js、fs、process、Buffer、fetch、timer、WebSocket 等 Host API。
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
| NDR-JS-01 | 映射当前源码，锁定旧行为与新合同 RED 测试；加入开发期 DB100 每启动重建及正式发布移除门禁 | TASK-20260829-001 | 否 | 进行中 | G042 / Phase 1 基线已 VERIFIED；开发期 DB100 启动重建门禁由下一阶段继续完成 |
| NDR-JS-02 | 实现递归 Document、Number、RFC6901、Schema 和规则本地元数据 | NDR-JS-01 | 否 | 待实现 | 全类型、JSON、路径、数组、Schema/no-Schema 测试通过 |
| NDR-JS-03 | 实现统一 HTTP/Document/Socket 规则、两写出阶段、多动作顺序、终止动作和方向级原子提交 | NDR-JS-02 | 否 | 待实现 | 条件/动作矩阵、前序可见、失败全回滚及生命周期提交通过 |
| NDR-JS-04 | 定义严格 Manifest、稳定错误和唯一 JSON-RPC wire | NDR-JS-01 | 否 | 待实现 | 本地/远程逐字段同形，注册与 Hook contract tests 通过 |
| NDR-JS-05 | 实现通用 Boa Sidecar、固定 exports、ESM 加载和内部字节适配 | NDR-JS-04 | 否 | 待实现 | export 预检、单 Context 串行、HTTP string、Socket Uint8Array/Base64 通过 |
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
- 固定 exports 注册前 existence/callable 预检；Module/相对 ESM 一次求值并缓存；不提供 Host API。
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
- 下一步：主 Agent 创建 G042 本地 rollback commit；随后进入 G043 / Phase 2 数据库 100 开发期每启动重建门禁。

## 修改文件

- `docs/tasks/pending/2026-08-29/nested-document-rules-javascript-websocket-packages.md`：用最终确认合同替换过时的初始任务正文。
- `package.json`：增加 Phase 1 基线、generated bindings 与统一 checkpoint scripts。
- `scripts/check-task-20260829-002-phase-baseline.mjs`、`scripts/check-task-20260829-002-phase-baseline.test.mjs`：校验 current-state inventory、harness 路径、generated-type 旧合同片段及 checkpoint 命令，结构漂移 fail-closed。
- `scripts/check-generated-bindings.mjs`、`scripts/check-generated-bindings.test.mjs`：跨平台验证 generated bindings freshness/determinism；`finally` 无条件写回原字节，回归覆盖 generator 删除输出后抛错并保留原错误。
- `test-support/fixtures/task-20260829-002/phase-1/current-contract-inventory.json`：Phase 1 当前合同、后续 owning phase、测试入口与 checkpoint/rollback 规则。
- `docs/testing/evidence/2026-08-29/TASK-20260829-002/phase1-green-contract-baseline/`：Phase 1 inventory 快照、结构化命令结果、复测入口、ADB 偶发失败与独立复验记录。
- 产品代码：`N/A`。

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

## 验收结果

- `VERIFIED`：G042 / Phase 1 current-state inventory、compileable harness 映射、generated bindings 门禁和十命令 checkpoint 已在修复后 fresh 通过；不改变产品运行行为，不包含故意失败测试。独立 Verifier 结论 P0=0、P1=0、P2=0；正式证据见 [phase1-green-contract-baseline](../../../testing/evidence/2026-08-29/TASK-20260829-002/phase1-green-contract-baseline/README.md)。
- `NOT_RUN`：Phase 2 至 Phase 18 产品合同替换、真实链路、打包与最终任务验收尚未执行。

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

## CI 情况

- `NOT_RUN`：未推送、未触发远程 CI。

## 完成总结

- `N/A`：任务状态为待实现。
