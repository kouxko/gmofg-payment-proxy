# ANDROID-7-COMPATIBILITY-001

## 目的

验证 Android Companion 主 APK、两个测试探针和 Rust 数据面已经把最低平台统一降为
Android 7.0（API 24），同时保持 API 26+ 前台服务/通知行为、固定签名、四 ABI 和 16 KiB 对齐。

## 环境与被测状态

- 执行时间：2026-09-03 16:36:05 +08:00
- 平台：macOS arm64
- 当前提交：`167192a6b85992170c692279871c13965db4aaff`
- Android Gradle Plugin：9.0.0
- Gradle：9.1.0；下载包 SHA-256 为
  `a17ddd85a26b6a7f5ddb71ff8b05fc5104c0202c6e64782429790c933686c806`，与 Gradle 官方校验值一致
- compileSdk/targetSdk：36/36
- Android NDK：使用仓库构建脚本自动发现的本机最新安装版本
- 被测文件内容以 `metadata.json` 中的逐文件 SHA-256 为准；测试期间其他任务只修改了范围外文件

## 步骤与实际结果

1. 修改前读取既有 release APK：`aapt dump badging` 显示 `sdkVersion:'26'`，证明 Android 7 无法安装。
2. 使用 `ANDROID_MIN_API=24` 探测 Rust 数据面，arm64-v8a、armeabi-v7a、x86、x86_64 均成功链接。
3. 将主 APK、两个探针与默认 Rust NDK API 统一改为 24；集中处理 API 24/25 的 `startService` 与
   旧式低优先级通知，API 26+ 保留 `startForegroundService` 和 Notification Channel。
4. 执行主 APK JVM 回归：

   ```bash
   gradle --no-daemon :app:testDebugUnitTest -x buildRustAndroid
   ```

   实际：8 个测试类、28 项测试，0 failure、0 error、0 skipped；其中新增兼容策略 2 项通过。
5. 执行完整 release lint：

   ```bash
   gradle --no-daemon :app:lintRelease -x buildRustAndroid
   ```

   实际：PASS，0 error、5 warning；没有 `NewApi` 错误。5 个 warning 均为既有同步持久化、
   target API 提示、定时调度建议和未使用资源，不属于本次 API 24 兼容缺口。
6. 以默认 API 24 构建四 ABI Rust 数据面，全部 PASS；再构建主 release APK 与两个 debug 探针，
   109 个 Gradle task 完成，BUILD SUCCESSFUL。
7. 运行 `scripts/verify-android-companion.sh`：固定 signer、单 signer、四 ABI、zipalign 与四个 ELF
   16 KiB LOAD 对齐全部 PASS。
8. 对三个 APK 执行 `aapt dump badging`：

   - `com.interceptproxy.vpn`：`sdkVersion:'24'`、`targetSdkVersion:'36'`，包含四 ABI；
   - `com.interceptproxy.vpn.isolationprobe`：`sdkVersion:'24'`；
   - `com.interceptproxy.vpn.targetprobe`：`sdkVersion:'24'`。

9. 将已验证 release APK 暂存到 `src-tauri/resources/android-companion.apk`；与 build output 逐字节一致，
   SHA-256 均为 `5fa98df0dda2c856bafd4d18f3992cd6351420248416d899edcb40ecbb1696aa`。
10. 独立代码复审结果为 `APPROVE`。复审前发现生产分支与可测试版本策略重复；修正为生产代码直接复用
    `usesApi26ForegroundContract` 后，重新执行单测、lint、release 构建与 APK 门禁均通过。
11. 一次并行 Kotlin 编译触发增量缓存注册冲突并导致测试类加载失败；清理 `app` 构建目录后，使用单 worker、
    关闭 Kotlin 增量编译重新执行，最终 8 个测试类、28 项测试全部通过。该失败未通过修改期望或忽略测试处理。

## 验收结果

- PASS：构建合同、Manifest、Kotlin API 分支和 Rust NDK 最低 API 均为 24。
- PASS：API 24/25 不再直接调用 `startForegroundService`、Notification Channel 或带 Channel 的构造器。
- PASS：API 26+ 保持原前台服务启动和 Notification Channel 行为。
- PASS：release APK 固定签名、四 ABI 和 16 KiB 对齐保持不变。
- PASS：桌面打包所读取的暂存 APK 已更新为同一 API 24 release 产物。
- NOT_RUN：Android 7.0/7.1 真机或模拟器安装、VPN 授权和实际弱网数据面。本机连接设备为 API 29、29、31，
  且没有可启动的 API 24/25 AVD；因此本证据证明“可安装/可链接/无静态 NewApi 缺口”，不把真实 Android 7
  运行描述为已验收。
- NOT_RUN：CI、提交、推送和发布，用户未要求。

## 复测入口

```bash
cd android-companion
gradle --no-daemon :app:lintRelease :app:testDebugUnitTest :app:assembleRelease \
  :isolation-probe:assembleDebug :target-probe:assembleDebug
cd ..
scripts/verify-android-companion.sh android-companion/app/build/outputs/apk/release/app-release.apk \
  --release --expected-cert-sha256 "$(tr -d ':[:space:]' < android-companion/signing/certificate-sha256.txt)"
```

最终运行验收需在 API 24 或 25 设备上安装 release APK，完成 VPN 授权，分别验证 ADB 控制启动、
通知停止、无代理独立弱网和一个透明代理路由场景。

## 不适用项

- HTTP/Socket 业务报文、Server 响应：N/A，本任务不改变桌面 Listener、协议或业务数据。
- UI 截图：N/A，Companion 只有系统 VPN 授权入口，本次没有视觉改动。
- APK 副本：未在证据目录重复保存 34,841,581 字节产物；权威生成路径和 Tauri 暂存路径逐字节一致，
  本页保存稳定 SHA-256 与完整复测命令。
