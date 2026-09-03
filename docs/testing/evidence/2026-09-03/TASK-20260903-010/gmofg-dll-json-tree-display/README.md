# GMO-FG DLL JSON 树形 Display 验证

- 任务：`TASK-20260903-010`
- 用例：`gmofg-dll-json-tree-display`
- 执行时间：2026-09-03 16:35:01 至 16:44:34 +08:00
- 结果：`PASS_WITH_RUNNING_APP_VISUAL_NOT_RUN`
- 父任务：`TASK-20260903-005`
- 父用例：`gmofg-dll-downstream-package`
- 父证据：`docs/testing/evidence/2026-09-03/TASK-20260903-005/gmofg-dll-downstream-package/`

## 目的与被测状态

验证完整 Credit Document 不再以 `$` JSONPath 平铺，而是按真实 Object、Array、字段与数组下标生成
嵌套折叠树；每个容器均可独立展开/收起，标量 table、稳定分色、HTML 安全清洗、逐表横向滚动和
1 MiB / 8192 节点门禁保持有效。

- 基础提交：`c2fd81e9382a7adc36615ec7d64a03f267fc4605`
- 工作区：存在其他任务的并行修改；验收期间暂停本任务五个被测源码/测试文件的修改。
- Rust：`rustc 1.98.0`、`cargo 1.98.0`
- Deno：`2.9.6 aarch64-apple-darwin`
- 平台：macOS arm64，Asia/Shanghai。

被测文件 SHA-256 记录在 `metadata.json`；实际使用的 Credit fixture、Manifest 和最终 Wasm 已保存为
本目录资源与输出快照。

## 输入、步骤与实际结果

输入为 `resources/credit-success.json`，协议包首先完整 Decode 为 Document，再经 Wasmtime Host 调用
downstream Display。实际合同由自动化逐项读取生成 HTML 并断言：

- 根节点为 `<details open>`，summary 为“基本信息 / Object / 字段数”。
- `KCCI_01` 为 Array 节点并显示真实元素数量。
- 每个公司记录以 `[index] / Object` 出现，内部 `card_ranges` 继续作为 Array 子节点。
- 递归统计 Document 中全部 Object/Array 数量，与输出的 `details`、`summary` 和闭合标签数量完全相等。
- 只有根节点默认展开；所有子容器默认收起，用户可逐层展开。
- 输出不含 `>$</caption>` 或 `>$.`，但保留全部字段、数组下标、保留区和转义后的值。
- Proxy 清洗后保留两个测试 Disclosure 和根 `open`，删除注入的 `ontoggle=alert(1)`。
- 所有 table 继续获得不同且确定的 HSL 背景/边框，并保留独立横向滚动容器。

详细复测命令见 `replay/commands.md`，结构化结果见 `outputs/validation-summary.json`。

## 资源与输出

| 路径 | 来源 | 用途 | 必需 |
| --- | --- | --- | --- |
| `resources/credit-success.json` | 活动 fixture `examples/protocol-packages/gmofg_payment_dll/tests/fixtures/credit-success.json` | 完整嵌套 Credit Document | 是 |
| `resources/manifest.json` | 活动包 Manifest | 证明实际 Component 身份与 downstream Schema | 是 |
| `outputs/gmofg-payment-dll-1.0.0.wasm` | 本次 release build | 实际 Host 回放产物 | 是 |
| `outputs/gmofg-payment-dll-1.0.0.wasm.sha256` | 本次 build 输出 | 产物完整性复核 | 是 |
| `outputs/validation-summary.json` | 本次验收汇总 | 机器可读 PASS/NOT_RUN | 是 |

## 验证结果与未执行项

- 包级 Rust：14/14 PASS。
- Wasmtime Host：1/1 PASS。
- ProtocolSafeDisplay：13/13 PASS。
- Rust fmt/strict Clippy、目标 ESLint、TypeScript、Wasm build、Next production build、diff/锁文件检查：PASS。
- 运行中桌面 App 点击式展开/收起与截图：`NOT_RUN`，当前验收使用浏览器 DOM 回归和 production build。
- 真实 GMO-FG Server、Android 设备、CI：`N/A`；本任务只改变 Display 表现，未触及网络或设备数据面。

复测后如生成新产物，应重新比较活动 Wasm 与本目录快照，不能覆盖本次证据。
