# GMO-FG Payment DLL Wasm Component

`gmofg-payment-dll@1.0.0` 是 GMO-FG Payment DLL 的 HTTP 协议包，仅对 downstream 响应执行
字段级业务 Decode、Encode 和 Display。包覆盖连接测试 `0000`、Credit DLL `0001`、UnionPay DLL
`0002`，并接受 `D48` 等没有下载表的错误响应。

## Downstream 合同

- Decode 校验 JSON、`TransactionType`、每张表的 `Length`、表 ID、固定字符宽度、CA Key 数量、
  `@`/`@@` terminator 和交易类型对应表。
- Document 保留原协议的顶层字段名和表键；`Length` 是派生值，不进入可编辑 Document。
- 表内所有业务字段、空格和保留区均进入 Document。未知保留区按源码位置命名，例如
  `reserved_155_156`。
- 未修改 Document 时 Encode 原样返回 `original-input`，保留 JSON 空白、字段顺序和换行。
- 修改 Document 时 Encode 严格校验每个字段宽度，重建 `Data` 并按 Unicode 字符数自动计算
  `Length`；不截断、不补空格、不跳过未知表。
- Display 按完整 JSON Object/Array 关系渲染为树：每个容器节点使用原生 `details/summary` 独立
  展开或收起，标题显示字段名或数组下标、JSON 类型和成员数量；根节点显示“基本信息”且默认展开，
  子节点默认收起，不显示 `$`/`$.` 技术路径。所属 Object 的标量叶子继续使用语义 HTML table，
  保留区和字符串空格完整保留，所有字段名和值均进行 HTML 转义，不输出 JSON `<pre>`。每张 table
  使用不同且稳定的边框和底色，重复渲染不会随机换色。完整 Credit 样本保持在 Proxy 已确认的
  1 MiB / 8192 DOM 节点安全渲染上限内。

当前已知表：

| ID | Document | 结构 |
| --- | --- | --- |
| `0` | `KICC_01/GICC_01[].tables[]` risk | 固定 412 字符，包含两个显式保留区 |
| `1` | `KCCI_01[]` | 37 字符头 + N 个 32 字符卡号范围 + `@` |
| `2/3` | `KJSI_01` | 两段组合表，合计 90 字符 |
| `4` | `KBAT_01` | 8 字符内容 + `@` |
| `5` | `KDST_01/GDST_01` | 38 字符内容 + `@` |
| `6` | `KCAK_01/GCAK_01[]` | 4 字符头 + N 个 602 字符 CA Key + `@` |
| `7` | `KBRD_01/GBRD_01[]` | 66 字符内容 + `@` |
| `8` | `KICC_01/GICC_01[]` | 14 字符头 + 有序嵌套 `0/9` 表 + `@@` |
| `9` | IC 公司嵌套通信 KID | 固定 10 字符 |
| `A` | `KGIN_01` | 56 字符内容 + `@` |

## Upstream ABI 适配

Proxy 的 HTTP WIT API 1 强制每个 Component 导出上下行六个函数，而且绑定协议包后上下行都会进入
package pipeline。为保持用户确认的 downstream-only 业务范围：

- upstream 只验证并规范化 JSON Document，不解析 DLL 请求业务字段；
- 未修改时 Encode 返回原始请求；
- 任何 upstream Document 修改都会明确失败，防止包意外改写请求；
- Manifest 不声明 upstream Schema。

## Listener 配置

Payment 请求和响应都是 Shift-JIS JSON。绑定此包的 HTTP Listener 应显式配置：

- Request Body Codec：`Shift-JIS`
- Response Body Codec：`Shift-JIS`
- Content-Encoding：identity

字符集转换属于 Proxy transport；Component 输入输出是 WIT `string`。`Length` 按 Android 当前
`String.length()` 行为对应的可表示字符数计算，不按 Shift-JIS 字节数计算。

## Fixture 来源

- `tests/fixtures/d48.json`：派生自 `TASK-20260903-001/PAYMENT-DLL-D48-MOCK-001` 实际抓包。
- `tests/fixtures/credit-success.json`：完整 Credit 成功响应快照，SHA-256
  `3b3d5ef13317145a404d718fc72aa77c38a921b06fa904ac4ac6376ba3958fc2`。
- `tests/fixtures/union-pay-success.json`：由 Android 当前 `CreditDLLTest` 中的 UnionPay 响应字段生成的
  最小完整 `A/2/3/5` 表用例；共享 CA、Brand、IC 嵌套结构由 Credit 完整样本覆盖。
- `tests/fixtures/connection-test.json`：依据 Android `ConnectionTest.Response` 字段构造的确定性用例。

## 构建与测试

从仓库根目录运行：

```bash
cargo test --locked --all-targets \
  --manifest-path examples/protocol-packages/gmofg_payment_dll/Cargo.toml

deno run -A examples/protocol-packages/gmofg_payment_dll/build.mjs
```

构建器会运行原生 Rust 测试、生成 `wasm32-wasip2` Component、嵌入唯一
`intercept-proxy:manifest` section，并输出：

```text
examples/protocol-packages/gmofg_payment_dll/dist/gmofg-payment-dll-1.0.0.wasm
examples/protocol-packages/gmofg_payment_dll/dist/gmofg-payment-dll-1.0.0.wasm.sha256
```

把 `.wasm` 文件导入 Proxy 后即可在 HTTP Listener 的协议包配置中选择
`gmofg-payment-dll@1.0.0`。
