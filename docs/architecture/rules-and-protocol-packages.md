# 规则、Document 与协议包

本文说明统一规则聚合、Document 模型、内置 Rhai 协议包、外部 WebSocket
JSON-RPC 协议包，以及它们在 HTTP/Socket Exchange 中的真实执行顺序。这里描述的是当前源码，
不是未来扩展设想。

## 1. 单一规则聚合与差异内容

Workspace 只保存一个 `rule_definitions` 集合，所有生命周期操作只通过统一
`rule_definition_list/get/save/toggle/delete` 用例。规则顶层统一拥有 identity、revision、名称、
启用状态、priority、created order、Listener 和 `RuleStage`；内容使用带标签的类型保持能力隔离。

| 内容 | 作用对象 | 典型条件 | 典型动作 |
| --- | --- | --- | --- |
| `RuleContent::Http` | HTTP Header、URL、raw Body、连接与可选协议 Document | HTTP 字段、第 N 次命中、可选 Schema 字段严格相等 | HTTP 修改/故障与可选 Document 动作 |
| `RuleContent::Socket` | Socket 协议包解码后的类型化 Document | Schema 字段严格相等 | 记录命中、设置字段、清除字段、清空 Document |

HTTP 内容可以在同一规则中组合 Header/raw Body 与协议 Document 条件和动作；完整规则成功前不会
提交命中计数或一次性禁用状态。Socket 内容不会携带 HTTP 字段或 HTTP 动作。前端只消费 Rust 返回的
stage/content capability 和草稿，不自行拼接默认 payload。

## 2. 统一规则运行顺序

### 2.1 确定性执行顺序

运行时为每个 Listener/epoch 取得统一规则快照。消息阶段先按固定顺序执行，每个阶段内部再按以下键排序：

```text
固定阶段顺序 -> (priority 升序, created_order 升序, rule_id 升序)
```

规则必须已启用、Listener 匹配且阶段匹配才进入求值。单条规则的普通条件是 AND；`NthHit`
基于规则、终端 IP 与证书指纹维护独立计数。命中后动作按声明顺序组合；遇到终止动作立即停止
本规则并停止后续规则。`one_shot` 规则命中后自动禁用并递增 revision。

### 2.2 阶段能力矩阵

保存前由 Rust 领域校验器拒绝跨阶段能力。前端只能展示 Rust 返回的能力，不能自行扩展。

| 统一阶段 | 允许的关键能力 | 明确禁止 |
| --- | --- | --- |
| `app_to_proxy` | App 请求 Decode 后的可选 Document 条件与动作 | HTTP Header/终止动作、响应能力 |
| `proxy_to_upstream` | 请求 Header/URL/raw Body、可选 Document、上行延迟/限速、Mock、上游连接/读写故障 | 响应状态、响应故障、下行限速 |
| `upstream_to_proxy` | Server 响应 Decode 后的可选 Document 条件与动作 | HTTP Header/终止动作、请求能力 |
| `proxy_to_app` | 响应 Header/raw Body、可选 Document、状态码、下行延迟/限速、截断/错误长度/下行断连 | 请求终止动作、上行限速 |
| `tls_handshake` | 客户端证书指纹条件、`RejectTlsHandshake` | HTTP/Document 内容条件、普通内容动作和其他终止动作 |

还必须满足以下不变量：

- 限速和间歇网络的方向由阶段固定：请求只能 upstream，响应只能 downstream；
- 一条规则最多一个终止动作；
- 终止动作必须位于动作列表最后；
- TLS 握手规则只能使用证书条件和第 N 次命中条件；
- 规则不能直接设置 `Content-Length`、`Transfer-Encoding`、`Connection` 等 hop-by-hop Header；
- JSONPath、正则、状态码、延迟、限速、分块和断连偏移都在保存或执行前 fail-closed 校验。

### 2.3 快照与命中提交

Listener 启动时取得不可变配置快照；规则运行服务在明确替换时才接收新快照。一次求值先在内存中
形成结果，再提交命中计数与一次性禁用状态。revision 或规则集合签名不一致时拒绝静默覆盖。

