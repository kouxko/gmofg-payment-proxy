# 规则、递归 Document 与协议包

本文是规则编辑、HTTP/Socket Document 处理、本地单文件协议包和远端调试软件包的当前架构合同。
当前运行时决策见 [ADR-010](decisions/ADR-010-in-process-webassembly-protocol-packages.md)。

## 1. 所有权

- Domain 拥有 `RuleDefinition`、递归 `Document`、Schema、唯一 condition/action 配对与验证。
- Application 提供 rule editor context 和 typed factory；UI 只消费 predicate/action capability，不生成默认。
- Exchange 拥有 Reader/Writer 顺序与观察事件，Infrastructure 把统一规则接入 HTTP/Socket actor。
- Proxy 拥有业务连接、网络写出与进程内本地 Component；`/packages` WebSocket 只承载远程调试包。

Workspace 只保存一个 `rule_definitions` 集合。HTTP 与 Socket 内容保持带标签隔离，不能交换 transport
字段或动作；Document 条件和动作通过共同领域合同复用。

## 2. 递归 Document

递归 `Document` 是自然 JSON 树，值类型包括 null、boolean、number、string、object 和 array。JSON
Pointer 定位具体值；Schema 的 object properties 与 array items 描述能力。array items 只是元素类型
模板，只有用户显式创建 index 后，规则本地路径才出现具体索引。

Schema-free 编辑通过 Rust typed factory 创建第一条规则本地叶子。叶子的 predicate/action 保留
`DocumentValueType`，包括只清除值的 Clear；不另建独立 metadata 持久化字段。现有规则本地叶子按
value type 重新读取 Rust capability，因此 null、object、array 和未来由 Rust 暴露的类型不会被 UI
硬编码过滤。

## 3. 条件与 action

每条规则必须且只能包含一个 condition 和一个对应 action，不提供单条规则内的 AND/OR 或 action
列表。condition 使用 Rust capability 给出的 field、selector、operator 和 typed value；Set、Clear、
Insert、Append 是否可用由目标路径的 value type 决定。规则保存会再次在 Domain/Application 校验，
不能依赖前端隐藏非法选项。需要多个独立行为时创建多条规则，它们分别匹配并按规则顺序执行。

HTTP 条件只有一套当前合同：Method、Path 和 Header。终端 IP 与证书指纹只用于连接诊断和抓包展示，
不作为新规则的匹配条件。UI 的
`Path（包含 Query 参数）` 对应内部 request target，是请求入口捕获的原始 `/path?query`；同一
transaction 把这份不可变请求元数据传给请求与响应两个阶段，
响应阶段不得从 status line 重建目标，也不得加入 scheme、host、port 或规范化。Header selector 是
单层 `/name`，名称按 ASCII 大小写不敏感，重复字段按 ANY 匹配。Method 只支持 Equals；其他字符串
字段由 Rust capability 声明 Equals、Contains、StartsWith、EndsWith、Wildcard。

Document 条件路径使用 RFC 6901 扩展：完整 token `*` 只展开一个 object/array 层，多个结果按 ANY
判断。Schema 只提供递归路径选择能力，手动路径始终由同一 Rust factory 校验；无 Schema 时不生成
前端默认字段。普通 HTTP Body 使用内建 JSON Decode/Encode 提供 schema-free Document；只有规则实际
包含 Document 条件或动作时才进入该事务，非法 JSON 按 Decode 失败终止 Exchange，不回退到文本匹配。
协议模式的精确包只由 Listener 绑定决定，规则不复制包身份。Document mutation 与条件复用同一
RFC 6901 单层 `*` 扩展；动作先从当前 Document 解析全部具体路径快照，再以原子方式执行，零命中
不修改 Document，也不创建缺失父节点。旧 `PathOrRequestType`、HTTP
`JsonPath` field 和 Regex operator 已物理删除，不存在 alias、fallback 或双执行路径。

每个 Listener/epoch 使用不可变规则快照，规则排序为：

```text
(priority 升序, rule_id 升序)
```

