# CPS checksum 本地完整验证

任务 TASK-20260907-004，用例 cps-checksum。结果：**PASS_LOCAL_CPS_JAVA_HOST_HTTP_REVIEW**。

## 目的、输入和实际结果

验证实际 gmofg-cps@1.0.0 Component 的上下行 Decode / Display / Encode、四类 CPS
响应签名和真实 HTTP Decode → Rule → Encode → Finalize → App 链路。
输入为源码模型派生的合成 pay/confirm/refund/reverse JSON，无真实商户或交易。
Java 14 向量逐条比较 canonical 和 MD5；原生 8 测试及 1 doctest、实际 Host、真实 HTTP
wire、严格 Clippy、格式/lint和架构门禁均通过，详细命令/起止时间/退出码见
[执行记录](outputs/timed-runs.json)，整体审查见 [review](outputs/review.md)。

金额 100 → 250，原请求 token 保持，App 最终签名
F070F340438D40D0BC07295E241EE519 与独立 Java 预期一致。HTTP 原始帧、配置、规则、
解析结果、逐字段比较与连接清理见 outputs/wire；Host 全向量输入输出和编辑链见
outputs/logs/host.log。异常业务输入完整返回，非法五字段封装明确报错。

## 环境、资源和复测

- 当前被测文件 hash：inputs/tested-source-sha256.json；实际版本/平台：inputs/environment.json。
- 包源码、Cargo.lock、Manifest、WIT、测试、合成fixture和Java oracle：resources/source 下对应仓库路径。
- 原 CPS 源码与精确 Gson JAR：resources/cps-source、resources/gson-2.9.0.jar，来源/用途/SHA见 resources/provenance.json。
- 实际测试的 Wasm 和摘要：outputs/gmofg-cps-1.0.0.wasm 及 .sha256。
- 父合同 TASK-20260907-001 / http-finalize 位于 ../../TASK-20260907-001/http-finalize，复用通用Host/WIT/HTTP链，本次增加真实CPS模型与签名预期，未改父档案。

从包含 HTTP Finalize 的同版本仓库建立可丢弃副本，按 resources/source 原路径恢复本次文件。
安装 Rust wasm32-wasip2 target、Deno、Android Studio JBR；Java可通过 JBR_HOME 指定等价JDK。
脚本使用归档 CPS 源码和 Gson，不依赖原 Android checkout 或 Gradle缓存。
运行 `python3 <本目录>/replay/run.py <仓库副本根目录>`，它先核对源文件hash，再依次构建、
运行Java/Host/HTTP及静态检查；任一步失败即停止。日志和新wire证据写到新的临时目录，不改本档案。
HTTP测试使用127.0.0.1临时端口，自建TCP上游与Listener，测试结束停止Listener、释放端口；
无证书、外部服务、设备、系统代理或数据库变更，无额外恢复操作。

TLS/mTLS、真实CPS服务、Android终端支付结果、运行中App UI为 NOT_RUN；数据库迁移为N/A。
用户明确排除1e2、1E+2、-0数字词法输入；初审差异未修复，复审按该支持范围给出结论。
HTML安全性在包测试中验证，不代替桌面UI验收；不存在虚构截图或报文占位文件。
