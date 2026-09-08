# 资源来源

- device-netd.o：从用户指定设备 10.0.34.117:5555 的 `/system/etc/bpf/netd.o` 只读导出；原始名称 netd.o；用途是核对设备固件自带的 ingress BPF 规则，不是替换或安装文件。
- SHA256：406a0056aed04e1868305670ccfd2298e8d1ae36a634c9994ec3a23dd2dc6658。
- 获取命令：`adb -s 10.0.34.117:5555 exec-out cat /system/etc/bpf/netd.o`。
- 固件：PAX/A77/A77:10/QKQ1.200913.002/eng.releas.20251107.093633:user/dev-keys；内核 4.9.227-perf，2025-11-07。
- 设备 `/sys/fs/bpf` 不可读，因此未直接读取内核 uid_owner_map 或导出当前已加载程序。配置状态证据来自 dumpsys connectivity；静态指令证据来自系统分区对象。