每条规则只有一个 typed condition 和一个对应 action。统一 Document gate 返回 typed condition
evaluation；普通 HTTP 条件仍由 actor 自己求值。Encode 失败不提交 hit，也不保留 working Document。

## 4. 两个写出阶段

统一规则只在两个网络写出阶段执行：

```text
App -> Proxy:    Frame? -> Decode -> Display -> Envelope
Proxy -> Server: working Document condition -> action -> Encode -> write

Server -> Proxy: Frame? -> Decode -> Display -> Envelope
Proxy -> App:    working Document condition -> action -> Encode -> write
```

每个方向开始时从 Decode 结果创建私有 working Document。规则按
`(priority, rule_id)` 顺序执行；`created_order` 仅用于编辑历史展示，不参与运行时排序。每条规则的唯一
condition 读取当前 working state，命中后执行唯一 action，并供后序规则条件观察。方向完成后只
Encode 一次。Document、普通 HTTP action、Encode
与 actor delta 作为一个事务提交，全部成功才一次提交 lifecycle，任一步失败都回滚。

Direct Socket 不进入 Frame/Decode/Rules/Encode。Display 是未信任观测输出，失败可回退文本/Hex，
但 Frame、Decode、Rules、Encode 或 Writer 失败结束 Exchange。

## 5. package API 1

本地包是一个 WebAssembly Component 文件。顶层唯一 `intercept-proxy:manifest` custom section 固定
`api: 1`、kind、精确 id/SemVer 与上下行递归 Schema；导入先验证 Component、Manifest 和对应 WIT
world，提交后由主进程 Wasmtime 实例化。运行时没有旧 ZIP 迁移、别名、Sidecar 或自动回退路径。

本地 Pipeline 通过 `ProtocolPackageRuntime` 直接调用 Component：HTTP 使用 string，Socket 使用原始
bytes，Document 使用领域对象。只有远程调试软件包主动连接 `/packages` 并发送
`package.register`，其固定方法为：

- `hooks.upstream.frame|decode|encode`
- `hooks.downstream.frame|decode|encode`
- `document.upstream.display`
- `document.downstream.display`

远程 Frame/Decode/Encode 的二进制 wire 使用 Base64 JSON-RPC；Base64 不进入本地 Wasm 调用。
Document 是 JSON 值，Display 是未信任 HTML。精确版本不可调用时引用它的 Listener fail-closed，
不切换版本或 Direct。

## 6. 观察与失败

每次协议处理可记录 received Document、逐规则 typed operation summary、final working Document、
Encode/result/process evidence 和 stable code。operation summary 受共享 observation serialization budget
限制；达到预算后停止收集并设置 `changes_truncated`，业务规则、最终 Document 和 Encode 继续执行。

UI 直接展示 typed 事件，不用占位文案伪造缺失阶段。Display HTML 经过清洗并放入无能力 iframe；
不得用 `dangerouslySetInnerHTML` 注入主文档。

## 7. 持久化和发布边界

SQLite Schema 100 是产品 1.00 兼容基线。Schema 100 数据原样保留；非空数据库存在唯一有效版本标记
`<100` 时，删除 SQLite 主文件、WAL 与 SHM，再创建全新的 Schema 100。未来版本、缺失、重复或损坏
标记均 fail-closed，失败不得改写数据库或用户数据。发布 checker 锁定该单一路径；不存在迁移、兼容
别名或其他 reset 分支。

## 8. 验证重点

- recursive Schema、schema-free condition、单条件/单动作配对与 Insert/Append/Clear typed capability；
- 当前 working state 按序匹配、前序可见、hit 成功提交与 Encode rollback；
- HTTP 与 Socket 两个写出阶段共用统一规则但保持 transport DTO 隔离；
- Component/API 1、WIT、Host WebSocket、远程 `package.register`、断线与精确版本生命周期；
- received/process/final/encoded typed evidence、`changes_truncated` 和 stable error；
- Schema 100 preserve、唯一有效 `<100` 清除重建、未来/缺失/重复/损坏标记 fail-closed 与发布 checker 门禁。
