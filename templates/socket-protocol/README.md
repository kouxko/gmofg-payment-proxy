# Socket 协议包开发模板

这个目录是 Socket 协议包开发资料，不只面向 ISO 8583。

```text
socket-protocol/
├── API.md                     Host API 与 Manifest 的稳定契约
├── AUTHORING.md               自定义协议编写步骤和常见拆帧方式
└── iso8583-standard/          可编译、可回编码的完整示例
```

开始编写自己的协议时：

1. 先阅读 [API.md](API.md)，不要从示例代码反推 Host API。
2. 按 [AUTHORING.md](AUTHORING.md) 选择拆帧方式并声明两个方向的字段结构。
3. 复制 `iso8583-standard/` 的目录结构，但替换协议相关的字段结构、拆帧、解析和重建代码。
4. 保留 Manifest 中 `document.upstream/downstream` 与 `hooks.upstream/downstream` 的方向语义。

ISO 8583 只是一个包含位图和定长字段的示例。同一套接口也可以实现 TLV、定长报文、分隔符报文、自定义二进制头、Protobuf 包装帧或其他 TCP 应用协议。

协议包应用级安装；Workspace 只引用精确的 `package_id + version`。Workspace 导出会嵌入所引用的包，整个应用导出会包含全部已安装包，详细冲突和启用规则见 [API.md](API.md#11-安装范围身份与可移植性)。

> 当前状态：Host API v1 已接入产品；导入会严格校验 ZIP、Manifest、字段结构、Rhai 模块和全部入口，运行时按精确包版本执行完整处理链。
