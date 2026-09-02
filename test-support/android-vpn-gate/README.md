# Android VPN 弱网门禁

该门禁只选择 `com.android.shell` 进入 VpnService，用设备端 `nc` 产生真实 TCP/UDP
流量，并同时使用 Companion 自身作为非目标应用验证隔离。报告只保存计数、耗时和
错误，不保存应用 Payload。

```bash
ANDROID_SERIAL=127.0.0.1:6555 test-support/android-vpn-gate/run.sh
```

`reports/latest.json` 是本轮证据。门禁按真实设备流量逐项验证：

- IPv4 TCP/UDP 转发、目标应用接管、Companion 非目标应用绕过和 ADB 存活；
- 固定延迟、均匀抖动、上下行限速、重复包、乱序；
- 第 N 个 SYN 丢弃及真实重传、第 N 个 SYN-ACK/ACK 丢弃；
- 随机全丢、Gilbert-Elliott 连续丢包和指定时间窗口断网；
- DNS 53/853 黑洞、多目标地址命中与绕过；
- MSS Clamp、IPv4 分片、PMTU 信号、PMTU 黑洞和 Payload 位翻转；
- 停止 VPN 后 5 秒内恢复系统网络；恢复超时或停止命令失败都会使门禁失败。

门禁最后故意检查 PMTU signal/fragment 是否真正落到数据面；如果实现仍只停留在
Rust 决策层，门禁必须失败，不能把未实现功能报告为通过。

IPv6 场景不会再被隐含到普通 `PASS` 中：

- 设置 `ANDROID_VPN_GATE_IPV6_HOST` 后，门禁会启动独立的 IPv6 TCP/UDP 服务，分别验证
  目标应用转发和非目标应用绕过；端口可用 `ANDROID_VPN_GATE_IPV6_TCP_PORT`、
  `ANDROID_VPN_GATE_IPV6_UDP_PORT` 覆盖；
- 未配置可达 IPv6 端点或设备没有 IPv6 路由时，报告中的
  `ipv6_tcp_forwarding_and_isolation`、`ipv6_udp_forwarding_and_isolation` 明确为 `SKIP`，
  整轮结果为 `PASS_WITH_SKIPS`，并保存设备地址、路由和跳过原因，不能写成完整通过；
- IPv6 扩展头目前按 `Other` 传输层处理。整包延迟、限速和随机丢包仍生效，但端口过滤、
  DNS 黑洞和第 N 个 TCP 标志规则不适用；该边界写入报告的 `supported_boundaries`。

以下能力受模拟器网络环境限制，不伪装成现场通过，而由 Rust/Kotlin 自动测试负责：

- IPv6 Packet Too Big：Genymotion 当前没有可达的宿主 IPv6 测试端点；
- FIN/RST 双方向第 N 包：现场请求只能稳定控制 SYN/SYN-ACK/ACK，五种 TCP 标志的
  双方向精确计数由 `every_supported_tcp_flag_and_direction_can_be_dropped_exactly_at_n`
  覆盖；
- shared UID、包卸载、签名或 UID 变化：通过 Profile 启动前校验单元测试覆盖，避免
  为验证破坏模拟器中的真实安装状态。
