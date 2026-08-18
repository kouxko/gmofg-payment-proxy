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
├── display.rhai                display 必需入口
├── libraries/
│   └── iso8583.rhai            ISO 8583 字段解析和编码
└── samples/
    ├── financial-request.json  Upstream 请求拆包、解码和回编码样例
    └── financial-response.json Downstream 响应粘包、解码和回编码样例
```

`manifest.toml` 就是协议包的导出表。Rhai 本身不需要额外的 `export` 关键字；宿主只调用 Manifest 明确声明的函数。

## 方向与处理顺序

`upstream/downstream` 表示完整的数据流向；每个方向由一份字段结构、一个展示入口以及一组 frame/decode/encode 入口组成：

```text
Upstream:   App -> Proxy rule -> Proxy -> Server rule
Downstream: Server -> Proxy rule -> Proxy -> App rule
```

## 数据流程

```text
TCP bytes
  -> frame(reader, context)
  -> complete origin Blob
  -> decode(origin, context) -> Document
  -> 按边界顺序执行字段规则
  -> encode(origin, document, context)
  -> display(document, context) 生成只读协议视图
  -> TCP bytes
```

- Socket 包两个方向的 `frame`、`decode`、`encode`、Schema 和 `display` 都是必需声明；绑定协议包即执行完整链。
- 没有规则修改时，本模板的 `encode` 会重建等价报文；协议作者也可以在确认字段未变化时主动返回 `origin`。
- 入口明确绑定一个协议包的 `id + version`，协议包之间不做自动识别。
- Display 失败只使应用回退完整报文十六进制，不影响网络写出。
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
