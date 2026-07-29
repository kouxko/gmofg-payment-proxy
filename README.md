# GMO-FG Payment Proxy

面向 GMO-FG Payment 联机调试的 Windows/macOS 桌面代理工具。它把原来的
`Server ↔ App` 通信改为 `Server ↔ Proxy ↔ App`，由 Rust 实现双向 mTLS、
HTTP/1.1 转发、抓包、断点、规则、故障注入、证书、设置、校验和导出。
Next.js + HeroUI 仅负责展示 Rust ViewModel 和发送用户操作。

产品、UI、Rust 架构、IPC 和测试的唯一实施基线是
[`docs/requirements.md`](docs/requirements.md)。

## 技术边界

- Tauri 2 加载 Next.js 静态导出，不运行 Node.js 服务端。
- 前端不直接访问网络、文件、证书、数据库、`localStorage` 或 `IndexedDB`。
- SQLite 不保存 Session Payload；Payload 仅在 Rust 受限内存中存在。
- 私钥和密码在 Windows 上使用当前用户范围 DPAPI、在 macOS 上使用 Keychain 保护。
- App → Proxy 与 Proxy → Server 均使用 TLS 1.2 双向认证。

## 本地开发

需要 Node.js 22、pnpm 11、稳定版 Rust，以及 Tauri 2 对应的平台依赖。

```bash
pnpm install --frozen-lockfile
pnpm bindings
pnpm tauri:dev
```

前端静态预览：

```bash
pnpm dev
```

## 验证

```bash
pnpm check
```

`pnpm check` 统一执行类型绑定生成、Rust-only 前端边界扫描、UI 合约测试、
ESLint、TypeScript、Next.js 静态构建、Rust fmt、Clippy 和 workspace 测试。
`src/generated/rust-types.ts` 只能由 Rust 生成：

```bash
pnpm bindings
git diff --exit-code -- src/generated/rust-types.ts
```

## Windows 交付

安装包：

```powershell
pnpm tauri build --bundles msi,nsis
```

便携包：

```powershell
./scripts/package-portable.ps1
```

便携版依赖目标机器已有 Microsoft Edge WebView2 Runtime。证书密文使用
DPAPI 当前用户范围保护，因此不能复制到另一 Windows 用户后继续解密。
`.github/workflows/windows-release.yml` 会在 Windows runner 上生成 MSI、NSIS
和便携 ZIP；正式分发前仍必须配置组织的 Windows 代码签名证书。
