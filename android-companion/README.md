# Intercept Proxy Android Companion

Android Companion 使用 `VpnService` 只接管 Profile 中明确选择的应用流量。未选择应用、
Android 系统流量和 ADB 不进入 TUN。

## 当前实现

- 包名固定为 `com.interceptproxy.vpn`。
- `VpnConsentActivity` 只调用系统 `VpnService.prepare()` 授权页。
- `InterceptVpnService` 负责前台通知、TUN、`addAllowedApplication()`、JNI 和 socket
  `protect()` 回调。
- 每次启动重新读取包名、当前签名和 UID；shared UID 必须选择完整应用组并显式确认。
- Companion 自身禁止进入允许列表；单个 Profile 最多 64 个包。
- Boot 后等待用户解锁和桌面控制端重新建立 USB/ADB 通道；5 分钟内失败 3 次会关闭自动恢复。
- 用户通过通知停止后保持停止。

## 数据面与安全边界

Android 工程已接入最小真实数据面：`TUN -> ImpairedTun -> tun2proxy -> 进程内
SOCKS5 -> 命中透明代理路由时进入 adb reverse -> 桌面 Listener`；未命中透明代理
路由的连接才通过 `VpnService.protect(fd) -> 原始目标`。Rust 动态库由 Gradle 在构建 APK 前
编译并装入四个 Android ABI 目录。每个 TCP/UDP 外连 socket 都必须在连接或发送前调用
`protect(fd)`；任何原生运行时、TUN、SOCKS5 或 protect 异常都会回调 Service 关闭 TUN，
恢复系统直连。

包级调度已经接入固定延迟/抖动、上下行限速、随机与连续丢包、重复、乱序、断网窗口、
DNS 黑洞、第 N 个 TCP 标志包、PMTU 黑洞、MSS Clamp 和 Payload 位翻转。IPv4 主动分片、
ICMP Fragmentation Needed 与 IPv6 Packet Too Big 的主动构造仍需在后续架构门禁中用真机
抓包完成验证，不能仅以 Rust 单元测试代替。

Payload 只存在于 TUN 读写缓冲区，不写入 SharedPreferences、文件或数据库。

### 无线网络不可用时的 USB/ADB 路径

- 透明代理路由默认且优先使用 `adb reverse`。设备只连接 `127.0.0.1:<临时端口>`，ADB
  负责把该端口送到电脑上的代理 Listener，因此设备无需知道电脑 LAN IP。
- TUN 使用 tun2proxy 的虚拟 DNS。Android 不再依赖当前 Wi-Fi/蜂窝网络提供 DNS；域名会在
  TUN 内保留，并由电脑侧解析后下发的地址快照共同完成匹配。
- 电脑仍需能够访问真实上游 Server。所谓“设备无网络”是指设备没有可用 Wi-Fi/蜂窝外网，
  不是电脑也断网。
- ADB 控制通道与业务数据通道相互独立：控制使用 `adb forward` 连接 Companion socket，
  业务连接使用 `adb reverse` 从设备进入电脑 Listener。

## 构建

```bash
./scripts/build-android-companion.sh
```

脚本会先构建并测试 release APK，再校验固定 signer、四 ABI 和 16 KiB 对齐，最后把同一个
APK 暂存为 Tauri 资源。release APK 位于：

```text
android-companion/app/build/outputs/apk/release/app-release.apk
```

### 固定升级签名

`android-companion/signing/` 保存项目固定 keystore、公开证书、配置和证书 SHA-256。这样
开发机与 CI 产出的 `com.interceptproxy.vpn` 能覆盖升级同一设备安装，而不会每次生成不同
身份。该身份随源码分发，只保证升级连续性，不作为保密或来源真实性边界。私有 keystore
不会进入 Tauri 资源；桌面包只携带已经签名并通过门禁的 APK。

本地或 CI 可以用同一个门禁脚本验证产物：

```bash
./scripts/verify-android-companion.sh \
  android-companion/app/build/outputs/apk/release/app-release.apk \
  --release \
  --expected-cert-sha256 "$(cat android-companion/signing/certificate-sha256.txt)"
```

Gradle 的 `preBuild` 会调用 `scripts/build-rust-android.sh`，分别为 `arm64-v8a`、
`armeabi-v7a`、`x86_64`、`x86` 交叉编译 `intercept-proxy-android-engine`，并把生成的
`libintercept_proxy_android_engine.so` 放入对应的 `app/src/main/jniLibs/<ABI>/`。脚本给
链接器设置 16 KiB 最大页大小；CI 仍需独立检查 APK 签名和 ELF page alignment。

初始 ADB 救援入口只接受 shell/root（Manifest 要求系统 `DUMP` 权限）：

```bash
adb shell am start -n com.interceptproxy.vpn/.AdbControlActivity \
  --es command stop
```

`configure_and_start` 还需通过 `profile_json` extra 传入完整 JSON。正式桌面控制应使用计划中
的 `adb forward localabstract:intercept_proxy_vpn` 长度前缀 JSON 协议；该通道已经实现
`start`、`apply`、`stop`、`emergency_restore` 和 `status`，并通过对端 UID 校验只接受
shell/root。救援 Activity 仍保留，但不作为最终流式控制通道。
