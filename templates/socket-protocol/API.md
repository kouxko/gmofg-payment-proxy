# Socket Protocol Package Host API v1

本文是协议包作者的接口契约。示例中的具体协议格式不属于 Host API。

> 实现状态：ZIP/Manifest/Schema 校验、Rhai 编译以及 Document/Context Host API 已在 Rust 中实现并
> 通过契约测试。Reader/Framing 与入口执行链仍在后续任务中，因此当前模板尚未接入产品 Socket 数据面。

## 1. 包结构和路径

协议包最终是普通 ZIP，解压后的根目录必须直接包含 `manifest.toml`：

```text
custom-protocol-1.0.0.zip
├── manifest.toml              必需
├── document.toml              必需
├── protocol.rhai              文件名由 Manifest 决定
├── display.rhai               可选
├── libraries/                 可选
└── samples/                   可选，仅作为说明和测试向量
```

路径规则：

- `script` 和 `schema` 都是相对包根目录的 UTF-8 路径。
- 不允许绝对路径、`..`、符号链接逃逸或访问包外文件。
- Rhai 模块使用 `import "libraries/parser" as parser`；规范写法省略 `.rhai`。
- 导入路径仍受包根目录限制，不能加载系统或用户目录中的模块。
- Manifest 声明的脚本、所有递归导入模块和入口函数在写入注册表前统一编译验证。任意 Rhai 语法错误、
  模块解析错误、入口缺失、参数数量或返回类型错误都会拒绝导入。

### 1.1 安装范围、身份与可移植性

- 协议包安装在应用级注册表，不属于某一个 Workspace。
- 包的不可变身份是 `package.id + package.version`。
- 协议包没有内容摘要或数字签名字段，ZIP 中也不要求这类文件。
- 同一个 `id + version` 已安装时直接复用现有版本；新导入内容不能覆盖它。作者修改包内容时必须提升版本号。
- 编译 AST、缓存和本机安装路径不进入任何导出文件；目标机器重新校验并编译脚本。

可移植文件规则：

| 文件 | 协议包内容 |
| --- | --- |
| `.intercept-workspace` | 嵌入该 Workspace 的 Listener 精确引用的全部 `id + version` 包 |
| `.intercept-config` | 嵌入应用已安装的全部协议包，并保存应用级启用/停用状态 |

导入 Workspace 时：

1. Rust 在修改 Workspace 前校验全部需要安装的嵌入包、Manifest、Schema、模块和入口函数。
2. 已安装的精确 ID/版本直接复用，不比较或覆盖内容；缺失版本安装到应用级注册表。
3. 任一缺失版本校验失败都会让整个 Workspace 导入失败，不留下部分 Workspace 或部分协议包。
4. 从 Workspace 新安装的协议包默认停用；用户在协议包页面检查并显式启用后，相关 Scripted Listener 才能启动。
5. Workspace 只保存 Listener 的 `package_id + package_version` 引用，不保存全局启用状态。

导入整个应用配置时，Rust 原子校验并恢复全部协议包和它们的启用状态；仍不会恢复编译缓存或运行中的连接。

### 1.2 启用、停用与删除

协议包启用状态属于应用级注册表：

```text
启用   -> 通过校验且 API 兼容的包可以被 Scripted Listener 启动使用
停用   -> 包和 Listener 引用保留，但不能启动新的 Scripted Listener Runtime
删除   -> 从应用级注册表移除该精确 id + version
```

引用约束：

- 只要有运行中的 Listener 引用该版本，就拒绝停用；现有连接不会在运行中失去脚本。
- 只有已停止 Listener 引用时允许停用；Listener 配置继续保留精确 ID/版本。
- 启动引用已停用包的 Listener 时失败，并返回包 ID/版本和“前往协议包页面启用”的建议操作。
- 只要任何已保存 Listener 仍引用该版本，无论是否运行，都拒绝删除。
- 删除前必须由 Rust 再次查询引用，不能只相信前端已显示的使用者列表。
- 包启用、停用和删除不自动修改、重绑定或升级任何 Listener。

UI 应展示阻止操作的 Workspace/Listener 使用者，并提供跳转；是否允许操作及稳定错误码由 Rust ViewModel/Command 决定。

### 1.3 导入与应用详情

导入是“先完整校验、后原子安装”：

1. Rust 读取 ZIP 并校验路径、文件数量和解压大小。
2. 严格解析 Manifest 与 Document Schema。
3. 编译所有声明脚本和递归导入模块，验证 Rhai 语法与入口函数参数数量。
4. 只有前三步全部成功，才返回无源码预览并允许安装。
5. 安装使用已经校验的同一份有界内容，不重新信任可能已被替换的原文件路径。

