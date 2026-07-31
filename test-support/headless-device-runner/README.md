# Headless Android 实机规则验证

这是通过真实 Android 设备、真实双向 mTLS 和真实 GMO-FG DLL 上游执行的 integration/E2E 测试入口，不是单元测试。入口直接构造 Rust `ApplicationHost/Application`，不启动 Tauri 或 WebView。

## 前置条件

- macOS 已完成 Proxy 证书和上游身份配置，正式桌面 App 与其他占用 `16127`/`16627` 的进程已停止。
- Android 设备已通过 `adb` 连接；脚本默认构建、安装并在结束时卸载 instrumentation APK。
- 设备可以访问 Proxy 地址，Proxy 证书 SAN 覆盖该地址。
- 调用环境可以读取现有 Proxy CA DER 与客户端 PKCS12。
- 脚本仅通过 stdin 将 CA、PKCS12 和一次性读取的 Payment 设置写入测试包私有
  `filesDir`（0600），不放入 ADB 命令行或证据日志，并在退出时精确删除。
- 本机命令可用：`cargo`、`adb`、`jq`、`rg`、`sqlite3`、`lsof` 和 `shasum`。

## 必需环境变量

- `GMOFG_PROXY_CA_DER`：现有 Proxy CA DER 文件路径。
- `GMOFG_CLIENT_P12`：现有客户端 PKCS12 文件路径。
可选变量：

- `GMOFG_DEVICE_SERIAL`：ADB 序列号，默认使用当前项目实机。
- `GMOFG_PROXY_URL`：设备访问的 DLL Proxy URL。
- `GMOFG_APP_DATA_DIR`：桌面 App 数据目录。
- `GMOFG_EVIDENCE_ROOT`：证据输出目录；未指定时创建独立临时目录。

所有敏感值只从调用环境和指定文件读取，不写入源码、README 或 runner 日志。

## 运行

```bash
export GMOFG_PROXY_CA_DER=/secure/path/proxy-ca.der
export GMOFG_CLIENT_P12=/secure/path/client.p12
./test-support/headless-device-runner/run-real-device-scenarios.sh
```

## 场景与判定

`scenarios.json` 是机器可读矩阵。`GMOFG_BATCH=A|B|C|D|E|F|G|ALL` 选择批次，
`GMOFG_SCENARIO=<id>` 可定向复测。每项同时要求 Android 结果、Rust
hit count/规则轨迹、动作效果以及创建 rule ID 清理为 0；批次前后都执行
D48 baseline。

- A：请求/响应修改、Header、延迟、HTTP 状态、Mock 与非法 JSON。
- B：TLS 拒绝、断连、三类上游超时、两类丢弃、错误 Content-Length 与截断。
- C：请求/响应断点，runner 自动执行 `ForwardOriginal` 并证明 queued → resolved。
- D：NthHit（三次序列）、OneShot、优先级、非终止动作组合、延迟 + Mock。
- E：四类匹配字段、Equals/Contains/Regex、正反例、AND 与失败轨迹。
- F：九类非法配置必须由 Rust 拒绝，随后真实 DLL D48 baseline 仍可用。
- G：上下行限速、抖动、间歇通断保持 Android D48，并验证规则动作与耗时；
  间歇通断会组合一个只用于分块的限速动作，确保小型 DLL 报文真实跨过阻塞
  窗口。上行 Body 中途断连必须产生 `IOException`，下行不完整 Body 必须产生
  `ProtocolException`，批次后再以 D48 baseline 证明链路恢复。

`request-delay` 和 `response-delay` 会各自在紧邻场景前执行一次 D48 baseline，
除了绝对耗时外还校验差值。`ALL` 会在一个证据包中执行全部批次，并生成
`batch_summaries`；模板清单必须与矩阵中的 22 个模板完全一致。

TLS 拒绝场景以 Proxy 的 TLS 阶段 `RuleHit`、HTTP 处理器未进入和 Android
连接失败共同验收。Android/OkHttp 在服务端拒绝客户端证书时会按平台 TLS
实现暴露 `IOException`，矩阵不要求不稳定的 `SSLHandshakeException` 子类。

每个场景保存 runner、instrumentation 与过滤后的设备日志，并生成
`results.jsonl` 与 `report.json`。`security-checks.txt` 记录私有材料权限和
instrumentation 参数安全检查，不包含秘密值。报告同时绑定设备序列号、
Proxy host:port、源码内容摘要、runner/APK SHA-256，并记录 Android 实测耗时。

## 状态保护与清理

- 如果发现名称不以 `headless-device-` 开头的已启用规则，runner 会 fail closed，不启动测试，也不修改用户规则。
- 启动时只清理 `headless-device-` 前缀的遗留测试规则。
- 每个场景结束时只删除本场景创建的 `rule_id`。
- 超时或中断会先停止 Proxy、清理本场景规则，再返回失败。
- runner 不保存会话 Payload；测试结束必须显示 `remaining_test_rules=0`。
- 最终报告要求测试包已卸载、两个监听端口均释放，SQLite 中
  `headless-device-` 与故障模板测试规则计数均为 0。
