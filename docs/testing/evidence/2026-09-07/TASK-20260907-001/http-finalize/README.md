# HTTP Finalize 本地验收

任务：TASK-20260907-001；用例：http-finalize；执行日期：2026-09-07（Asia/Shanghai）。

结果：**LOCAL_PASS_REVIEW_APPROVED_WITH_EXISTING_FULL_TYPECHECK_FAILURE**。53 个不同的相关 Rust 测试通过；独立整体审查 APPROVE，0 findings。

## 目的与范围

验证 Proxy 本地 Wasm HTTP 两个独立可选 Finalize、既有 HTTP Pipeline 次序、五字段输入输出、真实 HTTP 写出、失败回滚与观测、旧包及 Schema100 兼容。CPS、远端 RPC Finalize、Socket Finalize、真实支付、应用安装和 CI 不在范围。

## 验收与实际结果

| 对象 | 结果 | 原始输出 |
| --- | --- | --- |
| Exchange 阶段与不写出 | 30/30 | [日志](outputs/logs/exchange-tests.log) |
| Wasm optional ABI/guest error/trap/错误签名和非函数导出 | 5/5 | [日志](outputs/logs/package-tests.log) |
| HTTP 五字段/Header/原子写回/顺序/旧库与真实 Wasm | 10/10，包含 Schema100 1 项 | [日志](outputs/logs/infrastructure-tests.log) |
| HTTP Mock、URI 与错误阶段 | 3/3 | [日志](outputs/logs/runtime-tests.log) |
| 运行时 replace/deactivate/取消 | 3/3 | [日志](outputs/logs/runtime-lifecycle.log) |
| 真实 Wasm→Listener→上游→App | 1/1 | [日志](outputs/logs/wire.log)、[原始收发与比较](outputs/wire/comparison.json) |
| 运行日志→观察存储 | 1/1 | [日志](outputs/logs/observation-test.log) |
| Strict Clippy、fmt、lint、架构、500行门禁 | PASS | outputs/logs/ 对应日志 |
| Rust 绑定生成、源码 TypeScript 检查 | PASS | [生成](outputs/logs/bindings.log)、[源码检查](outputs/logs/source-typecheck.log) |
| 全项目 TypeScript 检查 | FAIL：历史证据副本的 5 个 TS2307 | [原始失败](outputs/logs/typecheck.log) |

单独 schema100-test.log 是较早执行记录；最终有效 Schema100 验收在 infrastructure-tests.log 的 10 项中，不重复计数。bindings.log 是实际生成结果，曾包含并行开发期间警告；最终集成 strict Clippy 无警告。package-tests.log 的 guest trap 是明确预期的失败路径，测试整体通过。

## 实际 HTTP 交换

输入 POST /before?value=before，正文 `"before"`（8 bytes）。上游实际收到 POST /upstream?value=upstream、X-Value=upstream、正文 `"upstream"`（10 bytes）。origin 返回 200 和 `"before"`；App 实际收到 202 Accepted、X-Value=downstream、正文 `"downstream"`（12 bytes）。四份完整报文各有 .bin/.hex/.http；comparison.json 保存 SHA256、逐字段结果和 Content-Length 比较。connection.json 保存同一次测试的唯一 path_id、两个 TCP 段及清理结果，App 收到的是 HTTP 响应而非五字段包装。

## 资源与复测

- [环境和清理步骤](steps/environment-and-cleanup.md)、[可执行复测入口](replay/run.py)、[精确命令](replay/commands.json)。
- [实测源码 SHA256](inputs/tested-source-sha256.json)、resources/source 下同路径源码/测试/WIT/manifest/lockfile；resources/source-changes.patch 用于隔离 checkout 复现。
- [包来源、用途与 SHA256](resources/package-provenance.json)。五种 fixture 组件按 features 隔离 target；测试加载时嵌入归档 manifest。非函数导出由已归档测试源码在内存构造。
- 复用父任务 TASK-20260901-001、父用例 json-pretty-wasm-example 的旧 Component；本次验证新增 Host 可选 ABI 及 Schema100 重开，不修改父证据。原资源：[旧包](../../../2026-09-03/TASK-20260901-001/json-pretty-wasm-example/outputs/json-pretty-1.0.0.wasm)。
- [独立审查](outputs/overall-review.md)。所有 tests 的输入、expected 和断言保存在各自归档源文件；不是人工创建的占位向量。

## 限制

全量 typecheck 的 5 个缺失导入均来自既有 TASK-20260906-001 历史证据副本。复测脚本仅为源码检查生成临时 tsconfig，排除整个证据目录，未改变产品 tsconfig 或历史证据。pre-100 数据库迁移政策、Windows/Linux、真实设备、TLS 和业务验收未执行；不能由本地 PASS 推导。UI、证书资源和生产请求为 N/A（本次未使用）。
