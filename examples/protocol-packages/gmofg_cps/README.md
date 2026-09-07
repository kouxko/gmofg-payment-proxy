# GMOFG CPS HTTP Component

`gmofg-cps@1.0.0` 提供 HTTP 上下行 Decode / Display / Encode，以及 downstream
Finalize 响应重签。业务合同来自 `jp_gmofg_cps` 的
`db99af6fde2de3f8ae8f29777e19aa45d920c3f9`，覆盖 pay、confirm、refund、reverse。

## 构建和使用

从 Proxy 仓库根目录执行：

```sh
deno run -A examples/protocol-packages/gmofg_cps/build.mjs
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-package-runtime --test gmofg_cps_component -- --nocapture
examples/protocol-packages/gmofg_cps/tests/java_reference/run.sh
```

需要 Rust `wasm32-wasip2` target、Deno 和本仓库 WIT；Java 对照需要 JBR 与 Gson，
详见 [Java oracle](tests/java_reference/README.md)。构建器先运行原生测试，再生成并校验
Component 与内嵌 manifest，产物是 `dist/gmofg-cps-1.0.0.wasm` 和同名 `.sha256`。

将单个 `.wasm` 文件导入支持 HTTP Finalize 的 Proxy，绑定 HTTP Listener 的协议包。
不需要单独导入 manifest。规则的 Document 路径直接对应完整 CPS JSON，例如
`/data/result/amount`。下行顺序为 Decode → Rules → Encode → Finalize → HTTP framing。
Finalize 使用 Host 保留的 App 原请求 URL，金额改动不会重新生成 URL 中的 token。

## Decode、Display、Encode

有效 JSON 解码为相同结构的 Document，所有未知字段保留。Display 显示经过 HTML 转义的
完整 JSON；空或非 JSON 正文作为原文 string Document 显示。非法 Document 返回 PackageError。

未修改的 Document 保留原始正文，包括空白与数字表示；比较采用已有 JSON 包的 Host f64
数字语义。修改后输出 JSON，原始非 JSON 的 string Document 修改后输出该字符串。
Host Document 的数字精度边界仍适用；需要精确保留超过 f64 精度范围的数据时使用 JSON 字符串。
Decode / Display / Encode 不签名；请求方向没有 upstream Finalize。

## 响应签名范围

仅对 POST、2xx 响应、以下路径且包含非空 `data.sign` 的可解析 CPS 响应重签。
允许 Host 配置的路径前缀；绝对 HTTP(S) URL 和 origin-form 路径均支持。

| 响应模型 | gateway 后的接口名 | 接口名后的固定路径 |
| --- | --- | --- |
| Pay | stub-products-pointPay / products-pointPay / stubPay / pay | `version=1.0/B01/1/0/{token}` |
| Confirm | stub-products-pointConfirm / products-pointConfirm / stubConfirm / confirm | `version=1.0/B01/1/0/{token}` |
| Refund | stub-products-refund / products-refund / stubRefund / refund | `B01/1/0/{token}` |
| Reverse | stub-products-reverse / products-reverse / stubReverse / reverse | `B01/1/0/{token}` |

URL 路径末段百分号解码一次作为原 token；保留补空格和字面 `+`，不 trim，不把 `+` 当空格。
query / fragment 不属于 token。缺 token、未知接口、非 2xx、空/非 JSON 正文、缺失或空 sign、
必要模型类型不适用时，完整 Finalize 输入原样返回，保留前序规则结果。
Host 五字段封装损坏或 Wasm 执行失败明确报错。

## 字段与算法

| 层级 | 参与签名的模型字段 |
| --- | --- |
| 根 | `data`、`meta` |
| meta | `code`、`message` |
| data | `errorCode`、`errorInfo`、`subErrorCode`、`result` |
| result 基类 | `orderDetailId`、`orderId`、`transTime`、`transStatus` |
| Pay result | 基类 + `currencyCode`、`payType`、`extendedInfo`、int `amount`、int `amountRmb` |
| Confirm result | 基类 + `payType`、`payCheckDate` |
| Refund result | 基类 + `currencyCode`、int `refundAmount` |
| Reverse result | 基类 + `currencyCode`、int `amount` |

继承字段合并后按名称排序，拼接 `name=value` 并用 `&` 分隔，嵌套模型用 `{...}` 包围。
排除 `sign`、未知字段、null 和 int 0；空字符串保留；字段值内 ASCII 空格删除，tab / newline
保留。字符串字段接受 Gson 的 number / boolean 标量转换，int 字段按 Gson 的整数转换处理。
按用户确认，`1e2`、`1E+2` 和 `-0` 等数字词法场景不纳入真实环境支持与验收范围；
当前实现不保证这些数值作为 Java String 字段时保留原始词法。
最终输入为 `{<根字段串>&key=B2B<原token>}`，以 UTF-8 计算 MD5，转为大写十六进制。
[Android Charset 文档](https://developer.android.com/reference/java/nio/charset/Charset)
确认 Android 默认字符集为 UTF-8。

Finalize 只写回 `body.data.sign`，保留其余 JSON 字段以及 url / method / code / header。
签名已经正确时完整输入原样返回；Host 负责最终 Content-Length 等 framing。
业务错误的 2xx 响应仍按模型重签；非 2xx 对应 App 不验签的错误解析路径。

## 回归与边界

[Java 向量](tests/fixtures/java-signatures.json) 包含四类模型、成功/业务错误、继承字段、
空值/空串/零、Unicode、空格、类型转换和无效类型。`pay-before` 金额 100 的签名为
`79A34C9A4170AE8A202955FAAE937BDA`；仅改金额为 250 后，使用相同原 token，签名为
`F070F340438D40D0BC07295E241EE519`。这些是合成测试输入。

包不修改 CPS Android 源码，不生成请求签名，也不将 Display 作为最终验签或支付成功结论。
本地 Java、Host 和 HTTP 测试不能替代真实 CPS 服务、TLS/mTLS 或终端业务验收。
