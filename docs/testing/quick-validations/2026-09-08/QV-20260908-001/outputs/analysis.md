# 诊断分析

## 当前已验证

1. 用户切换后，POSLINK_COMMUNICATION_TYPE=3；7788 处于 LISTEN，UID 10116。
2. VPN tun0 的 UID 范围含 10116；PermissionMonitor 的 Interface filtering rules 同样将 UID 10116 列在 tun0。
3. 2026-09-08 16:06:35 +08:00，从 10.0.34.43:65064 到 10.0.34.117:7788 的 TCP connect 8 秒超时，应用数据发送量 0。采样期间仅见 LISTEN，无该连接记录。
4. 设备 netd.o 的 bpf_cgroup_ingress 指令 86-92 从 uid_owner_map 读取允许接口与规则；111-122 检查规则位 0x20，若允许接口非0且实际ifindex非1且不等于允许接口，选择drop。这里不检查 protectedFromVpn 标记。
5. 设备 tcp_fwmark_accept=1、fwmark_reflect=1；此前带 mark 0x3006d 的路由查询返回 wlan0，而无 mark 的 UID 查询返回 tun0。

## 推断（高置信度）

VPN UID 入站接口隔离与物理 WLAN 的 PosLink 服务接入存在冲突，是当前首要原因。问题更可能发生在入站过滤，而非先前仅凭默认 UID 路由推断的回包被送入 VPN。

## 未知/限制

没有本机 BPF 抓包权限，设备无 root 且内核 BPF map 不可读；未取得这次 SYN 的落包记录或丢弃计数，不能宣称已逐包证明丢弃点。主机为10.0.34.43，未复测10.0.28.17/39的相同请求路径。不发送业务报文、不切换VPN、不更改Launcher模式。Android10原版r1/r47没有同一IIF实现，不能把当前厂商固件实现泛化为全部Android10。
