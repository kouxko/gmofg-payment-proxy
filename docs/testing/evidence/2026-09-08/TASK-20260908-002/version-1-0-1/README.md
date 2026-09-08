# Companion 1.0.1 版本与交付验证

用户要求主Android版本+1：versionCode1→2、versionName1.0.0→1.0.1。测试探针版本保持。

第一次执行原 `scripts/build-android-companion.sh`（环境复用相邻local-build）成功，包含45项单元、release Lint、四ABI与固定签名/16KiB门禁。完整日志见outputs/build.log；实际APK badging确认2/1.0.1，见outputs/badging-before-integration.txt，原始APK见resources/before-develop-integration.apk。

远端develop在此期间新增Document整数序列化修复5fd873d7，测试索引插入冲突需保留两边。合并后的最终APK需要重新构建原生domain依赖；后续结果追加，不能把本次原生缓存产物作为合并后最终APK。此版本标识改变不触发再次覆盖当前活动真机VPN；此前真机验证证据保持原版本标识，产品修复代码相同。
