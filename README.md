# Intercept Proxy

Intercept Proxy 是一个由 Rust 驱动的桌面网络测试代理，用于观察、解析、修改和转发 HTTP 与
Socket 数据。桌面端使用 Tauri + Next.js，Android Companion 通过按应用 allowlist 的
`VpnService` 把指定应用流量导向代理。

当前实现强调三件事：真实网络字节由 Rust 掌控、HTTP 与 Socket 保留各自协议边界、同一 App
连接的所有收发和失败证据按顺序归入一个 Exchange。

## 当前能力

- HTTP 正向 absolute-form 请求与固定 Server 反向转发。
- 固定 Server 的 HTTP/HTTPS、下游 TLS/mTLS、上游 TLS/mTLS 和双侧 TLS/mTLS。
- Socket RemoteServer/LocalServer、TCP/TLS/mTLS、按协议转发与透明转发。
- 内置 Rhai 协议包，以及 Socket 外部 WebSocket JSON-RPC 协议包。
- Frame、Decode、Document Rules、Encode、Display 的强类型 Pipeline。
- HTTP 基础规则、协议 Document 规则、断点、Mock、弱网与故障注入。
- HTTP/Socket 统一规则列表和统一实时抓包列表。
- 连接级 ExchangeObservation、运行日志、只读 MCP 和复现报告。
- Workspace、协议包、Listener TLS 材料和设置的应用级导入导出。

当前不支持 HTTP CONNECT、HTTP Upgrade/WebSocket tunnel、CONNECT MITM 和 HTTP 外部
WebSocket 协议包。这些请求会在创建 Server connection 与 Exchange 前明确返回 `501`，不会
静默进入透明转发或其他兜底路径。

## 数据流摘要

协议模式严格按一问一答推进：

```text
App read
  -> Upstream Reader Pipeline
  -> Server write
  -> Server read
  -> Downstream Reader Pipeline
  -> App write
```

HTTP Reader 执行 `Decode -> Display`，Socket Reader 先执行 `Frame`，再执行
`Decode -> Display`。Writer 对 Reader 产生的 Envelope 克隆 Document，顺序执行 Rules，
随后 Encode 并写入真实连接。Socket 透明模式不构造 Document，读取多少就向另一端写多少。

LocalServer 与 RemoteServer 实现同一个 Server 端口：LocalServer 只是本地回环服务，不是第二套
交易流程。观测链路 fail-open，不能改变成功交易；Frame、Decode、Rules、Encode 或 transport
失败则 fail-closed，结束当前 Exchange。

## 代码组织

| 目录 | 职责 |
| --- | --- |
| `src-tauri/crates/domain` | Workspace、Document、规则、证书引用等领域不变量 |
| `src-tauri/crates/application` | Use Case、ViewModel、端口、事件和能力矩阵 |
| `src-tauri/crates/exchange` | Exchange、Pipeline、Envelope、Reader/Writer 与方向约束 |
| `src-tauri/crates/proxy` | HTTP、Socket、TCP/TLS、连接和 Listener runtime |
| `src-tauri/crates/protocol-scripting` | 协议包校验、Rhai、Frame/Decode/Encode/Display |
| `src-tauri/crates/infrastructure` | SQLite、证书、ADB、协议包、外部包和运行适配器 |
| `src-tauri/crates/host` | 无 UI 的完整 Rust 组合根与后台任务所有权 |
| `src-tauri/src` | Tauri Command、AppState、日志、MCP 和桌面生命周期 |
| `src/features` | 前端 feature、交互草稿和 Rust ViewModel 展示 |
| `android-companion` | Android VpnService、TUN、SOCKS5 与弱网数据面 |

更完整的依赖方向、源码入口和测试锚点见
[模块与代码组织](docs/architecture/modules.md)。

## 文档导航

- [新人接手与项目全景指南](docs/onboarding-guide.md)：从零开始理解系统、运行项目、定位代码、测试和发布。
- [文档总览](docs/README.md)：产品、架构、操作、诊断和测试文档入口。
- [架构总览](docs/architecture/README.md)：推荐阅读顺序与当前架构结论。
- [Exchange 与 Pipeline](docs/architecture/exchange-pipeline.md)：核心 trait、泛型约束和连接生命周期。
- [真实数据流](docs/architecture/data-flow.md)：HTTP、Socket、LocalServer、透明转发和错误传播。
- [规则与协议包](docs/architecture/rules-and-protocol-packages.md)：规则能力矩阵、Document、Rhai 和外部包。
- [观测与诊断](docs/architecture/runtime-observability.md)：ExchangeObservation、日志、UI 刷新和 MCP。
- [安全与持久化](docs/architecture/security-and-persistence.md)：TLS/mTLS、SQLite、证书和导入导出。
- [用户操作说明](docs/user-operation-guide.md)：从 Workspace 到真实交易验证的操作步骤。
- [发布级验证矩阵](docs/testing/release-validation-matrix.md)：可重复执行的完整验收合同。
- [2026-08-25 App 测试结果](docs/testing/release-validation-results-20260825.md)：最新 release App 测试用例与结果。
- [2026-08-24 发布验证结果](docs/testing/release-validation-results-20260824.md)：完整自动化、真实 App 与外部包证据。

## 开发

环境要求：Node.js、pnpm、Rust、Tauri 的 macOS 构建依赖；需要 Android Companion 时还要安装
JDK 和 Android SDK。桌面端使用系统已有的 `adb`，不内置 platform-tools。

```bash
pnpm install
pnpm tauri:dev
```

常用验证：

```bash
pnpm bindings
pnpm scan:architecture
pnpm scan:source-size
pnpm lint
pnpm typecheck
pnpm test:ui-contracts
pnpm test
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml --workspace
```

完整门禁：

```bash
pnpm check
```

只构建 macOS App：

```bash
pnpm tauri build --bundles app
```

`pnpm tauri:build` 会先构建并验证固定升级签名的 Android Companion release APK，再构建桌面
安装包。测试脚本和固定端口见 [发布级验证矩阵](docs/testing/release-validation-matrix.md)。

## 维护约束

- Rust 是规则、网络、证书、持久化和能力合法性的唯一真相源。
- 前端只渲染 Rust 返回的能力；保存时领域层再次校验，不能把隐藏控件当安全边界。
- 请求阶段不能配置响应状态/响应故障；响应阶段不能配置 request terminal；TLS 只允许证书条件与
  拒绝握手。限速方向由阶段固定，终止动作唯一且必须位于最后。
- HTTP 与 Socket 可以共用 Exchange 抽象、Document 和 UI 列表，但不能混用 Header/Status 与
  Frame/原始字节语义。
- 运行时报文只保存在有界内存，不写 SQLite。普通诊断日志使用独立滚动 JSONL。
- 修改架构、数据流、规则能力或验证范围时，同步更新 `docs/architecture`、ADR 和测试矩阵。
- 生产源码文件保持小于 500 行；优先拆分职责，不用压缩格式规避门禁。

## 安全边界

本项目用于隔离测试环境。应用导出可能包含用户明确选择的 Listener TLS 可移植材料和 P12 密码，
导入前会展示范围并执行严格校验；本机安装级 Root CA 私钥、运行时报文和 ExchangeObservation 不进入
应用备份。项目开发期允许完整业务报文进入诊断日志以便排查，但密钥、私钥和凭据仍不得进入普通
ViewModel、Debug 输出或文档。
