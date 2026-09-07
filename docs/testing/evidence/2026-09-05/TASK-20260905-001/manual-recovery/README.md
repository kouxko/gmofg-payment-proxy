# MANUAL-RECOVERY-001

TASK-20260905-001 #3：受保护VPN停止后等待用户显式启动。**本地验证通过，独立审查APPROVE；真机NOT_RUN。** 父任务保留已阻塞，未归档为完成。

- [Android恢复准入阶段](android/README.md)：protected等待、unprotected恢复、JNI代际保护；旧阶段APK保留。
- [最终Android原子热更新与APK](android-atomic-hot-apply/README.md)：apply_active、JVM45/45、lint/release/签名/对齐/4ABI通过。最终APK SHA256 `ced81ae37ef0415f38dfe5d8d0d830a359bb6999ebc86a2b6a46536e03f3e1df`。
- [桌面Rust与实际控制Frame](desktop/README.md)：Application432/432、Infrastructure474/474、最终专项5/5；Clippy/fmt/架构/文档通过。十个控制Frame的JSON/bin/hex及复测入口已保存。
- [独立整体审查](independent-review.md)：无P0/P1/in-scope P2。
- [审查后文档措辞验证](post-review-docs/README.md)：仅去除过时“计划中”用语，无代码变更。

[原父用例](../android-contracts/README.md)保持不变。当前实际源/资源映射见verified-current-sources.json；阶段快照不改写。六项既有500行超限未扩大。Display1MiB、默认5秒保护及Task002 Listener Draft合同保持。

使用方式：有owner时点击“应用修改”，无owner时点击“启动”；后台重连/刷新不自动恢复已停止的受保护VPN。有效运行中的LAN热更新需设备原子活租约准入。旧Companion拒绝未知apply_active，不回退普通apply。

限制：无连接Android设备，仪器测试仅编译通过；真实LocalSocket/TUN、设备业务响应与UI截图未验证，不能用JVM或本地TCP替代。未提交、推送、安装或触发CI。各子用例README/metadata包含环境、实际输入输出、资源和复测步骤。
