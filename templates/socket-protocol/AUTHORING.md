# 编写自定义 Socket 协议包

这份指南说明如何把模板改成 ISO 8583 之外的协议。完整 Host API 以 [API.md](API.md) 为准。

## 1. 先确定 Frame，而不是先写字段解析

TCP 只有连续字节流，没有消息边界。首先确认协议如何判断一条完整 Frame：

### 固定长度

```rhai
fn frame(reader, context) {
    let frame_length = 64;
    if reader.available() < frame_length {
        return framing::need_more(frame_length);
    }
    framing::complete(frame_length)
}
```

### 长度头

```rhai
fn frame(reader, context) {
    let header_length = 4;
    if reader.available() < header_length {
        return framing::need_more(header_length);
    }

    // 本例长度头是小端，且只计算 payload。
    let payload_length = reader.peek_u32_le(0);
    let frame_length = header_length + payload_length;
    if frame_length > 1024 * 1024 {
        return framing::reject("frame exceeds 1 MiB");
    }
    if reader.available() < frame_length {
        return framing::need_more(frame_length);
    }
    framing::complete(frame_length)
}
```

必须根据真实协议确认：长度字段大小、大小端、长度是否包含头部、是否包含校验码。

### 分隔符结尾

```rhai
fn frame(reader, context) {
    let delimiter = "\r\n".to_blob();
    let offset = reader.find(delimiter, 0);
    if offset < 0 {
        if reader.available() >= 64 * 1024 {
            return framing::reject("delimiter not found before limit");
        }
        return framing::need_more(reader.available() + 1);
    }
    framing::complete(offset + delimiter.len())
}
```

### Magic + 长度头

先用 `reader.peek()` 验证 magic，再读取长度。Magic 不匹配时应 `reject()`；不要逐字节丢弃数据尝试自动识别另一个协议包。

## 2. 声明规则需要的 Document

Schema 描述的是应用需要展示、匹配或修改的稳定语义，不是把每一个原始字节都机械展开。

例如 TLV 协议：

```toml
id = "terminal-command"
version = 1
title = "Terminal Command"

[[fields]]
name = "command"
label = "Command"
type = "int"

[[fields]]
name = "terminal_id"
label = "Terminal ID"
type = "string"

[[fields]]
name = "unknown_tlvs"
label = "Unknown TLVs"
type = "blob"
```

保留未知字段时使用 `blob`，可以避免 decode 后再 encode 丢失未识别内容。

## 3. Decode 只接收完整 Frame

```rhai
fn decode(origin, context) {
    let document = document::create();

    // 示例：第 0 字节是命令，第 1..2 字节是 payload 长度。
    document.set("command", origin[0].to_int());

    // 根据协议继续解析 TLV、位字段、BCD、定长字符串或校验和。
    // 所有 offset/length 在读取前都要检查 origin.len()。

    document
}
```

解析原则：

- 二进制字段保持 Blob，不要先转 UTF-8；Rhai 的 `as_string()` 是宽松转换，严格文本字段必须先验证原始字节。
- 对每次 offset + length 做边界检查。
- 明确大小端、字符编码、填充、符号和缩放单位。
- 必需字段缺失、校验和错误或格式非法时 `throw`。
- 只写入 Schema 已声明字段，并保持类型一致。

## 4. Encode 可以回原字节，也可以重建

仅观察和匹配：

```rhai
fn encode(origin, document, context) {
    origin
}
```

需要修改线上字段时，重新构造完整 Frame：

```rhai
fn encode(origin, document, context) {
    let payload = encode_payload(document);
    let result = blob(4, 0);

    // 根据协议写入长度、magic、校验和等全部内容。
    write_u32_le(result, 0, payload.len());
    result += payload;
    result
}
```

如果协议有未知字段，decode 时必须把能够无损回编码的信息保存在 Document 的 `blob` 字段中，或者在 encode 中有意识地返回 origin。

Listener 对每个方向分别控制 Decode 和 Encode。协议作者必须考虑四种运行状态：

```text
Decode 关 / Encode 关 -> frame 后原样转发 origin
Decode 开 / Encode 关 -> 可以解析和匹配，但最终仍发送 origin
Decode 关 / Encode 开 -> encode 收到空的 Schema Document，可只处理 origin
Decode 开 / Encode 开 -> 完整 decode、规则和 encode 链
```

因此 Encode 不应假定所有字段一定存在；如果它需要 Decode 产生的字段，应先用 `document.has()` 检查并给出明确错误。

## 5. 两个方向可以使用不同协议

请求和响应格式不同时，可以分别声明脚本和函数：

```toml
[hooks.upstream.receive]
script = "request.rhai"
frame = "request_frame"
decode = "decode_request"

[hooks.upstream.send]
script = "request.rhai"
encode = "encode_request"

[hooks.downstream.receive]
script = "response.rhai"
frame = "response_frame"
decode = "decode_response"

[hooks.downstream.send]
script = "response.rhai"
encode = "encode_response"
```

两个 decode 仍必须返回同一个 `document.toml` 所约束的 Document。方向独有字段在不适用的报文中可以保持未赋值。

## 6. Display 不要承担解析职责

`display()` 只读取 Document。不要在 Display 中重新解析 origin，也不要让 Display 的成功与否影响转发。

```rhai
fn display(document, context) {
    let command = document.get("command");
    `<section><h3>Command ${command}</h3></section>`
}
```

Display 没有独立 Listener 开关，而是跟随当前方向的 Encode：Encode 关闭时宿主不调用 Display；Encode 开启时，
Manifest 声明了 Display 才会调用。Decode 关闭/Encode 开启时 Display 可能收到空 Document，所以应使用 `has()`
保护可能未赋值的字段。任何 Display 错误只使应用回退 Hex。

## 7. 从 ISO 8583 示例改成其他协议

| 文件 | 保留什么 | 替换什么 |
| --- | --- | --- |
| `manifest.toml` | Hook 层级和 API 版本 | package id/name/version、脚本路径和函数名 |
| `document.toml` | Schema 文件结构 | 全部业务字段 |
| `protocol.rhai` | 四入口职责 | Frame 边界、协议头和长度处理 |
| `libraries/iso8583.rhai` | 边界检查、Document 写入方式 | 整个协议解析和编码算法 |
| `display.rhai` | 只读展示职责 | 字段布局和文案 |
| `samples/*.json` | 拆包、完整 Frame、预期 Document | 自己协议的真实测试向量 |

库文件可以改名或拆成多个模块；`libraries/iso8583.rhai` 不是固定路径。

## 8. 交付前检查

- ZIP 根目录直接包含 Manifest。
- 两个 receive Hook 都声明可编译的 frame/decode。
- ZIP 导入前确认主脚本和所有 `import` 模块都能用标准 Rhai 语法编译；语法错误会直接拒绝导入。
- 所有 Schema 字段名、标签和类型设置正确；协议必填条件由 decode/encode 自己校验。
- 测试过一个 Frame 被拆成多个 TCP chunk。
- 测试过多个 Frame 一次到达。
- 测试过空报文、截断报文、超长报文和非法长度。
- decode 输出符合 Schema。
- encode 返回完整 Frame，并重新计算长度和校验和。
- 分别测试每个方向的 Decode/Encode 四种组合；Decode 关闭时 encode 不得盲读未赋值字段。
- Encode 关闭时确认 origin 字节完全不变且 Display 不调用。
- Display 未声明或失败时仍能使用 Hex 查看报文。
- 应用只展示 Manifest 声明、Schema 和诊断，不展示协议包源码；源码审查和版本控制由协议作者在外部工具完成。
