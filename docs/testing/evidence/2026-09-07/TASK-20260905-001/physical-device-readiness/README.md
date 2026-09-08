# 物理设备就绪核对

TASK-20260905-001；2026-09-07。仅只读清点完成，真机验收NOT_RUN。

ADB当前连接A920MAX 2740072778（API29/armeabi-v7a）与IM28 3330000478（API31/armeabi-v7a）。A920已安装Companion；IM28未安装；两台都运行现有业务Launcher。原“无设备”条件已变化，但不能因此判定通过验收。实际命令/输出/执行时间见metadata.json和outputs。读取前台时只保存mResumedActivity/topResumedActivity字段。

原SavedRecoveryAdmissionInstrumentedTest只验证恢复准入，不启动真实Service/TUN；connectedDebugAndroidTest会安装/卸载APK。现有完整android-vpn-gate固定模拟器Host=10.0.3.2，emulator-proxy-gate拒绝物理设备，不能直接搬到物理机运行。完整失联/手动恢复/LocalSocket/TUN验收会改变设备网络，用户尚未指定专用验收机及维护窗口。本轮没有安装、授权VPN、发送控制帧、改转发、结束App或发送业务请求。

后续具备条件时在新独立用例执行，引用本次就绪清点及[原manual-recovery](../../../2026-09-05/TASK-20260905-001/manual-recovery/README.md)，补保存设备前态、明确Profile/Listener/目标应用、控制失联与手动恢复步骤、业务请求/响应和恢复状态。当前清点不能替代正式验收；设备接管选择通过用户问题等待回答。

复测：逐条执行metadata.json内command（全部adb shell只读查询），或 `adb devices -l`。要求ADB和设备已连接/调试授权。设备不可复制，无文件型输入；resources/replay目录不需要占位。协议报文、截图N/A；网络/业务验收NOT_RUN。