失败不会产生协议包记录、部分文件或可复用编译缓存。应用的协议包页面按 package ID 列表展示；点击包行打开 Dialog，
Dialog 列出所有已安装版本，并可查看精确版本身份、能力、Schema、校验结果和使用者。Rust ViewModel 不包含 Rhai
源码、原 ZIP、AST 或绝对安装路径，应用也不提供源码查看/编辑入口。

## 2. Manifest

```toml
api = 1

[package]
id = "custom-protocol"
name = "Custom Protocol"
version = "1.0.0"

[document]
schema = "document.toml"

# 可选；两个方向共用一个 Document 展示入口。
[document.display]
script = "display.rhai"
function = "display"

# App -> Proxy
[hooks.upstream.receive]
script = "protocol.rhai"
frame = "frame"
decode = "decode"

# Proxy -> Server；整个区块可省略。
[hooks.upstream.send]
script = "protocol.rhai"
encode = "encode"

# Server -> Proxy
[hooks.downstream.receive]
script = "protocol.rhai"
frame = "frame"
decode = "decode"

# Proxy -> App；整个区块可省略。
[hooks.downstream.send]
script = "protocol.rhai"
encode = "encode"
```

约束：

- `api` 必须是宿主支持的整数版本，目前只有 `1`。
- `package.id` 使用小写字母、数字和连字符，匹配 `[a-z][a-z0-9-]*`。
- `package.version` 使用 SemVer；Listener 固定引用 `id + version`。
- 两个 `receive` 区块及其中的 `frame/decode` 都是必需的。
- `document.display` 和两个 `send` 区块是可选能力。
- 两个方向可以指向不同脚本和不同函数名；示例复用同一函数不是限制。
- 入口必须是脚本顶层函数，函数名和参数数量必须与声明匹配。

方向始终以 Proxy 为观察者：

```text
hooks.upstream.receive    App -> Proxy
hooks.upstream.send       Proxy -> Server
hooks.downstream.receive  Server -> Proxy
hooks.downstream.send     Proxy -> App
```

## 3. 入口函数

| 入口 | 函数形式 | 必需 | 返回值 |
| --- | --- | --- | --- |
| `frame` | `frame(reader, context)` | receive 必需 | `FramingDecision` |
| `decode` | `decode(origin, context)` | receive 必需 | 当前 Schema 的 `Document` |
| `display` | `display(document, context)` | 可选 | HTML `string` |
| `encode` | `encode(origin, document, context)` | send 可选 | 一条完整 Frame `Blob` |

调用约定：

- `origin` 永远是一条完整 Frame，包含 `frame()` 识别出的协议头、长度头或结束符。
- `decode()` 收到的 `origin` 不会与其他 Frame 共用 Blob。
- 规则引擎在 `decode()` 之后读取或修改 Document。
- `encode()` 收到同一条 `origin` 和规则处理后的 Document。
- `encode()` 返回完整 Frame，不只是 payload；宿主不会自动补长度头或校验和。
- `display()` 只读 Document，不参与网络转发。Encode 先确定写出 bytes；网络写出不等待 Display 完成。

### 3.1 Listener 独立开关

Manifest 声明协议包能够提供的入口；Listener 决定每个方向是否调用 Decode/Encode：

```text
upstream.decode_enabled
upstream.encode_enabled
downstream.decode_enabled
downstream.encode_enabled
```

- `frame` 在 Scripted 模式始终调用，没有开关。
- 两方向的 `decode_enabled`、`encode_enabled` 独立保存，共有 16 种合法组合。
- Display 没有独立开关，跟随同方向 `encode_enabled`。
- Manifest 未声明该方向 Encode 时，Listener 不能开启对应 `encode_enabled`；伪造配置由 Rust 拒绝。
- Decode 关闭时不调用 Decode、不执行 Document 规则；宿主为 Encode 创建空的当前 Schema 绑定 Document。
- Encode 关闭时不调用 Encode 或 Display，发出字节逐字节等于 origin，应用显示 origin Hex。
- Encode 开启时调用 Encode；若 Manifest 声明 Display，也调用 Display。Display 缺失或失败只回退 Hex。

单方向四态：

| Decode | Encode | Document/规则 | 发出字节 | 展示 |
| --- | --- | --- | --- | --- |
| 关 | 关 | 不调用 Decode/规则 | origin | Hex |
| 开 | 关 | Decode + 只读匹配；修改不进入线路 | origin | Hex |
| 关 | 开 | 空 Schema Document，不执行规则 | Encode 结果 | Display 若声明，否则 Hex |
| 开 | 开 | Decode + 规则匹配/修改 | Encode 结果 | Display 若声明，否则 Hex |

