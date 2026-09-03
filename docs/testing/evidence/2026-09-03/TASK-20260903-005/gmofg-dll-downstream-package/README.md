# GMO-FG Payment DLL downstream 协议包验证

- 任务：`TASK-20260903-005`
- 用例：`gmofg-dll-downstream-package`
- 执行时间：2026-09-03 16:08:06 +08:00
- 环境：macOS arm64，Rust/Cargo 1.98.0，Deno 2.9.6
- 被测状态：分支 `codex/intercept-proxy-generalization`，基线提交
  `3586ea2ca2dbe1f56ea0ad472190947594573505` 上的本任务未提交修改；验证期间停止修改被测文件。
- 结果：`PASS`（真实 GMO-FG Server 与 Android 设备联机为 `NOT_RUN`）。

## 资源

- `resources/fixtures/d48.json`：历史真实 downstream D48 响应快照。
- `resources/fixtures/credit-success.json`：完整 Credit 成功报文，覆盖全部 Credit 外层表及 IC 嵌套表。
- `resources/fixtures/union-pay-success.json`：UnionPay `A/2/3/5` 表响应。
- `resources/fixtures/connection-test.json`：连接测试 `0000` 响应。
- `resources/manifest.json`：本次实际嵌入 Component 的 Manifest。
- `outputs/gmofg-payment-dll-1.0.0.wasm`：Host 实际加载的单文件 Component，547533 bytes。
- `outputs/gmofg-payment-dll-1.0.0.wasm.sha256`：产物校验值。

上述资源均从当前活动 fixture/构建产物复制；原始文件名未修改。复测入口见
`replay/commands.md`。

## 验证结果

1. 包级 13/13 测试通过：`0000`、`0001`、`0002`、D48 Decode；Credit 共 13 个卡公司、62 个
   卡号范围、6 个 CA 组、32 个 CA Key、6 个品牌、10 个 IC 公司、25 个风险表、10 个通信 KID；
   保留区和空白阈值保留。
2. 未修改 Document 的 D48、Credit、UnionPay 和连接测试均逐字符 Encode 回原 Body。
3. 修改 Card、Merchant、Batch、Terminal、CA、Brand、IC、Risk、Communication KID 和 UnionPay
   代表字段后均从 Document 重建；增加一个 Card Range 后 `KCCI_01.Length` 从 2478 自动变为 2510，
   再次 Decode 得到新增记录。
4. 长度不一致、错误宽度、CA 数量不一致、未知嵌套表 ID、错误外层表 ID、未知 JSON 字段和交易类型
   与表组合不匹配均 fail-closed。
5. Display 输出语义 `<table>/<thead>/<tbody>`，保留全部嵌套字段和保留区，HTML 特殊字符被转义，
   不包含 JSON `<pre>`；各 table 获得不同且确定的安全配色。
6. 完整 Credit Display 经真实 Wasmtime Host 调用后为 68424 bytes，保守 DOM 节点计数 4107；低于
   用户确认的 Proxy 1 MiB / 8192 节点限制。
7. Proxy 安全渲染 11/11 测试通过：512 KiB 和约 4500 节点输入可渲染；超过 1 MiB 或 8192 节点
   仍被拒绝；元素/样式白名单、CSP、sandbox、深度限制和不同 table 配色清洗保持有效。
8. 包与 Host 测试的 strict Clippy、Rust format、TypeScript typecheck、目标 ESLint、Deno 构建脚本检查、
   Component release build、嵌入 Manifest 校验、Host 加载/调用和 `git diff --check` 全部通过。

## N/A 与未执行项

- TCP chunks / HTTP Header / Shift-JIS 原始字节：N/A。本 Component 合同输入为 Host 已按 Listener
  Body Codec 解码后的 WIT `string`；字符集转换没有在本任务中修改。
- Frame：N/A。包类型为 HTTP，WIT 不包含 Socket Frame。
- 规则处理：N/A。本任务不创建或修改规则。
- 真实 GMO-FG Server / Android 设备联机：`NOT_RUN`。本次无新的成功 Server 抓包和设备环境，不能
  用本地 fixture/Host PASS 代替真实业务验收。
