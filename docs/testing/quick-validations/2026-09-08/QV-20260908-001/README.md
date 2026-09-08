# QV-20260908-001 Launcher TCP 7788 观察性验证

- 用户问题：已经切换到tcp了你在检查看看。
- 生命周期：FINAL；结果：FAILED。
- 执行者：01a07fed-750e-7511-8509-fc0762cfbb31/root；时间：2026-09-08 16:05:34 +08:00。
- 目标：10.0.34.117:7788，path-001，直接 LAN TCP。
- 成功标准：8 秒内完成 TCP connect；不发送应用数据，不证明交易或应用协议正常。
- 当前环境：本机 en0 10.0.34.43，设备 Android 10 / Launcher 3.01.23；用户已切 TCP，7788 LISTEN UID 10116，VPN tun0 正在接管此 UID。
- 停止条件：一次握手结束或超时；若成功最多保持 2 秒后关闭。
- 清理：关闭探测 socket 和当次抓包进程，验证模式、监听及 VPN 保持一致。
- 资源：无用户文件；证书、DNS、TLS、应用报文 N/A（仅 TCP）。
- 本验证不修改设备模式、VPN、代码、Workspace 或规则，不触发 CI。
- 复测入口：replay/probe.py；实际输入与输出随后保存在 inputs/、outputs/。

## 分层结果

| path_id | 层级 | 结果 | 证据 | 边界 |
| --- | --- | --- | --- | --- |
| path-001 | DNS | N/A | 数值IP | 无域名解析 |
| path-001 | TCP | FAILED | [probe.json](outputs/probe.json) | 10.0.34.43:65064 → 10.0.34.117:7788，8秒超时 |
| path-001 | 应用协议 | NOT_RUN | 未发送应用数据 | 不证明业务结果 |
| path-001 | 清理 | VERIFIED | [after.json](outputs/after.json) | Socket已关闭、抓包进程已退出，TCP模式、监听inode和VPN保持一致 |

## 环境与执行证据

- [初始状态和工具版本](inputs/baseline.json)、[精确探测输入](inputs/probe.json)、[连接期间设备状态](outputs/during.json)、[VPN与相关日志](outputs/runtime-observations.json)。
- Launcher：3.01.23；Companion：1.0.0、versionCode 1、targetSdk 36；设备 Android 10/API29，PAX 2025-11-07 固件。
- 直接LAN路径，无Proxy Workspace/Listener修改。DNS、TLS、证书、SNI和mTLS为N/A，仅握手。
- 本机tcpdump无法打开/dev/bpf0；设备iptables与/sys/fs/bpf读取被权限拒绝，未改变权限。
- [设备BPF对象及来源](resources/README.md)、[NDK反汇编](outputs/netd-ndk-disassembly.txt)、[命令与警告](outputs/netd-ndk-disassembly-command.json)。Apple objdump不能识别BPF目标，NDK版本能输出指令但有BTF.ext调试元数据警告；不将其当运行抓包。

## 最终结论

- 整体：FAILED（TCP握手超时）。
- [事实、推断与未知](outputs/analysis.md)：当前VPN入站接口隔离是高置信度首要原因，未取得实际落包点的逐包证据。
- 未修改Launcher、VPN、系统设置或业务代码，未发送交易数据、未触发CI。
- 未创建正式实现任务；当前仅观察性诊断，不宣称产品功能验收或修复完成。后续若更改VPN策略或应用通信实现，必须单独登记正式任务并重新验收。

## 复测方式

1. 创建新的QV档案，保留本档案不可变；复制[探测脚本](replay/probe.py)至新档案，按新执行者更新所有权检查常量。
2. 前置：设备10.0.34.117:5555已连接，Launcher TCP=3且7788监听，VPN状态与基准相同；本机en0=10.0.34.43。若源地址或VPN不同，须声明为新的诊断路径。
3. 在新档案RUNNING状态执行脚本。它保存基准，发起一次8秒TCP connect，不发送应用数据，然后关闭Socket并保存结果。
4. TCP connect成功才为VERIFIED；连接超时为FAILED。无抓包权限时只能记录缺口；不能根据超时单独认定落包点。

- 完成时间：2026-09-08 16:14:28 +08:00。