旧 `rules`/`protocol_rules` 双集合、旧完整配置和旧导入 payload 不读取、不转换、不迁移。Schema
版本不是当前版本，或记录仍带旧字段时，Host fail-closed 返回稳定错误，并保持数据库及 sidecar 原样；
只有用户主动清除应用数据后才创建当前 Schema。

## 3. Document 与 Schema

### 3.1 数据结构

`DocumentSchema` 是不可变、有序字段契约，包含：

- `id`：`[a-z][a-z0-9-]*`；
- 正整数 `version`；
- 展示标题；
- 1 到 256 个不重名字段；
- 字段名 `[a-z][a-z0-9_]*`、标签和类型。

当前字段类型只有 `string`、`int`、`bool`、`blob`。`DocumentValue` 使用显式类型标签，文本
`"7"`、整数 `7` 和字节 `[7]` 不发生隐式转换。

`Document` 共享 `Arc<DocumentSchema>`，并按 Schema 顺序保存同长度的可空值槽。因此它能区分：

1. 字段未在 Schema 声明；
2. 字段已声明但当前报文没有值；
3. 字段已有类型正确的值。

当前模型是扁平字段集合，不支持任意嵌套对象或数组。复杂协议应在 Schema 中声明稳定的叶子字段，
或把需要逐字节保真的子结构保留为 `blob`；不得把尚未实现的嵌套 Document 写成现有能力。

### 3.2 Document 规则

包含 Document 的统一规则冻结绑定以下身份：

```text
Listener ID + package id/version + Schema version + content type
```

创建后不能借更新操作切换这些绑定。规则阶段可以修改，但保存前必须使用目标阶段的 Rust capability
重新校验当前全部 HTTP/Document 内容；不兼容时拒绝保存，不能静默隐藏或丢弃内容。每个阶段的运行
程序在构造时验证整份快照，包括 disabled 规则，然后按阶段内
`(priority, created_order, rule_id)` 冻结排序。

条件当前只有类型严格相等。多条件按 AND 执行，空条件恒匹配；未赋值字段使条件不匹配。动作按
声明顺序执行，后续规则可以观察并覆盖前序规则的修改。任何条件或动作失败时 owned 工作副本被
丢弃，不返回半修改 Document。

## 4. Exchange 中的执行位置

### 4.1 协议 Pipeline

```text
HTTP Reader:  read -> Decode -> Display -> Envelope
Socket Reader: read chunk -> Frame -> Decode -> Display -> Envelope
Writer:        clone Document -> Rules -> Encode -> transport write
```

`Envelope.context/document/display` 在 Reader 完成时固定。Writer 只修改 Document 的 clone，
不会重新渲染 `display`；因此 UI 展示的是“收到时事实”，发送内容以 `Sent.context` 为准。

Socket Scripted 模式一次 `read` 只允许得到一个完整 Frame。Frame 返回 `NeedMore` 时继续读；
完整 Frame 后立即进入 Decode。一次读取若同时包含第二个 Frame 或尾部字节会作为 Endpoint 合同
错误失败，当前实现不支持 Socket pipelining。

### 4.2 四个 Document 规则阶段

每个方向只 Decode/Display 一次、Encode 一次，中间串联两组规则：

```text
App -> Proxy:
  Decode -> Display -> AppToProxy Rules -> ProxyToUpstream Rules -> Encode -> Server

Server -> Proxy:
  Decode -> Display -> UpstreamToProxy Rules -> ProxyToApp Rules -> Encode -> App
```

HTTP 若未绑定协议包，使用内置 `http-text` Schema，字段只有 `header` 和 `body`，规则链为空，
Decode/Encode 保持文本往返。绑定 HTTP 协议包后，协议包只处理 UTF-8 Body，HTTP Header 仍由
HTTP 运行时管理；Encode 返回非 UTF-8 Body 会失败。

HTTP 联合执行器按 Workspace 明确的 stage execution order 处理同一方向的规则；前一阶段的
Document 修改可被后一阶段条件观察。Document、HTTP action 和 Encode 全部成功后才提交 hit/revision/
one-shot 元数据；任一步失败都丢弃 working message 与 working Document，不留下半提交状态。

## 5. 内置 Rhai 协议包

### 5.1 包结构与 Manifest

