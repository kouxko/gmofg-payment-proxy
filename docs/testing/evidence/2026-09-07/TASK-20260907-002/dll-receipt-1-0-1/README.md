# DLL / 电子小票 1.0.1 验证

任务：TASK-20260907-002；用例：dll-receipt-1-0-1；日期：2026-09-07（Asia/Shanghai）。
目的：让同一 DLL Host 上的 U010/U011 响应使用独立小票模型，保留原下载协议和错误码。

## 输入、实际结果和层级

| 验证 | 输入与预期 | 实际结果 | 证据 |
| --- | --- | --- | --- |
| 原合同基线 | Credit/UnionPay/连接测试/D48 及旧非法结构、树形展示 | 14/14 PASS | outputs/baseline-tests.log |
| 修复前小票 | 合成五字段小票，预期正常解析 | 两项失败，unknown field ErrorMessage；负向测试PASS | outputs/red-tests.log |
| 最终包 | 原14项+四项小票测试；含报告19条logged JSON与合成成功/失败/修改/非法/重复字段 | 18/18 PASS | outputs/build.log |
| Wasm构建 | wasm32-wasip2，唯一Manifest，1.0.1 | PASS，552335 bytes | outputs/build.log、outputs/*.wasm |
| 实际Host加载 | 精确1.0.1，D48/Credit原回归+U010/U011成功失败及修改、HTML、非法输入 | 1/1 PASS | outputs/host-tests.log |
| Shift_JIS边界 | 既有Host codec读取原始UTF-8/Shift_JIS字节 | 1/1 PASS | outputs/body-codec-test.log |
| 静态检查 | 包/Host Clippy -D warnings、fmt、Deno check、diff whitespace | PASS | outputs/*clippy.log、*fmt.log、deno-check.log、static-checks.log |

失败业务码不会转换为解码失败，空格保持原样。新的小票模型没有ResponseTimeStamp，未削弱DLL模型必填或表结构校验。
Host实际输出8个向量已展开至 outputs/host-vectors/，包含输入、Document、编码输出、再次解码和HTML；
对应 .bin/.hex 是用 Python Shift_JIS 编码这些实际 Unicode 字符串生成的比较材料，非服务器抓包。
四条未修改输入/输出字节相等；四条修改输出解析字段等于目标Document。
严格Host codec测试与Wasm测试分开执行，没有将它们声称为一次真实Listener完整串联验收。

## 资源来源与复用

- resources/intercept-proxy-reproduction.md：用户提供原文件完整复制，用于根因及19条日志Body提取；原路径为企业微信缓存中的同名文件。
- resources/package/tests/fixtures/receipt-observed-log.json：报告提取的19条对象；原文日文已经有损，仅证明日志结构可解析。
- resources/package/tests/fixtures/receipt-{credit,union-pay}-{success,failure}.json：按Payment模型合成；日文和E01不是服务器原文或已确认错误码语义。
- Host内联合成四个样本：完整保存在 resources/gmofg_payment_dll_component.rs 和 outputs/host-vectors；E01/E02同样是测试值。
- resources/payment/：当前Payment的JsonReceipt模型、Converter、请求类型及发送流程源码快照，仅作合同依据。
- resources/package/：实际包源码、测试、fixtures、构建及Manifest快照；所有自动化回归使用活动目录，快照用于历史核对。
- inputs/environment.json、inputs/tested-files-sha256.json：工具版本、HEAD、系统、最终被测文件及产物哈希。
- outputs/gmofg-payment-dll-1.0.1.wasm 与 .sha256：实际构建且由Host加载验证的新产物副本。

父任务 TASK-20260903-005，父用例 gmofg-dll-downstream-package，父证据
[原DLL证据](../../../2026-09-03/TASK-20260903-005/gmofg-dll-downstream-package/README.md)。
复用旧fixture预期（表长度、保留区、错误码、未修改原文）；新增小票模型不替代旧合同。

## 限制与复测

[命令及成功判定](replay/commands.md)。完整报告、fixtures、输入和输出均已保存。
UI截图/真实Server报文/TCP chunks/证书/设备及业务结果：N/A，本次仅协议包与Host本地验证，未运行真实外部链路。
原始日本语无法从有损日志还原，不声称复现原142字节Body。Windows加载包无哈希，新产物未自动安装到该机器。
CI、Windows/Android端到端：NOT_RUN。旧1.0.0哈希保持98f8a15009f52dda74a05c40b802dbe0f1ff0027294da76d67d1511e57df6074。
整体独立审查结果另存 outputs/review.md；以最终任务档案为准。
