# Headless Android 实机规则验证

这是通过真实 Android 设备、真实双向 mTLS 和真实 GMO-FG DLL 上游执行的 integration/E2E 测试入口，不是单元测试。入口直接构造 Rust `ApplicationHost/Application`，不启动 Tauri 或 WebView。

## 前置条件

- macOS 已完成 Proxy 证书和上游身份配置，正式桌面 App 与其他占用 `16127`/`16627` 的进程已停止。
- Android 设备已通过 `adb` 连接，并已安装包含 `DllProxyRealDeviceTest` 的 instrumentation APK。
- 设备可以访问 Proxy 地址，Proxy 证书 SAN 覆盖该地址。
- 调用环境可以读取现有 Proxy CA DER 与客户端 PKCS12；脚本不会复制或输出其内容。
- 本机命令可用：`cargo`、`adb`、`rg`、`base64`、`awk` 和 `tr`。

## 必需环境变量

- `GMOFG_PROXY_CA_DER`：现有 Proxy CA DER 文件路径。
- `GMOFG_CLIENT_P12`：现有客户端 PKCS12 文件路径。
- `GMOFG_CREDIT_TID`：测试终端 ID。
- `GMOFG_CONFIRM_CODE`：测试确认码。
- `GMOFG_PAYMENT_PASSWORD`：测试密码。

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
export GMOFG_CREDIT_TID=...
export GMOFG_CONFIRM_CODE=...
export GMOFG_PAYMENT_PASSWORD=...
./test-support/headless-device-runner/run-real-device-scenarios.sh
```

## 场景与判定

- `baseline`：Android 用例成功，并记录 `DLL_PROXY_D48_CONFIRMED` 与 `errorCode=D48`。
- `custom-status`：Rust 响应阶段规则命中，Android 明确收到 HTTP 503。
- `invalid-json`：Rust 响应阶段规则命中，Android 明确报告响应无法解析为 `CreditDLL`。
- `delay`：Rust 注入 10 秒响应延迟；紧邻 baseline 与 delay 的 Android 测试时间差必须至少 8.5 秒，且 Rust 轨迹必须记录规则命中。
- 最后再次执行 `baseline`，确认清理后恢复 D48。

每个场景保存 runner、instrumentation 与设备日志，并检查 `HEADLESS_READY`、`HEADLESS_RESULT` 和 `HEADLESS_CLEAN`。

## 状态保护与清理

- 如果发现名称不以 `headless-device-` 开头的已启用规则，runner 会 fail closed，不启动测试，也不修改用户规则。
- 启动时只清理 `headless-device-` 前缀的遗留测试规则。
- 每个场景结束时只删除本场景创建的 `rule_id`。
- 超时或中断会先停止 Proxy、清理本场景规则，再返回失败。
- runner 不保存会话 Payload；测试结束必须显示 `remaining_test_rules=0`。
