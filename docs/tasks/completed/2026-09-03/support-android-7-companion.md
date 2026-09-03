# TASK-20260903-007：设备端 VPN APK 最低兼容 Android 7

- 任务 ID：TASK-20260903-007
- 状态：已完成
- 任务日期：2026-09-03
- 创建时间：2026-09-03 16:04:54 +08:00
- 开始时间：2026-09-03 16:04:54 +08:00
- 最后更新时间：2026-09-03 16:36:05 +08:00
- 完成时间：2026-09-03 16:36:05 +08:00
- 创建路径：`docs/tasks/pending/2026-09-03/support-android-7-companion.md`
- 归档路径：`docs/tasks/completed/2026-09-03/support-android-7-companion.md`
- 关键词：`Android 7`、`API 24`、`VPN`、`Companion`、`Foreground Service`、`Notification`
- 任务优先级：高（改变设备端 APK 的平台兼容合同，覆盖 Kotlin、Manifest、Rust NDK 与四 ABI，且最终支持结论需要 Android 7 运行证据）

## 背景、目标与需求确认

当前设备端 VPN APK 的 `minSdk` 与 Rust NDK 默认 API 都是 26，只能安装在 Android 8.0 及以上系统。
用户先询问能否最低支持 Android 7，随后明确要求“修改一下 兼容android 7”。

目标：把设备端主 APK 与配套探针的最低系统版本降为 Android 7.0（API 24），保持 `targetSdk 36`、
固定签名、四 ABI、16 KiB ELF 对齐、VPN 授权、控制协议、弱网数据面和 Android 8+ 行为不变。

## 范围、不在范围与需求就绪检查

- 将 `app`、`isolation-probe`、`target-probe` 的 `minSdk` 调整为 24。
- 将 Rust Android 数据面默认 NDK API 调整为 24，并验证四 ABI 均可链接。
- 为 API 24/25 使用普通 `startService`，API 26+ 继续使用 `startForegroundService`。
- API 24/25 使用无 Channel 的前台通知构建方式与低优先级；API 26+ 保留 Notification Channel。
- 检查 Manifest 中高版本属性在低版本的兼容声明，不删除 targetSdk 36 所需权限和服务类型。
- 不改变桌面端命令、Profile Schema、控制协议、弱网模型、路由合同或签名身份。
- 不增加 AndroidX 或其他依赖，不增加未约定的运行时回退。
- Android 7 指 Android 7.0/7.1（API 24/25）；Android 6 及以下不在范围。

具体输入：API 24 系统安装并通过 ADB 唤醒、授权、启动 VPN；API 26+ 继续使用现有前台服务路径。
输出：release APK 的 Manifest `sdkVersion` 为 24，四 ABI 存在且 Android 7 上服务启动不调用 API 26 方法。
错误行为：控制协议、VPN 授权和数据面失败继续使用现有 fail-closed 合同，不因低版本兼容而吞错或伪造成功。

需求就绪检查：目标、最低 API、范围、兼容分支、保持项和自动化验收已明确；真实 Android 7 设备或模拟器
是否可用尚待环境检查，不阻止先实施可独立验证的代码和构建修改。2026-09-03 16:04:54 +08:00 进入实现。

## 问题与根因分析

- 实际现象：当前 release APK 的 `aapt dump badging` 显示 `sdkVersion:'26'`，Android 7 无法安装。
- 预期行为：用户要求最低 Android 7.0，APK 应声明 API 24 并避免在 API 24/25 调用 API 26 专属方法。
- 最小复现：读取 `android-companion/app/build.gradle.kts` 与现有 release APK badging。
- 当前已验证：主 APK、两个探针和 Rust NDK 脚本均以 26 为最低版本；通知渠道、带 Channel 的
  `Notification.Builder` 及三个 `startForegroundService` 调用直接依赖 API 26。
