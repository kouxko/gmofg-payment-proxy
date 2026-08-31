# 规则、递归 Document 与协议包

本文是规则编辑、HTTP/Socket Document 处理、本地 JavaScript ZIP 和远端软件包的当前架构合同。
历史决策见 [ADR-009](decisions/ADR-009-nested-document-javascript-package-runtime.md)。

## 1. 所有权

- Domain 拥有 `RuleDefinition`、递归 `Document`、Schema、条件树、action 与验证。
- Application 提供 rule editor context 和 typed factory；UI 只消费 predicate/action capability，不生成默认。
- Exchange 拥有 Reader/Writer 顺序与观察事件，Infrastructure 把统一规则接入 HTTP/Socket actor。
- Proxy 拥有 `/packages` WebSocket、业务连接和网络写出；Boa Sidecar 拥有本地 JavaScript 执行。

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

条件是递归 AND/OR 树，叶子使用 Rust capability 给出的 predicate。action 是有序列表；Set、Clear、
Insert、Append 是否可用由目标路径的 value type 决定。规则保存会再次在 Domain/Application 校验，
不能依赖前端隐藏非法选项。

每个 Listener/epoch 使用不可变规则快照，规则排序为：

```text
(priority 升序, created_order 升序, rule_id 升序)
```

NthHit counter 由 actor 唯一持有。统一 Document gate 返回 typed condition evaluation；普通 HTTP 条件
仍由 actor 自己求值。Encode 失败不消费 NthHit、不提交 one-shot，也不保留 working Document。

## 4. 两个写出阶段

统一规则只在两个网络写出阶段执行：

```text
App -> Proxy:    Frame? -> Decode -> Display -> Envelope
Proxy -> Server: working Document 条件 -> 有序 action -> Encode -> write

Server -> Proxy: Frame? -> Decode -> Display -> Envelope
Proxy -> App:    working Document 条件 -> 有序 action -> Encode -> write
```

每个方向开始时从 Decode 结果创建私有 working Document。规则按
`(priority, created_order, rule_id)` 顺序执行；每条 condition 读取当前 working state，命中的 action
立即更新它并供后序规则条件观察。方向完成后只 Encode 一次。Document、普通 HTTP action、Encode
与 actor delta 作为一个事务提交，全部成功才一次提交 lifecycle，任一步失败都回滚。

Direct Socket 不进入 Frame/Decode/Rules/Encode。Display 是未信任观测输出，失败可回退文本/Hex，
但 Frame、Decode、Rules、Encode 或 Writer 失败结束 Exchange。

## 5. package API 1

本地包是严格 ZIP，根目录包含：

```text
manifest.json
protocol.js
display.js
```

`manifest.json` 固定 `api: 1`、kind、精确 id/SemVer 与上下行递归 Schema。导入先验证 ZIP 路径、数量、
大小、压缩比、strict JSON、Schema 和模块路径；提交后由独立 Boa Sidecar 加载。当前 host 不注入
Node、文件系统、process、Buffer、fetch、timer 或 WebSocket bindings；这是当前 host surface，不是
对 Boa default/native capabilities 的概括。运行时没有旧包迁移、别名或第二执行路径。

本地 Sidecar 与远端软件包都主动连接 `/packages` 并发送一次 `package.register`。固定方法为：

- `hooks.upstream.frame|decode|encode`
- `hooks.downstream.frame|decode|encode`
- `document.upstream.display`
- `document.downstream.display`

Frame/Decode/Encode 的二进制 wire 使用 Base64 JSON-RPC 或 JavaScript `Uint8Array` 边界；Document 是
JSON 值，Display 是未信任 HTML。精确版本离线时引用它的 Listener fail-closed，不切换版本或 Direct。

## 6. 观察与失败

每次协议处理可记录 received Document、逐规则 typed operation summary、final working Document、
Encode/result/process evidence 和 stable code。operation summary 受共享 observation serialization budget
限制；达到预算后停止收集并设置 `changes_truncated`，业务规则、最终 Document 和 Encode 继续执行。

UI 直接展示 typed 事件，不用占位文案伪造缺失阶段。Display HTML 经过清洗并放入无能力 iframe；
不得用 `dangerouslySetInnerHTML` 注入主文档。

## 7. 持久化和发布边界

SQLite Schema 100 是产品 1.00 兼容基线。Phase17 已删除 pre-100 recreate/reset policy、marker 和
启动分支：Schema 100 数据原样保留；Schema `<100`、未来 Schema、缺失/重复/损坏标记均 fail-closed，
且失败不得改写数据库 bytes 或数据。发布 checker 会阻止临时 reset 合同重新进入生产启动路径。

## 8. 验证重点

- recursive Schema、schema-free first leaf、AND/OR、Insert/Append/Clear typed capability；
- 当前 working state 按序匹配、ordered action、前序可见、NthHit/one-shot 成功提交与 Encode rollback；
- HTTP 与 Socket 两个写出阶段共用统一规则但保持 transport DTO 隔离；
- ZIP/API 1、Boa sandbox、`package.register`、超时/额度/断线与精确版本生命周期；
- received/process/final/encoded typed evidence、`changes_truncated` 和 stable error；
- Schema 100 preserve、pre-100 fail-closed/no-mutation 与发布 checker 门禁。
