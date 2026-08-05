# Intercept Proxy

Intercept Proxy 是 Rust 驱动的通用 HTTP/HTTPS 测试代理：支持标准正向代理、CONNECT、
显式 MITM、固定上游反向代理、TLS/mTLS、抓包、规则、断点和故障注入。Android Companion
使用按应用 allowlist 的 `VpnService`，为指定应用提供可复现的 TCP/IP 弱网。

## 架构

- Rust 负责全部网络、证书、规则、校验、存储、ADB 和弱网逻辑。
- Next.js 静态导出与 HeroUI 只负责显示 ViewModel 和提交用户操作。
- `host` 不依赖 Tauri，可被测试和未来 TUI/CLI 复用。
- 默认应用不携带具体业务模板、地址、证书或返回码。

维护代码前建议先阅读 [系统设计与原理](docs/architecture/README.md)，其中按端到端链路说明了
模块职责、HTTP/TLS 转发、规则管线、证书与秘密、Android VPN 透明路由、状态机和失败恢复。
产品验收基线见 [需求文档](docs/requirements.md)，实际操作见
[用户使用说明](docs/user-operation-guide.md)。

## 开发

```bash
pnpm install
pnpm check
pnpm tauri:dev
```

Android Companion 位于 `android-companion/`。桌面端只使用系统已有 `adb`，不内置
platform-tools。执行 `pnpm build:android-companion` 会先构建并校验固定升级签名的 release
APK，再把同一 APK 放入 Tauri 资源目录；`pnpm tauri:build` 会自动先执行这个步骤。

## 安全提示

只在隔离测试环境使用 HTTPS MITM、mTLS 客户端身份和弱网故障。Root CA 私钥、P12 和密码
不会进入前端或 Workspace 导出文件；导出的 Root CA 只包含公开证书。
Companion 的项目固定 keystore 随源码保存，只用于维持覆盖升级身份，不应被视为保密发布凭据。