## 4. Rhai 运行环境

Host API v1 使用 Rhai 1.25.x 官方语法，提供 Rhai 标准的基本值、字符串、数组、Map 和 Blob 操作。

允许：

- 纯计算、条件、循环和函数调用。
- 包内 Rhai 模块导入。
- `Blob`、String、Array、Map 等标准操作。
- 本文明确列出的 Host API。

不允许：

- 文件系统、网络、Socket、进程、环境变量和系统命令。
- 动态加载原生插件或包外脚本。
- `eval` 或运行时生成脚本。
- 从脚本直接发送网络数据或保存真实连接对象。

宿主会限制单次调用的操作数、调用深度、Blob/字符串大小和执行时间。超过限制等同入口执行失败；脚本不能依赖无限循环或无限内存。

## 5. Blob

`Blob` 是字节数组，每个元素取值 `0..255`。

模板使用的标准操作：

```text
blob()                         空 Blob
blob(length, fill)             创建 Blob
value.len()                    字节数
value[index]                   读取或修改一个字节
value.extract(offset, length)  复制指定范围
value += other_blob            追加字节
text.to_blob()                 UTF-8 字节
blob.as_string()               按 UTF-8 宽松转换
```

Rhai 1.25.x 的 `as_string()` 会把非法 UTF-8 序列替换为 Unicode replacement character，而不是严格失败。二进制协议不要先转成 String；严格 ASCII/UTF-8 字段必须先验证原始字节，再调用 `as_string()`。

## 6. Reader

`Reader` 是当前连接、当前方向的只读 FIFO 视图，只在一次 `frame()` 调用期间有效。

```text
reader.available()                   -> int
reader.peek(offset, length)          -> Blob
reader.peek_u8(offset)               -> int
reader.peek_u16_be(offset)           -> int
reader.peek_u16_le(offset)           -> int
reader.peek_u32_be(offset)           -> int
reader.peek_u32_le(offset)           -> int
reader.find(pattern, start_offset)   -> int
```

精确语义：

- `available()` 返回当前已经缓冲的字节总数。
- 所有 offset 都从当前 FIFO 第一个未消费字节开始，以 `0` 为起点。
- `peek*` 不消费数据；越过 `available()` 会抛错，所以调用前必须检查长度。
- `find()` 返回 pattern 第一个字节的 offset；没有找到返回 `-1`，不消费数据。
- 空 pattern 非法；`start_offset` 必须位于 `0..available()`。
- Reader 不能存入全局变量、Document、Array 或 Map，也不能从 `frame()` 返回。

## 7. FramingDecision

`frame()` 必须返回下面三种结果之一：

```text
framing::need_more(total_bytes)  当前 Frame 至少需要缓冲到的总字节数
framing::complete(frame_bytes)   FIFO 前 frame_bytes 字节组成一条完整 Frame
framing::reject(reason)          当前流无法继续解析，关闭连接并记录原因
```

注意：

- `need_more(20)` 表示“缓冲区总长度达到 20”，不是“再读取 20 字节”。
- `complete(20)` 会让宿主取出 FIFO 前 20 字节作为 `origin`，剩余字节留给下一条 Frame。
- `total_bytes` 必须大于当前 `available()`，否则视为无进展错误。
- `frame_bytes` 必须在 `1..available()` 内。
- `frame()` 不得自己消费 Reader，也不能返回 payload 长度代替完整 Frame 长度。
- EOF 时仍不足以组成 Frame，宿主按截断 Frame 关闭连接并记录错误。
- 脚本与宿主都会执行最大 Frame 限制；超限必须 reject，不能无限请求更多字节。

## 8. Document Schema

```toml
id = "custom-message"
version = 1
title = "Custom Message"

[[fields]]
name = "message_type"
label = "Message Type"
type = "string"

[[fields]]
name = "payload"
label = "Payload"
type = "blob"
```

规则：

- `id` 匹配 `[a-z][a-z0-9-]*`，`version` 是正整数。
- 字段名匹配 `[a-z][a-z0-9_]*`，并拒绝 Rhai 保留字、重复名称。
- `label` 和 `title` 只用于 UI，不参与脚本变量绑定。
- Schema 只声明允许出现的稳定字段和类型，不声明协议必填条件；条件完整性由具体协议脚本校验。
- 字段只接受 `name`、`label`、`type`；包括 `required` 在内的未知键会使协议包导入失败。
- Document 是一层扁平字段集合；首版不支持嵌套 Schema、Array 或 Map 字段。