- 当前已验证：用 `ANDROID_MIN_API=24` 分别构建 arm64-v8a、armeabi-v7a、x86、x86_64 Rust 数据面均通过。
- 推断：Manifest 的高版本权限、属性与 `<property>` 在旧系统被忽略；仍需以 API 24 lint、APK 构建与运行验证确认。
- 未知：当前环境是否已有可启动的 API 24 模拟器或真实设备。
- 根因：平台最低版本被显式固定为 26，同时 Kotlin 前台服务和通知实现直接使用 API 26 合同，没有 API 24/25 分支。
- 影响范围：仅 Android Companion/探针构建与服务启动通知边界；桌面端和 Rust 业务合同不变。

## 最小改动与最优设计

| 方案 | 分析 |
| --- | --- |
| 最小改动 | 三处 Gradle 与 NDK 默认 API 改为 24；集中新增一个版本感知的服务启动入口；通知创建按 API 26 分支。职责明确、无重复、无需依赖。 |
| 最优设计 | 引入 AndroidX Core 的 `ContextCompat`/`NotificationCompat` 统一兼容层。会新增依赖，当前只有少量平台分支且现有项目无生产 AndroidX 依赖，收益不足。 |

采用最小改动：在 Companion 内部集中平台差异，不把版本判断散落到业务控制流程。

## 小任务、测试与审查

| ID | 内容 | 状态 | 验收 |
| --- | --- | --- | --- |
| T01 | 增加 API 24 服务启动兼容入口及单元测试 | 已完成 | API 24/25 选择 `startService`，API 26+ 选择 `startForegroundService` |
| T02 | 兼容 API 24 前台通知 | 已完成 | API 24/25 不创建 Channel，通知使用低优先级旧构造器 |
| T03 | 降低 APK、探针与 Rust NDK 最低 API | 已完成 | 所有最低版本统一为 24 |
| T04 | 执行 lint、单测、release APK 和四 ABI 门禁 | 已完成 | APK badging=24，签名/ABI/对齐保持通过 |
| T05 | API 24 运行验证、独立审查、证据与归档 | 已完成 | 环境无 API 24/25 设备或 AVD，运行项如实记为 NOT_RUN；独立复审 APPROVE |

- 文档影响：同步 Android Companion README 与 Android VPN 架构文档的最低版本和兼容分支。
- 测试计划：先补服务启动策略与通知策略的失败测试，再实现；执行 JVM 单测、lintRelease、assembleRelease、
  APK badging、固定签名、四 ABI/对齐验证，并检查 API 24 运行环境。
- 对抗审查：平台兼容合同为高优先级，实施完成后执行独立代码审查。

## 实施、测试与完成总结

- 主 APK、isolation probe、target probe 与 Rust NDK 默认最低版本统一为 API 24。
- 新增 `AndroidPlatformCompatibility` 集中选择 API 24/25 的 `startService` 与 API 26+ 的
  `startForegroundService`；通知同样按该已测试策略选择旧构造器或 Notification Channel。
- 三处既有服务启动调用均改为兼容入口，不改变控制协议、VPN 授权、数据面或错误传播合同。
- JVM 回归 8 个测试类、28 项全部通过；`lintRelease` 为 0 error、5 个既有 warning，无 `NewApi`。
- 主 release APK、两个 probe 构建通过；主 APK badging 为 `sdkVersion:'24'`、`targetSdkVersion:'36'`，
  四 ABI、固定单 signer、zipalign 与 16 KiB ELF LOAD 对齐门禁通过。
- 已验证 APK 暂存至 `src-tauri/resources/android-companion.apk`，与构建产物逐字节一致，SHA-256 为
  `5fa98df0dda2c856bafd4d18f3992cd6351420248416d899edcb40ecbb1696aa`。
- 独立代码复审：`APPROVE`。
- Android 7 真机/模拟器运行验收：`NOT_RUN`。当前只有 API 29、29、31 设备且没有 API 24/25 AVD；
  因此完成结论为 `PASS_WITH_ANDROID_7_RUNTIME_NOT_RUN`，不把真实安装、授权和数据面描述为已通过。
- CI、提交、推送和发布：`NOT_RUN`，用户未要求。
- 证据：[ANDROID-7-COMPATIBILITY-001](../../../testing/evidence/2026-09-03/TASK-20260903-007/ANDROID-7-COMPATIBILITY-001/README.md)。
