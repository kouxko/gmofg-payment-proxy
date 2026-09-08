# 本次实际使用资源

- `original-companion.apk`：从目标设备 `pm path com.interceptproxy.vpn` 对应 base.apk 原样读取，覆盖升级前的恢复候选及 signer 对照；必需，不是待交付新版本。
- `services.jar`：目标设备 `/system/framework/services.jar` 原文件，证明实际 PAX Android 10 系统隔离策略；必需。不是 AOSP 通用版本假定。
- `ConnectivityService.java`、`Vpn.java`：上述 jar 用 JADX 1.5.6 `--single-class com.android.server.ConnectivityService` / `com.android.server.connectivity.Vpn --single-class-output <file> --no-res` 的实际反编译输出。关注 requiresVpnIsolation、networkMisc.allowBypass、updateLinkPropertiesInPlace。
- netd BPF 原文件与反汇编直接复用不可变 [QV 资源](../../../../../quick-validations/2026-09-08/QV-20260908-001/resources/device-netd.o)，避免重复活动副本。

旧 APK 备份不表示已经执行回滚。新 APK 安装、拉回哈希及签名输出在本用例 outputs 中记录。