Host API v1 字段类型：

| Schema type | Rhai value | 用途 |
| --- | --- | --- |
| `string` | String | 文本、编码后的标识符 |
| `int` | INT（有符号 64 位） | 整数、最小货币单位 |
| `bool` | bool | 标志位 |
| `blob` | Blob | 未解释或需要保真的二进制字段 |

涉及小数的协议，v1 使用缩放后的 `int` 或保留原始 `string`；`decimal` 在精确运行时类型确定前不属于 v1 Schema 类型。

每个 Schema 字段会成为规则编辑器中的同名、同类型变量。当前报文未赋值的字段不存在于 Document 值集合，
但仍存在于规则变量目录中；规则读取值前可先判断字段是否存在。

## 9. Document

`Document` 由宿主创建并自动绑定当前协议包的 Schema：

```text
document::create()              -> Document
document.get(name)              -> Dynamic
document.set(name, value)       -> ()
document.has(name)              -> bool
document.fields()               -> Array<Map>
```

行为：

- `create()` 不接收 Schema 参数；宿主根据当前入口绑定正确 Schema。
- `set()` 只允许 Schema 已声明字段，并严格检查对应 Rhai 类型。
- 写入未知字段或错误类型会抛错。
- `has()` 表示当前 Document 是否已经给该字段赋值；未知字段会抛错。
- `get()` 返回已赋值字段；字段未赋值或未知时会抛错。可能未赋值的字段应先调用 `has()`。
- `fields()` 按 Schema 声明顺序返回全部字段，便于通用 Display。

Decode 关闭但 Encode 开启时，宿主创建同一种 Schema 绑定 Document，但所有字段初始未赋值。`has()` 返回 false，
`get()` 仍按未赋值规则报错；Encode 脚本可以忽略 Document、只处理 origin。

`fields()` 每一项是：

```rhai
#{
    name: "amount",
    label: "Amount",
    type: "int",
    present: true,
    value: 1000       // present=false 时为 ()
}
```

Document 不保存 origin、Socket、修改记录或网络状态，也不提供 `encode()`、`send()`、`is_dirty()` 或 `changes()`。

## 10. Context

`Context` 是当前 Frame 的只读调用上下文：

```text
context.direction()       -> "upstream" | "downstream"
context.stage()           -> "receive" | "display" | "send"
context.connection_id()   -> string
context.listener_id()     -> string
```

- 同一 Frame 的 decode、rules、display 和 encode 共享相同 `connection_id` 和方向。
- Context 不能修改，也不能存入全局变量或 Document。
- v1 不暴露真实 Socket、远端文件、证书、密钥或发送方法。

## 11. 失败与回退

| 场景 | 固定行为 |
| --- | --- |
| ZIP、Manifest、Schema、Rhai 语法、导入模块或入口无效 | 拒绝导入，不留下部分状态 |
| Scripted Listener 找不到精确包版本 | Listener 启动失败 |
| `frame` 抛错、reject、无进展、超时或超限 | 关闭当前连接 |
| EOF 时存在不完整 Frame | 关闭当前连接并记录截断错误 |
| `decode` 失败 | 不发送半成品，关闭当前连接并记录错误 |
| `display` 未声明、随 Encode 关闭或失败 | 显示 origin Hex；Display 失败不影响网络 |
| `encode` 未声明或关闭 | 原样发送 origin |
| `encode` 失败、返回非 Blob 或返回超限数据 | 不发送半成品，关闭当前连接 |

Display 返回值作为不可信 HTML，由宿主在隔离区域中清洗和渲染。脚本不能依赖 `<script>`、事件属性、外部资源、应用 API 或任意内联执行能力。

## 12. Samples

`samples/` 是可选的协议作者测试向量。Host API v1 不规定应用自动执行样例，也不把样例当作入口。

推荐 JSON 字段：

```json
{
  "description": "human readable case",
  "tcp_chunks_hex": ["...", "..."],
  "complete_frame_hex": "...",
  "expected_document": {},
  "expected_encode": "same_as_complete_frame",
  "expected_display": "html"
}
```

- `tcp_chunks_hex` 用来验证 TCP 拆包方式不会改变 Frame。
- `complete_frame_hex` 是 `decode()` 的 origin。
- `expected_document` 的键必须来自 Schema。
- 样例格式供模板测试和人工审查使用；未来若引入自动样例 Runner，需要通过新的 API 版本或独立样例 Schema 声明。
