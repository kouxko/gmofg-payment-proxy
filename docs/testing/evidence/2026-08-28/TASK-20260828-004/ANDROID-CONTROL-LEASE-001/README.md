# ANDROID-CONTROL-LEASE-001

- 目的：验证 Android VPN 控制失联保护的默认策略、逐设备心跳、5 秒 generation 看门狗、取消清理和多设备隔离。
- 环境：macOS 本地 Rust/TypeScript/Kotlin 可控时钟与模拟控制通道；未对已连接真实设备执行破坏性拔线或终止操作。
- 输入：开启或关闭保护的 Android Network Profile、`serial + epoch/generation`、heartbeat v2 报文、阻塞或取消的 ADB forward、旧代超时与新代启动交错。
- 预期：默认开启；每个 serial 独立续租；控制失联满 5 秒只关闭当前 generation 的 VPN/TUN；旧代回调不能停止新代；取消后的 ADB forward 必须精确清理；一台设备失败不影响其他设备。
- 实际：Kotlin 控制租约/协议/generation 17/17；Rust heartbeat 1/1、forward/取消清理 11/11、lease 隔离 3/3、Android engine 47/47；Android 前端 32/32；严格静态检查、格式、类型、源码大小和生成绑定均通过。
- 未执行：Android Gradle 在离线环境无法解析构建插件；真实 USB 拔线、桌面异常退出与真机 5 秒边界未执行，因为两台已连接设备没有独占操作窗口。
- 结果：`PASS_WITH_NOT_RUN`。自动化合同通过；真实设备场景保持明确未验证。
