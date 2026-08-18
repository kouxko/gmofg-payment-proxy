# ISO 8583:1987 ASCII Socket 协议包模板

这是应用内置、可直接导出为 ZIP 的 ISO 8583:1987 ASCII 基线 Profile。它完整声明并可编解码
DE2-DE128；DE1 和 DE65 分别作为第二、第三位图指示位，不作为业务字段。

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
    ├── financial-request.json  Upstream 请求拆包、解码和回编码样例
    └── financial-response.json Downstream 响应粘包、解码和回编码样例
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
- ISO 8583 报文：4 字节 ASCII MTI + 8 字节二进制主位图，可选 8 字节二进制第二位图。
- 字段范围：DE2-DE64 和 DE66-DE128；设置 DE65（第三位图）会明确失败。
- 字段编码：ASCII 定长、2 位 LLVAR、3 位 LLLVAR；长度前缀本身为 ASCII 数字。
- 二进制字段：DE52 PIN、DE55 ICC、DE64/DE128 MAC、DE96 Message Security Code。
- `amount`（DE4）保持 `int`，方便现有金额规则；其他数字域使用 `string` 保存前导零。

2 字节长度头属于此示例的传输 Profile，并非 ISO 8583 标准强制格式。接入真实系统时，应根据对端规格修改 `frame()` 与 `encode()`。

ISO 8583 的保留域、国家域和私有域没有跨网络统一内容。本模板将它们作为不透明 ASCII
LLLVAR 字段保存；真实接入必须根据收单机构或交换网络规范调整长度、编码以及内部子域结构。

## Document 与规则变量

`document.toml` 提前声明全部字段。`decode()` 通过宿主提供的 API 创建和填写 Document：

```rhai
let document = document::create();
document.set("amount", 1000);
```

因此应用在收到报文前就知道 MTI 和全部 DE2-DE128 规则变量。例如：

```text
message_type      string
processing_code   string
amount            int
transmission_time string
stan              string
terminal_id       string
currency          string
pin_data          blob
icc_data          blob
reserved_private_127 string
message_authentication_code_2 blob
```

模板实际使用到的宿主对象 API 为：

```text
Reader:   available(), peek_u16_be(offset)
Framing: framing::need_more(total), framing::complete(total), framing::reject(reason)
Document: document::create(), get(name), set(name, value), has(name), fields()
Context:  direction(), stage(), connection_id(), listener_id()
```

这里不是完整 API 清单；通用 `peek/find`、精确返回值、错误和类型规则以 [API.md](../API.md) 为准。这些宿主 API 仍需要由 Rust/Rhai 集成层注册。

`samples/` 是协议作者维护的说明和测试向量，不会被 Host API v1 自动执行；本仓库的 Rust
conformance 测试会逐个加载它们，第三方作者也应在交付前用自己的测试工具执行全部向量。
