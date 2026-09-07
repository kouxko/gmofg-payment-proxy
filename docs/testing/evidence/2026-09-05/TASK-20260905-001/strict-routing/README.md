# STRICT-ROUTING-001：严格目标匹配与真实 SOCKS5 直连

- 任务：TASK-20260905-001；结果：PASS_WITH_ANDROID_DEVICE_NOT_RUN。
- 环境：macOS，Rust 1.98.0，localhost临时TCP端口；HEAD与实际文件SHA见metadata.json。
- 前置：仓库Cargo依赖可用，不需目标域名DNS或外部服务；两个TcpListener由用例创建并清理。
- 源文件在最终测试期间保持稳定；同工作区其他分片不拥有本用例文件。

## 输入、预期与实际

单元输入为唯一域名 https.gmo-fg.net:16127，桌面DNS快照153.150.240.65。
198.18.0.1、192.0.2.1与2001:db8::1均为未知目标，即使端口相同也不匹配；已解析IP、规范化域名继续命中；错误端口/其他域名不匹配。

网络用例配置configured.example，DNS快照192.0.2.1，端口等于localhost原始Server端口。
SOCKS5请求127.0.0.1:<动态端口>，发送PING（50 49 4e 47）。原始Server返回ORIG（4f 52 49 47），代理对照Server返回PROX（50 52 4f 58）。
旧实现实际返回PROX；删除端口补偿后原始Server收到完整PING，App端收到完整ORIG，逐字节断言通过。端口与实际接收日志见final.log。

## 证据与复测

1. baseline.log：旧实现旧合同8/8 PASS；resources/routing-tests-before.rs保留执行时旧用例。
2. red.log：新合同路由7 PASS / 1预期FAIL。
3. wire-red.log：真实SOCKS5实际PROX而非ORIG的预期FAIL。
4. final.log：最终Android Engine 48/48 PASS；包含域名、IP/CIDR、离线DNS、SOCKS5直连与已匹配代理回归。
5. resources保存实际用例与实现快照，metadata记录来源、用途、SHA与命令。历史端口补偿由864ef524引入，本次明确替代；旧证据不修改。

从仓库根目录执行：

```sh
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-android-engine --lib -- --nocapture
```

当前自动化使用模块旁活动测试；复现本次历史行为使用resources源码快照与metadata对应HEAD。测试服务器与SOCKS5任务在用例结束释放，未修改运行App配置。
TLS、截图与外部交易N/A；真实Android开机/数据面NOT_RUN，当前无ADB设备，不能将本结果当作真机验收。
