# Local HTTP 默认响应与双向规则

TASK-20260905-004。用户已确认默认响应合同，本例结果 PASS。

- 修复前单元回归：实际首行为 POST 请求行，期望 HTTP/1.1 200 OK，RED；真实 TCP 两例都在完整响应前 EOF，RED。
- 修复后：单元1例、真实 TCP2例 GREEN。记录于 outputs/connector-green.log 和 outputs/live-green.log，含实际输入/输出完整字节数组。
- 本机响应保留上行处理后的 bytes，仅保留原始 Content-Type/Content-Encoding（包含字段大小写/OWS）；长度使用实际 byte 数重算，旧 Transfer-Encoding、Host、Authorization、Cookie 和自定义请求头不带回。
- 真实 localhost Listener 使用生产 RuntimePipelineAdapter actor；同一 TCP 连接顺序发送两请求验证 keep-alive。`/echo` 返回200+upstream-effective；`/modified`经过当前公开 ReplaceBodyText、SetHeader、CustomHttpStatus 后返回207+downstream-effective和x-local-rule。持久规则快照命中次数为[2,1,1,1]。
- 默认回环的另一条真实连接覆盖空Body与00 ff 81二进制Body。请求Host指向独立localhost TCP trap，结束时确认trap没有接入；Listener正常stop，测试资源仅内存数据库/短期本机Socket。

复测从仓库根目录执行：

```sh
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime --lib local_http_default_response -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --lib local_http_default_response -- --nocapture
```

精确请求、Header、公共RuleDefinition、Body/长度/状态预期和收发/关闭步骤随测试源码保存在resources，二进制输入同时保存为inputs/binary-body.bin/.hex。测试逐次write_all发送完整请求；输入和客户端实际完整响应的byte数组记录在日志。HTTP已完成framing，Socket Frame/Decode Hook为N/A；没有真实外部业务、设备、UI或CI。

派生自TASK-20260903-002 / HTTP-LOCAL-MOCK-001，保留其零上游、双向规则和BodyCodec边界；本次新增当前公开Body替换可建立在合法默认响应之上的合同，不依赖旧MockResponse完整响应替换。旧档案未改写。live-red-build-intermediate.log只是另一个Agent拆分Application时的编译暂态，不算行为RED。

任务最终稳定矩阵和独立审查见[最终验证](../final-verification/README.md)。
