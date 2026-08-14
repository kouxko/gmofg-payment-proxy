# ISO 8583 Socket 协议包模板

这是一个用于验证协议包结构的目录模板，不是 ZIP 成品，也不是完整的 ISO 8583 方言实现。

通用协议作者应先阅读：

- [Host API v1](../API.md)：Reader、Document、Context、入口和错误行为。
- [自定义协议指南](../AUTHORING.md)：定长、长度头、分隔符、TLV 和双向不同格式。

这个目录只是完整示例。其他协议可以保留同样的 Manifest/Schema/入口结构，替换 `frame()`、Document 字段以及解析和编码库。

## 文件结构

```text
iso8583-standard/
├── manifest.toml               协议包身份、Schema 和入口声明
├── document.toml               Document 字段及规则变量声明
├── protocol.rhai               frame / decode / encode 入口
├── display.rhai                display 可选入口
├── libraries/
│   └── iso8583.rhai            ISO 8583 字段解析和编码
└── samples/
    └── financial-request.json  拆包、解码和回编码样例
```

`manifest.toml` 就是协议包的导出表。Rhai 本身不需要额外的 `export` 关键字；宿主只调用 Manifest 明确声明的函数。

## Hook 命名

`upstream/downstream` 表示完整的数据流向，`receive/send` 始终以 Proxy 为主体：

```text
hooks.upstream.receive    App -> Proxy
hooks.upstream.send       Proxy -> Server
hooks.downstream.receive  Server -> Proxy
hooks.downstream.send     Proxy -> App
```

因此同一个方向的处理链是：

```text
Upstream:   upstream.receive.frame/decode -> rules -> upstream.send.encode
Downstream: downstream.receive.frame/decode -> rules -> downstream.send.encode
```

## 数据流程

```text
TCP bytes
  -> frame(reader, context)
  -> complete origin Blob
  -> Decode 开：decode(origin, context) -> Document -> rules
     Decode 关：空的 Schema Document，不执行 rules
  -> Encode 开：encode(origin, document, context)；display（若声明）作为旁路展示
     Encode 关：不调用 display，直接发送 origin
  -> TCP bytes
```

- `frame` 和 `decode` 是 Manifest 必需入口；Scripted Listener 始终调用 frame，但可以按方向关闭 Decode。
- `display` 没有独立开关，跟随同方向 Encode；Encode 关闭、Display 未声明或失败时应用显示完整 Frame 的 Hex。
- `encode` 未声明或未启用时，应用发送 `origin`；脚本也可以主动 `return origin`。
- Listener 明确绑定一个协议包的 `id + version`，协议包之间不做自动识别。
- Listener 分别配置 Upstream/Downstream 的 `decode_enabled` 与 `encode_enabled`，四个开关互相独立。
- Decode 关闭/Encode 开启时，encode 收到空的 Schema 绑定 Document；本模板 encode 依赖字段，因此实际使用时应同时开启 Decode。
- 导入会编译此目录所有声明脚本和模块；任意 Rhai 语法、入口参数或返回类型错误都会拒绝导入。
- 应用的版本详情 Dialog 可以查看此模板的 Schema 和入口能力，但不显示 Rhai 源码。

## 这个模板采用的 ISO 8583 Profile

- Socket Frame：2 字节大端长度头，长度不包含头本身。
- ISO 8583 报文：4 字节 ASCII MTI + 8 字节二进制主位图。
- 已实现字段：DE3、DE4、DE7、DE11、DE41、DE49。
- 暂不支持第二位图和其他字段；遇到未实现内容会明确失败，避免悄悄错位解析。

2 字节长度头属于此示例的传输 Profile，并非 ISO 8583 标准强制格式。接入真实系统时，应根据对端规格修改 `frame()` 与 `encode()`。

## Document 与规则变量

`document.toml` 提前声明全部字段。`decode()` 通过宿主提供的 API 创建和填写 Document：

```rhai
let document = document::create();
document.set("amount", 1000);
```

因此应用在收到报文前就知道这些规则变量：

```text
message_type      string
processing_code   string
amount            int
transmission_time string
stan              string
terminal_id       string
currency          string
```

模板实际使用到的宿主对象 API 为：

```text
Reader:   available(), peek_u16_be(offset)
Framing: framing::need_more(total), framing::complete(total), framing::reject(reason)
Document: document::create(), get(name), set(name, value), has(name), fields()
Context:  direction(), stage(), connection_id(), listener_id()
```

这里不是完整 API 清单；通用 `peek/find`、精确返回值、错误和类型规则以 [API.md](../API.md) 为准。这些宿主 API 仍需要由 Rust/Rhai 集成层注册。

`samples/` 当前是说明和测试向量，不会被 Host API v1 自动执行。