内置包以 ZIP 导入，入口文件由严格 `manifest.toml` 声明：

```text
manifest.toml
protocol.rhai
display.rhai
上行 Schema TOML
下行 Schema TOML
可选的包内 Rhai 模块
```

Manifest `api` 当前必须为 `1`，包身份是精确 `id + SemVer`。`hooks.upstream` 和
`hooks.downstream` 各自声明 `decode`、`encode`；两边同时存在 `frame` 时判定为 Socket 包，
两边都没有 `frame` 时判定为 HTTP 包，只出现一个方向的 frame 会拒绝整包。

导入链依次执行 ZIP 路径/数量/大小/压缩比校验、Manifest 严格 TOML 解析、Schema 校验、模块
解析、Rhai 编译和入口签名校验。只有全部成功才产生 `CompiledProtocolPackage`，不存在半安装
执行状态。

### 5.2 沙箱和资源限制

Rhai 固定版本并关闭浮点、时间、闭包和自定义语法等能力。运行时对操作数、调用深度、字符串、
Blob 和墙钟时间设限，执行支持连接级取消。协议脚本不能直接访问真实 Socket、HTTP、数据库、
进程或 UI；它只通过受限 Host API 处理当前 Context/Document。

协议包版本不可变。Listener 启动时冻结精确版本和执行限制；运行过程中不会自动升级或回退。

## 6. 外部 WebSocket JSON-RPC 协议包

外部进程连接设置页公布的 WebSocket 服务，路径固定为 `/packages`。服务端建立连接后只发送一次
JSON-RPC 2.0 `package.register`；注册返回 API 版本、精确包身份、上下行 Schema 与方法后缀。
Host 形成的方法名为：

- `hooks.upstream.<frame|decode|encode>`；
- `hooks.downstream.<frame|decode|encode>`；
- `document.upstream.<display>`；
- `document.downstream.<display>`。

每次调用携带连接代次和唯一请求 ID。未知响应 ID、重复响应、非法 envelope、Binary 业务消息或
结构不匹配会按协议错误处理；超时、并发额度、消息大小、心跳和关闭都有明确上限。外部包断线后
精确版本变为 offline，引用它的活动 Listener 不会静默切换到内置包或其他版本。

外部包与内置包最终都实现同一组 Frame/Decode/Display/Rules/Encode capability；区别只在能力
调用发生于进程内 Rhai 还是 WebSocket RPC，Exchange 不维护两套业务流程。

## 7. Display 安全边界

脚本或外部进程返回的 Display 是未信任 HTML。Rust 只把它标记为展示结果；前端
`ProtocolSafeDisplay` 再执行两层隔离：

1. DOMParser 白名单清洗，删除 `script/style/iframe/object/embed/svg/math/form`、事件属性、
   URL 属性、refresh、base、link 等主动内容；
2. 放入 `sandbox=""` 的无能力 iframe，并在 `srcDoc` 内注入 deny-by-default CSP。

前端不使用 `dangerouslySetInnerHTML` 把协议 HTML写入主文档。清洗失败、层级过深或 Display
入口失败时只降级为 HTTP Body/Socket Hex 展示；观测失败不阻断已完成的业务传输。

## 8. 失败语义与验证重点

- Frame、Decode、Rules、Encode、transport read/write 属于业务 Pipeline，失败会结束 Exchange；
- Display 属于观测，失败回退文本或 Hex，不改变交易结果；
- capability factory 失败不会构造空实现，而是记录同一 Exchange 的失败并关闭；
- Direct Socket 不加载协议包、不切 Frame、不创建 Document；
- LocalResponder 仍使用同一 Reader/Writer/Pipeline 模型，只把 Server Endpoint 换成本地响应器；
- 透明 Socket 逐 chunk 转发，不执行协议包或 Document 规则。

规则与协议包改动至少应覆盖：四阶段顺序、规则排序、Schema 绑定、未赋值字段、动作覆盖、取消、
资源上限、Display 降级、HTTP UTF-8 边界、Socket NeedMore/尾部拒绝、外部 RPC 超时/断线，以及
“请求无响应能力、响应无请求终止动作、TLS 仅证书拒绝、方向固定、终止动作唯一且末尾”的能力矩阵。
