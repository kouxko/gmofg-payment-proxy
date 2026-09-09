# Companion 1.0.1 版本与交付验证

用户要求主Android版本+1：versionCode1→2、versionName1.0.0→1.0.1。测试探针版本保持。

第一次执行原 `scripts/build-android-companion.sh`（环境复用相邻local-build）成功，包含45项单元、release Lint、四ABI与固定签名/16KiB门禁。完整日志见outputs/build.log；实际APK badging确认2/1.0.1，见outputs/badging-before-integration.txt，原始APK见resources/before-develop-integration.apk。

远端develop在此期间新增Document整数序列化修复5fd873d7，测试索引插入冲突需保留两边。合并后的最终APK需要重新构建原生domain依赖；后续结果追加，不能把本次原生缓存产物作为合并后最终APK。此版本标识改变不触发再次覆盖当前活动真机VPN；此前真机验证证据保持原版本标识，产品修复代码相同。

## 最终版本与 develop 集成结果

用户明确纠正后，最终权威版本为 versionCode **101** / versionName **1.0.1**。此前2仅为未交付中间构建，不作为最终验收。

合并gitlab/develop 5fd873d7后，显式执行 `bash android-companion/scripts/build-rust-android.sh` 重建四ABI（避免Gradle仅跟踪engine输入而漏掉上游domain变化），再执行完整 `bash scripts/build-android-companion.sh`。两次完整输出分别为integrated-rust-build.log、integrated-final-build.log。45/45测试、Lint零错误/5既有位置警告、签名/ABI/16KiB门禁PASS；APK内四个so与刚重建的JNI资源逐字节一致。final-badging.txt确认101/1.0.1，final-artifact.json记录SHA、源文件和测试结果。

最终可交付资源：resources/companion-1.0.1-code101.apk。当前远程设备仍使用此前已证明LAN修复的旧版本标识APK；本轮未再次覆盖活动VPN，也没有宣称新版本最终二进制已做业务E2E。
