# FINAL-REPLAY-001

## 目的

在当前冻结实现上重新执行可用的归档场景、完整本地门禁和真实隔离 App 控制面流程，并明确记录
无法在本机执行的真实设备或授权业务链路。历史结果只用于找到资源和入口，本目录结论均来自本次执行。

## 环境与范围

- 时间：2026-08-27（Asia/Shanghai）
- 平台：macOS arm64
- 隔离 App identifier：`com.interceptproxy.desktop.finalreplay`
- MCP：production IPv4/IPv6 wildcard Listener，端口 `17653`
- Android：ADB 可用，但没有已连接设备
- 业务数据：未读取正式 App 数据目录，未发送生产交易

## 执行步骤

1. 重新执行文档/架构/源码规模、前端、Rust、Windows 静态编译与外部包门禁。
2. 重新执行 HTTP、Socket、TLS、mTLS、协议包、Android 多设备所有权和环境候选回归。
3. 使用归档资源验证 First Data 证书链并对远端只做零业务字节 TLS 握手。
4. 构建隔离 macOS App，启动后调用 MCP resource list/read，读取验证与排障指南。
5. 对完整环境候选执行 create、apply、status；确认 `preview_ready`、`apply_queued`、`committed`。
6. 关闭 App，确认端口释放；重启同一 App，再次调用 capabilities；结束后再次确认端口释放。

## 结果

- 本地适用门禁：PASS；生产非回环 IPv4 MCP 实调因本机 Proton VPN 透明代理截流保持 `NOT_RUN`，未将超时算作成功。
- 前端：67 个测试文件、659 项测试 PASS。
- Rust workspace：除上述单一本机网络环境用例外，其余全部测试套件 PASS；核心计数见 `outputs/local-validation.json`。
- MCP 定向：71/71 PASS；完整候选真实隔离 App 流程 PASS。
- 数据面：Runtime 178 个单元测试及 47 个集成场景 PASS，覆盖真实 loopback HTTP、Socket、TLS、mTLS
  与端口释放。
- 外部包：Nuvei Tango Python 14/14、AU EFTEX 72/72、Deno ISO 14/14、Nuvei Tango Rhai 6/6 PASS。
- 远端证书：TCP 可达，TLS 1.3 握手和证书链验证 PASS，未发送业务数据。
- Android 真机 A/B 并行：NOT_RUN；本机没有连接设备。
- Nuvei Tango 授权真实交易：NOT_RUN；没有授权端点与测试交易窗口。
- MCP 生产非回环 IPv4 实调：NOT_RUN；本机透明代理使同机 LAN 地址流量在连接后无响应，严格 10 秒期限按失败停止。
- 远程 Windows 构建：PENDING；在最终代码交付后执行。

## 安全与不适用项

- confirmation token 只在进程内传递并已消费，本目录不保存其值。
- 无私钥、密码、生产交易、完整敏感报文或正式 App 数据。
- 真机与授权交易用例不得用当前自动化结果替代；复测入口见场景清单。

结构化结论见 [local-validation.json](outputs/local-validation.json) 和
[scenario-results.json](outputs/scenario-results.json)。复测命令见 [replay.md](replay/replay.md)。
