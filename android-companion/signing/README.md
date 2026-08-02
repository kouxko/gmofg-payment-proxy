# Android Companion 固定签名身份

`intercept-proxy-companion.jks` 固定 `com.interceptproxy.vpn` 的安装和升级身份。CI、开发机和
桌面安装包内的 APK 都必须使用同一证书，避免每次构建后无法覆盖更新设备端组件。

该 keystore 与密码随源码分发，因此它**不是保密的发布凭据，也不提供来源真实性**。它只保证
同一项目构建之间的签名连续性。若未来进入应用商店或公开分发，应另建受保护的商店签名流程，
并同步调整桌面端的 Companion 证书指纹门禁。

证书 SHA-256 由 `certificate-sha256.txt` 固定。`scripts/build-android-companion.sh` 会先构建
release APK，再校验包名、唯一 signer、证书指纹、四 ABI 与 16 KiB 对齐，最后才把 APK 放入
Tauri 资源目录。
