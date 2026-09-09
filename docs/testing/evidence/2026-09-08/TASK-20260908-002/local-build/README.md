# Companion allowBypass 构建与静态验收

被测基线 gitlab/develop `6764210eb057bb482b86a05cd1df23d7cdea5c5c`，仅 VpnTunFactory 增加 allowBypass 与注释；测试期间源码未变化，source-sha256.json 复查全部匹配。实际修改源、Gradle/脚本配置和单元测试输入见 resources；其他依赖由基线源码及 Cargo.lock 重建。

从仓库根目录执行 `bash scripts/build-android-companion.sh`。本机 PATH 加入 Gradle 9.3.0 bin，JAVA_HOME 使用 Android Studio jbr，ANDROID_HOME=/Users/codin/Library/Android/sdk；精确路径见 outputs/build-environment.json。SDK/NDK/Rust/Gradle 是系统依赖，不复制其本体；安装与版本配置见仓库脚本和构建日志。

实际结果：45 项 JVM 测试通过，无失败/跳过；lintRelease 0 errors、5 warnings（位于本次未修改的 RuntimeStateStore、ControlLeaseManager、targetSdk 与 string 资源）；四 ABI release 编译成功；固定 signer、四 ABI、zipalign/ELF 16 KiB 校验通过。完整 stdout/stderr 见 outputs/build.log，JUnit XML、lint 文本/XML/HTML 已保存。

`jadx --single-class com.interceptproxy.vpn.VpnTunFactory --single-class-output <output.java> --no-res <app-release.apk>` 复核实际打包 DEX：allowBypass 在 establish 之前且默认 IPv4/IPv6 路由仍存在，见 packaged-VpnTunFactory.java。APK 原始资源在相邻 [真机用例](../device-vpn-tcp/README.md) 中。

当前只证明实现、构建与制品合同；JVM 测试不证明 Android 系统 IIF 已解除。系统行为须在活动 VPN 上独立验收。Frame、业务协议、UI 截图、CI：N/A（未改变这些边界或未授权）。
