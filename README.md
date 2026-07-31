# GMO-FG Payment Proxy

面向 GMO-FG Payment 联机调试的 Windows/macOS 桌面代理工具。它把原来的
`Server ↔ App` 通信改为 `Server ↔ Proxy ↔ App`，由 Rust 实现双向 mTLS、
HTTP/1.1 转发、抓包、断点、规则、故障注入、证书、设置、校验和导出。
Next.js + HeroUI 仅负责展示 Rust ViewModel 和发送用户操作。

产品、UI、Rust 架构、IPC 和测试的唯一实施基线是
[`docs/requirements.md`](docs/requirements.md)。

## 第一次阅读本项目

如果你暂时不了解 Rust、Tauri、mTLS 或网络代理，建议按下面顺序阅读：

1. 先读 [`docs/requirements.md`](docs/requirements.md) 的“给第一次接触本项目的读者”、
   “新手术语表”和“第一次使用的端到端旅程”。它是需求与代码设计的唯一事实来源。
2. 再读 [`docs/user-operation-guide.md`](docs/user-operation-guide.md)，按页面学习怎样操作成品。
3. 想理解为什么核心能脱离 UI 复用时，读
   [`docs/generic-core-product-boundary.md`](docs/generic-core-product-boundary.md)。
4. 想理解限速、抖动、间歇通断和中途断连时，读
   [`docs/proxy-weak-network-fault-injection-design.md`](docs/proxy-weak-network-fault-injection-design.md)。
5. 最后从 `src-tauri/crates/domain` 开始读 Rust，再依次阅读 `application`、`proxy`、
   `infrastructure`、`host`、`product-payment`、`src-tauri` 和 `src`。关键模块已经补充
   中文注释，重点解释职责、调用方向、状态机、失败边界和不能跨越的边界。

可以把仓库先简化理解为：

```text
Next.js + HeroUI（只显示和收集操作）
                 │ Tauri Command / Channel
                 ▼
Tauri 薄适配层 ──► UI 无关的 Rust Host / Application
                             │
          ┌──────────────────┼──────────────────┐
          ▼                  ▼                  ▼
      领域规则           代理运行时          基础设施
   domain/application   rustls/HTTP/1      SQLite/密钥/文件
                             ▲
                             │ ProductProfile
                      Payment 产品适配层
```

如果代码注释与需求文档出现冲突，以 `docs/requirements.md` 为准，并先修正文档和测试，
再修改实现。不要只根据截图或某一次实机结果推断完整需求。

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
`.github/workflows/windows-release.yml` 在分支构建时只编译并预热 Windows/Tauri
缓存，不上传未签名二进制。只有 `v*` 标签构建才会上传 MSI、NSIS 和便携 ZIP；
标签构建必须配置以下受保护的 GitHub Actions 值，否则 fail closed：

- Secret `WINDOWS_CERTIFICATE`：组织 Authenticode PFX 的 Base64。
- Secret `WINDOWS_CERTIFICATE_PASSWORD`：PFX 密码。
- Variable `WINDOWS_TIMESTAMP_URL`：证书颁发机构提供的 RFC 3161 时间戳服务。

Tauri 构建后，workflow 会在上传前验证应用 EXE、MSI 和 NSIS 的签名证书指纹及
时间戳；便携 ZIP 只复制已验证签名的 EXE。
