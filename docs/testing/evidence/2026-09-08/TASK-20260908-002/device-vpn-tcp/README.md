# 远程设备覆盖安装与 VPN/TCP 对照

目标设备 A77/A920MAX，Android 10/API29，ADB 10.0.34.117:5555；Launcher jp.gmofg.app.launcher uid10116，TCP模式3，监听7788。客户端 Mac 10.0.34.43。仅零应用数据握手，不发送支付或其他业务报文。

## 已执行

1. 原 [QV-20260908-001](../../../../quick-validations/2026-09-08/QV-20260908-001/README.md) 记录旧 APK、活动 VPN 及 8秒超时；保持该 FINAL 档案字节不变。本任务新增对照不能改写 QV 状态。
2. 获取 Companion `status`（版本2，大端4字节长度JSON，临时 adb forward localabstract:intercept_proxy_vpn），设备返回 stopped，原因为控制租约5秒失联；系统无 tun0、无 IIF 规则。该停止发生在覆盖安装前。
3. 旧 APK / VPN 停止时连续三次7788握手成功，约8–20ms，见 outputs/before-install-vpn-stopped。
4. 从 pm path 返回路径原样备份旧 APK，核对固定签名；`adb -s 10.0.34.117:5555 install -r <new-companion.apk>` 成功。ADB 尝试增量安装不被此设备支持后按工具既有行为使用 streamed install；完整输出未删减。
5. 从设备新 pm path 拉回实际 APK，比对 SHA-256 完全一致：`8c8fe7048d29c5f85cae4a704fb2af9493ba710c581678bf67549a661b97cfd9`。UID10119 与 firstInstallTime 保持；未卸载或清除数据。配置私有内容未读取，不能声称已逐字段比较。
6. `adb shell am start -n com.interceptproxy.vpn/.AdbControlActivity --es command wake_control_server` 仅唤醒控制通道；确认新进程可返回状态。新 APK / VPN 停止时三次7788握手成功，约11–55ms，见 outputs/after-install-vpn-stopped。

## 待执行与复测

原桌面 Proxy 控制租约来自另一台电脑，本机无其完整 Profile/代理路由。必须在原 Proxy 用同一配置“应用修改”或“启动”；Companion 状态接口不提供完整配置，带临时代理路由的 Profile 不落盘自动恢复。禁止猜测端点或构造替代配置冒充原场景。

待 VPN 恢复后，从仓库根运行：

```bash
python3 docs/testing/evidence/2026-09-08/TASK-20260908-002/device-vpn-tcp/replay/tcp_check.py --adb /Users/codin/Library/Android/sdk/platform-tools/adb --expect-vpn running --out /tmp/vpn-local-tcp-new-run
```

输出目录必须不存在；脚本会保存前后控制状态、路由、PermissionMonitor、模式、监听以及三次握手的地址/时间/比较。活动 VPN、相同非空目标 Profile、目标 UID 默认路由仍入 tun0、UID IIF 限制消失与三次握手成功需要共同确认，不能仅使用 comparison.pass 宣称完整验收。
默认出站安全实流量/代理捕获仍待验证，应使用原配置下允许的安全请求及对应 Proxy 同连接证据；不把端口连通当业务成功。

资源来源、用途与原始名称见 resources/README.md。所有探测 socket 已关闭，临时 adb forward 已移除，未改变 Launcher TCP 模式；保留新 APK 属用户要求交付，不是待清理对象。VPN 保持安装前停止状态。
当前结果：安装/签名/哈希 PASS；新 APK 活动 VPN 下7788、默认出站、业务协议 NOT_RUN；不是完整修复验收。
