# TASK-20260907-004 CPS 独立只读对抗复审

## 结论

**APPROVE**

- P0: 0
- P1: 0
- P2: 0

审查对象为 Proxy HEAD `561fd57a3875629c25de7b984f3f36a1b46d2511` 中的：

- `examples/protocol-packages/gmofg_cps/**`（排除 `target`）
- `src-tauri/crates/package-runtime/tests/gmofg_cps_component.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/phase10_http_pipeline/cps_checksum_wire.rs` 及父模块登记
- `docs/tasks/pending/2026-09-07/implement-cps-checksum-wasm.md`
- `examples/protocol-packages/gmofg_cps/README.md`

源合同为 `/Users/codin/Code/jp_gmofg_cps` HEAD `db99af6fde2de3f8ae8f29777e19aa45d920c3f9`。

## 范围变更与初审 finding 处置

初审报告保存在 `/tmp/task0907-cps-review-initial.md`。初审曾对 Java `String` 模型字段接收特殊 JSON number 词法时的差异提出 P1，实际复现输入包括 `1e2`、`1E+2` 和 `-0`。

用户随后明确这些写法不属于真实环境输入，本次合同不考虑该边界。当前生产代码没有实施 RawValue 或其他相关修复；本复审也不把该 finding 描述为已修复。它因用户明确缩小输入合同而转为**范围外的已确认限制**，不再构成本次交付的未解决缺陷。

若未来需要接受 Java `String` 模型字段中的上述 number 词法，应重新登记需求并恢复初审报告中的双侧复现与回归要求。初审复现仍保留在：

- `/tmp/task0907-cps-review-repro-java.log`
- `/tmp/task0907-cps-review-repro-rust.log`
- `/tmp/task0907-cps-review-repro/`

## 复审结果

在缩小后的输入合同内，没有发现其他未解决 P0、P1 或 P2。

- 原 token 与路由：16 条源路由与 `TargetEnv.java` 四套环境路径一致；URL path 百分号解码一次，保留补空格、字面 `+` 和 `%25` 的单次解码语义，不以规则修改后的金额重建 token。
- 四响应模型：Pay、Confirm、Refund、Reverse 的 meta/data/result 具体模型字段、继承字段、排序、未知字段排除、`sign` 排除、null/int 0/空字符串、ASCII 空格、UTF-8 MD5 和大写 hex 与源算法及当前 Java oracle 一致。
- 数值与类型：当前约定的普通整数、整数字符串、`1.0` 整数转换、boolean/number 到 String 的已覆盖普通标量、无效对象/数组/非整数均有 Java/Rust 对照或不适用路径验证。用户明确排除的特殊数字词法不在此结论内。
- D6：未知路径、非 POST、非 2xx、缺 token/sign、非 JSON body、必要模型结构或类型不适用时，Finalize 在修改前完成判断并完整返回输入；五字段 HTTP wrapper 损坏、非法 Document 和 Wasm 执行错误仍明确报错。
- Codec 与字段保留：有效 JSON/未知字段完整进入 Document；未修改 Document 保留原 body 字节；Host f64 no-op 有实际 Host 断言；修改后按 Encode → Finalize 顺序重签；Finalize 仅改变 `data.sign` 的语义值。
- 导出与包：上游 Decode/Display/Encode、下游 Decode/Display/Encode/Finalize 均由实际 Component Host 测试加载执行；manifest、内嵌 manifest、dist Component 和 SHA 文件一致。
- HTML：Display 对 `& < > " '` 转义，原文和 JSON 展示未发现可执行 HTML 注入。
- Wire：真实 TCP HTTP 测试证明请求 token/path/body 到 origin 保持，响应 amount 100→250 后使用 Java `pay-after` 签名，App 收到完整响应；Content-Length、path-id、四帧与两组实际 socket read chunks 比较均 PASS。
- 文档：TASK 和包 README 与当前实现、测试、限制及未覆盖的真实服务/TLS/mTLS/终端业务验收边界一致。

## 证据

- `/tmp/task0907-cps-tested-state.json`：21 个受审文件当前 SHA-256 全部匹配，复审时 `mismatches=0`。
- `/tmp/task0907-cps-final-build.log`：包原生 8/8 PASS，release Component 构建成功。
- `/tmp/task0907-cps-final-java.log`：Java oracle 14/14 PASS。
- `/tmp/task0907-cps-final-host.log`：实际 Component Host 测试 PASS。
- `/tmp/task0907-cps-final-wire.log` 与 `/tmp/task0907-cps-final-wire/comparison.json`：真实 HTTP wire 测试、四帧和 TCP chunks 比较 PASS。
- `/tmp/task0907-cps-final-{clippy,host-clippy,infrastructure-clippy,architecture,source-size}.log`：静态检查 PASS。

本轮只读复审没有修改生产代码，也没有运行会干扰共享状态的 Cargo 大测试。
