# TASK-20260907-004 CPS 独立只读对抗审查

## 结论

**REQUEST CHANGES**

- P0: 0
- P1: 1
- P2: 0

审查对象为当前稳定源码（Proxy HEAD `561fd57a3875629c25de7b984f3f36a1b46d2511`）中 `examples/protocol-packages/gmofg_cps/**`（排除 `target`）、Host 组件测试、真实 HTTP wire 测试及父模块登记、TASK-20260907-004 pending 文档和包 README。源合同为 `/Users/codin/Code/jp_gmofg_cps` HEAD `db99af6fde2de3f8ae8f29777e19aa45d920c3f9`。

## Findings

### P1 — Gson `String` 字段的合法数字词法被 Rust 规范化，生成的响应签名与 CPS App 不同

**位置**

- `examples/protocol-packages/gmofg_cps/src/signing.rs:62-80`
- `examples/protocol-packages/gmofg_cps/Cargo.toml:10-13`
- 对照源：`/Users/codin/Code/jp_gmofg_cps/app/src/main/java/jp/gmofg/cps/https/msg/Meta.java:16-18`
- 对照算法：`/Users/codin/Code/jp_gmofg_cps/app/src/main/java/jp/gmofg/cps/https/msg/Message.java:343-370`
- 当前 oracle 覆盖：`examples/protocol-packages/gmofg_cps/tests/java_reference/OracleMain.suffix:20-25`

**问题**

CPS 的 `meta.code`、`meta.message`、`data.errorCode`、`data.errorInfo`、`data.subErrorCode` 和多个 result 字段在 Java 模型中是 `String`。Gson 2.9.0 接受这些字段的 JSON number/boolean 标量；对于 number，`JsonReader.nextString()` 保留原 JSON 数字词法，随后 `Message.getSortedNameValuePair()` 使用该 `String.toString()` 参与 MD5。

当前 Rust 先把完整 body 解析为 `serde_json::Value`，再由 `java_string()` 对 `Value::Number` 调用 `to_string()`。即使启用 `arbitrary_precision`，serde_json 仍会规范化部分合法数字词法：`1e2` 变为 `1e+2`、`1E+2` 变为 `1e+2`、`-0` 变为 `0`。因此字段集合与排序虽正确，MD5 输入字节仍与 App 不同。

**实际复现**

输入 operation=`confirm`、token=`TOKEN`、body：

```json
{"meta":{"code":1e2},"data":{"sign":"OLD","result":{}}}
```

使用经 SHA 门禁的 CPS `Message.java:305-474` 原算法和 Gson 2.9.0：

```text
canonical={data={result={}}&meta={code=1e2}&key=B2BTOKEN}
sign=8DADD417DB0FC64F6414A64B9333565E
```

使用当前 `signing.rs` 与本包启用 `arbitrary_precision` 的 serde_json：

```text
canonical={data={result={}}&meta={code=1e+2}&key=B2BTOKEN}
sign=AECFD8E0179E670CC73B20784034C993
```

另外两组同样失败：

| 输入 `meta.code` | Java canonical 值 / sign | Rust canonical 值 / sign |
| --- | --- | --- |
| `1E+2` | `1E+2` / `C3FFD8425A4EE30729C2462E208D38A3` | `1e+2` / `AECFD8E0179E670CC73B20784034C993` |
| `-0` | `-0` / `866CCEDE7816EA696D9523B8EA3B0715` | `0` / `981408E3A88E80CB004FA2E974088CD4` |

复现输出保存在：

- `/tmp/task0907-cps-review-repro-java.log`
- `/tmp/task0907-cps-review-repro-rust.log`
- `/tmp/task0907-cps-review-repro/`

**影响**

对于已识别路径、POST、2xx、非空 `data.sign` 的响应，只要任一 Java `String` 模型字段以这类合法 JSON number 形式出现，Proxy 会写入与 CPS App `checkSign()` 计算结果不同的签名，App 将其判为验签失败（源 `BaseNetUtil.java:105-120`，业务侧返回 `E09116`）。这违反 TASK 文档 `implement-cps-checksum-wasm.md:66,71` 与包 README `README.md:66-69` 声明的 Gson 模型投影/number 标量转换合同。

**所需修改与回归**

在签名投影阶段保留最终待发送 body 中、Java `String` 模型字段的原始 JSON number 词法；不能仅依赖 `serde_json::Number::to_string()`。修复后至少把 `1e2`、`1E+2`、`-0` 加入 Java oracle fixture，并增加 Rust 原生与实际 Component Finalize 对照测试。签名必须基于 Encode 后的最终 body；若 Encode 已把数字规范化，则以实际最终 body 词法为准。

## 已确认通过的范围

- 原 token：16 条源路由与 `TargetEnv.java:8-14` 一致；URL path 只百分号解码一次，空格、字面 `+` 和 `%25` 语义有测试。
- 四响应模型：字段名单、继承字段、排序、未知字段排除、`sign` 排除、null/int 0/空字符串、ASCII 空格、UTF-8 MD5 和大写 hex 与源算法一致；除上述 number-to-String 词法缺口。
- D6：业务不适用条件在修改前完成，并完整返回原 Finalize 输入；五字段 HTTP wrapper 损坏、非法 Document 等执行错误仍显式报错。
- Codec：完整 JSON/未知字段保留；未修改 body 原字节保留；Host f64 no-op 有 Host 断言；修改后 Encode 再 Finalize；HTML 对 `& < > " '` 全部转义。
- Component：上游 Decode/Display/Encode、下游 Decode/Display/Encode/Finalize 均实现；manifest、内嵌 manifest 和 dist SHA 一致。
- Wire：读取 `/tmp/task0907-cps-final-wire/comparison.json`，完整四帧、origin request/App response 实际 read chunks、Content-Length、path-id、金额 100→250、Java pay-after sign 均为 PASS。
- 文档：TASK、包 README 与当前实现/测试边界总体一致；已明确真实 CPS 服务、TLS/mTLS、真实终端业务验收未执行。

## 验证证据

- `/tmp/task0907-cps-tested-state.json`：21 个受审文件当前 SHA-256 全部匹配，mismatch=0。
- `/tmp/task0907-cps-final-build.log`：包原生 8/8 PASS，release Component 构建成功。
- `/tmp/task0907-cps-final-java.log`：现有 Java oracle 14/14 PASS。
- `/tmp/task0907-cps-final-host.log`：实际 Component Host 测试 PASS。
- `/tmp/task0907-cps-final-wire.log` 与 `/tmp/task0907-cps-final-wire/comparison.json`：真实 HTTP wire 测试与证据比较 PASS。
- `/tmp/task0907-cps-final-{clippy,host-clippy,infrastructure-clippy,architecture,source-size}.log`：静态检查 PASS。

现有 PASS 证明常规 CPS 向量和实际 wire 主路径正确，但没有覆盖本 finding 的数字词法输入，因此不能消除该 P1。
