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

绑定协议包就表示两个方向始终执行完整 `frame -> decode -> rules -> encode -> display` 链，不存在用户可见的解析、重建或展示开关。Encode 仍不能假定所有可选字段一定存在；读取可选字段前应使用 `document.has()`。

## 5. 两个方向可以使用不同协议

两个方向格式不同时，可以分别声明字段结构、展示函数和处理函数：

```toml
[document.upstream]
schema = "schemas/upstream.toml"
display = "display_upstream"

[document.downstream]
schema = "schemas/downstream.toml"
display = "display_downstream"

[hooks.upstream]
frame = "frame_upstream"
decode = "decode_upstream"
encode = "encode_upstream"

[hooks.downstream]
frame = "frame_downstream"
decode = "decode_downstream"
encode = "encode_downstream"
```

两个 decode 分别返回自己方向字段结构约束的 Document；同一方向的条件、修改和展示只使用该方向字段。

## 6. Display 不要承担解析职责

`display()` 只读取 Document。不要在 Display 中重新解析 origin，也不要让 Display 的成功与否影响转发。

```rhai
fn display(document, context) {
    let command = document.get("command");
    `<section><h3>Command ${command}</h3></section>`
}
```

Display 是两个方向都必须声明的只读入口。宿主在解析和规则处理后调用它；任何 Display 错误只影响协议视图并回退十六进制，不改变网络写出结果。

## 7. 从 ISO 8583 示例改成其他协议

| 文件 | 保留什么 | 替换什么 |
| --- | --- | --- |
| `manifest.toml` | Hook 层级和 API 版本 | package id/name/version、Schema 路径和函数名 |
| `document.toml` | Schema 文件结构 | 全部业务字段 |
| `protocol.rhai` | 四入口职责 | Frame 边界、协议头和长度处理 |
| `libraries/iso8583.rhai` | 边界检查、Document 写入方式 | 整个协议解析和编码算法 |
| `display.rhai` | 只读展示职责 | 字段布局和文案 |
| `samples/*.json` | 拆包、完整 Frame、预期 Document | 自己协议的真实测试向量 |

库文件可以改名或拆成多个模块；`libraries/iso8583.rhai` 不是固定路径。

## 8. 交付前检查

- ZIP 根目录直接包含 Manifest。
- 两个方向都声明可编译的 frame/decode/encode，且各自声明 schema/display。
- ZIP 导入前确认主脚本和所有 `import` 模块都能用标准 Rhai 语法编译；语法错误会直接拒绝导入。
- 所有 Schema 字段名、标签和类型设置正确；协议必填条件由 decode/encode 自己校验。
- 测试过一个 Frame 被拆成多个 TCP chunk。
- 测试过多个 Frame 一次到达。
- 测试过空报文、截断报文、超长报文和非法长度。
- decode 输出符合 Schema。
- encode 返回完整 Frame，并重新计算长度和校验和。
- 分别测试应用到代理、代理到上游、上游到代理、代理到应用四个规则边界的顺序与字段结构。
- 没有规则修改时确认 encode 可以逐字节保持 origin；有修改时确认完整重建长度和校验和。
- Display 失败时仍能使用十六进制查看报文，且不影响已经确定的网络写出。
- 应用只展示 Manifest 声明、Schema 和诊断，不展示协议包源码；源码审查和版本控制由协议作者在外部工具完成。
