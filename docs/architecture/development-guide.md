# 开发、定位与验证指南

本文面向准备修改 Intercept Proxy 的开发者，说明代码应该放在哪里、如何沿数据流定位问题，以及
提交前必须完成哪些验证。所有命令均从仓库根目录执行，除非小节另有说明。

## 1. 先判断改动属于哪个边界

| 需求 | 首选位置 | 不应放入 |
| --- | --- | --- |
| 实体、不变量、稳定错误语义 | `src-tauri/crates/domain` | 页面、Tauri command |
| Exchange/Pipeline 协议抽象 | `src-tauri/crates/exchange` | HTTP/Socket 具体 I/O |
| Frame/Decode/Encode/Display Sidecar 合同与执行 | `src-tauri/crates/package-contract`、`src-tauri/crates/package-runtime` | 数据库、真实网络 |
| 用户用例、Port、ViewModel、事件 | `src-tauri/crates/application` | SQLite/ADB 命令细节 |
| SQLite、证书、文件、外部包、ADB 适配 | `src-tauri/crates/infrastructure` | 前端展示逻辑 |
| HTTP/Socket/TLS/Exchange 网络运行 | `src-tauri/crates/proxy` | Workspace 持久化 |
| Tauri/MCP/日志装配 | `src-tauri/src` | 重复领域规则 |
| Android 弱网/路由决策 | `src-tauri/crates/android-engine` | Android UI 文案 |
| Android VpnService/JNI 平台边界 | `android-companion` | 桌面业务用例 |
| 页面和用户意图 | `src/features` | 网络、证书或规则推断 |

`product-api` 提供产品注入合同，`host` 是 Rust 组合根。新增跨 crate 依赖前先运行架构门禁，避免反向
依赖或让 presentation 直接控制 infrastructure。

## 2. 开始修改前写清楚六件事

1. 用户操作和期望结果；
2. 领域不变量及稳定错误码；
3. 所属协议、方向和阶段；
4. 哪个组件拥有连接、快照、取消或资源；
5. 成功与失败分别发布什么 ViewModel/事件；
6. 哪些测试和真实连接可以证明结果。

对于 cleanup/refactor，先用现有回归测试锁住行为，再删除死代码、修复边界，最后增加必要的结构门禁。
不要先增加抽象再寻找用途。

## 3. 沿真实数据流定位

### 3.1 HTTP

```text
Listener accept
  -> downstream TCP/TLS
  -> Hyper HTTP/1.1 framing
  -> HttpExchangeConnection
  -> upstream Pipeline: Decode -> Display -> Rules -> Encode
  -> HTTP request policy / upstream connector / upstream TLS
  -> downstream Pipeline: Decode -> Display -> Rules -> Encode
  -> HTTP response policy
  -> App writer
```

- 请求没进来：查 Listener plan、bind、downstream TLS；
- 收到但没有 Document：查 Decode/协议包 runtime；
- Document 正确但发送字节错误：查 Document Rules、Encode 和 HTTP 基础动作；
- 上游已回包但 App 未收到：查 downstream Pipeline、response policy 和 write timeout；
- UI 无记录：查 `exchange::ui` tracing、ObservationStore 和 UI 失效事件，不先改网络流程。

### 3.2 Socket

```text
Listener accept
  -> downstream TCP/TLS
  -> Exchange
       Direct/Transparent: chunk -> write
       Scripted: read -> Frame -> Decode -> Display -> Rules -> Encode -> write
  -> upstream TCP/TLS 或 LocalResponder
  -> 按同样模型返回 App
```

Scripted 模式不支持一次 read 中的第二个 Frame。`NeedMore` 表示继续读取；完整 Frame 后立即发送。
查看失败时先按 `read/frame/decode/rules/encode/write` 阶段分类。

### 3.3 外部协议包

```text
WebSocket /packages
  -> package.register
  -> external registry identity/generation
  -> capability factory
  -> hooks.upstream/downstream 或 document.*.display RPC
```

同时对齐 package id/version、connection generation、JSON-RPC request ID、method 和 Exchange ID。
不要把“WebSocket 在线”当成“某次 Frame/Decode 已成功”。

### 3.4 Android

```text
Desktop Application
  -> ADB control + runtime route prepare
  -> Companion VpnService/TUN
  -> JNI Rust data plane
  -> tun2proxy -> SOCKS5 -> route table
  -> adb reverse/LAN Listener 或 protect(fd) 原目标
```

分别验证设备控制状态、TUN 数据面、路由端点、桌面 Listener 和真实业务响应。

## 4. 快照、锁和取消

- Listener 运行任务只持有启动时构造的不可变 snapshot；
- protocol package 固定精确 id/version，不自动升级或 fallback；
- Document 每个 Frame 独占，不能跨连接复用；
- `runtime_epoch` 隔离一次启动，revision 保护配置并发写；
- 锁只保护短生命周期共享状态，不能跨真实网络/JNI `await`；
- 连接关闭、Listener stop、应用退出必须传播 CancellationToken 并释放端口、任务和容量；
- 业务 worker panic 不能被当作正常关闭；观测消费者失败不能阻断交易。

## 5. 错误与日志

新增错误应有稳定 code、中文用户说明和可定位字段。前端展示 Rust ViewModel，不解析 message 判断状态。

`tracing` 字段优先记录 Workspace、Listener、runtime epoch、connection/exchange ID、direction、stage、
package identity、RPC request ID、字节数、耗时、结果分类和稳定错误码。

Exchange 可逆报文通过结构化 observation 保存，不要让诊断代码从普通 fmt 日志恢复 payload。Display、
UI 事件或日志持久化失败必须 fail-open；Frame/Decode/Rules/Encode/transport 失败必须 fail-closed。

## 6. 注释和文件组织

注释解释“为什么”和所有权，重点包括：

- App/Proxy/Server 的数据来源和去向；
- TLS 两条连接及四类证书用途；
- prepare/commit/rollback/uncertain 的补偿原因；
- snapshot、revision、epoch 和 generation 的区别；
- fail-open/fail-closed 选择；
- 看似可合并但会破坏协议语义的边界。

手写 Rust、TypeScript/TSX、Kotlin/Java 文件必须小于等于 500 行。按职责拆分模型、执行、I/O、装配和
测试；不要用跳转式碎片化只为规避行数门禁。

## 7. 测试分层

### 7.1 领域单元测试

优先覆盖规则排序、Schema/Document 类型、Workspace/Listener/TLS 拓扑、Android Profile 和并发身份。

HTTP 规则能力验证必须固定以下矩阵：

- 请求阶段不出现响应状态或响应故障；
- 响应阶段不出现 request terminal；
- TLS 只允许证书条件、第 N 次命中和拒绝握手；
- 限速/间歇方向由请求 upstream、响应 downstream 固定；
- 终止动作唯一且位于动作列表末尾。

### 7.2 adapter/runtime 测试

覆盖短读写、EOF、取消、超时、背压、panic、资源清理、SQLite 回滚、外部包断线、TLS/mTLS 组合和
Exchange 事件顺序。不能只验证 happy path DTO。

### 7.3 前端测试

前端只验证 ViewModel 渲染、用户意图和事件刷新：

- 能力列表来自 Rust；
- 保存中/失败后控件状态恢复；
- overlay 可关闭且不会永久阻止再次新建；
- HTTP/Socket 统一列表仍保留协议标识；
- ExchangeObservationChanged 到达后当前页面实时刷新；
- Display HTML 只进入清洗后的 sandbox iframe。

### 7.4 真实应用测试

自动化通过后，用打包 App 验证：

1. HTTP LocalResponder 和真实 Server 转发；
2. Socket Direct、Scripted、LocalResponder 和透明转发；
3. TCP、TLS、HTTPS、mTLS 的上下游组合；
4. 本地与远端 WebSocket Sidecar 包；
5. 规则实际改写、Decode/Display、四方向 Received/Sent；
6. 连接断开后 Closed/Failed 是否追加；
7. Android ADB reverse/LAN、目标 App、非目标 App 和停止恢复。

真实设备/真实 Server 结果单独记录，不用单元测试或“端口 listening”替代。

## 8. 常用验证命令

### 8.1 快速定向验证

```bash
pnpm lint
pnpm typecheck
pnpm test:ui-contracts
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-domain
```

修改具体 crate 时用 `-p <crate>` 运行其测试，并为失败路径增加定向测试。

### 8.2 架构与代码质量

```bash
pnpm scan:architecture
pnpm scan:source-size
pnpm check:rust:clippy
pnpm check:rust:windows
git diff --check
```

这些门禁检查 crate/前端边界、架构文档、Socket relay 边界、死代码抑制、文件行数、Clippy 和
Windows Rust 编译面。

### 8.3 全量门禁

```bash
pnpm check
```

该命令依次生成 Rust-TypeScript bindings，执行架构/文件大小门禁、lint、typecheck、UI 契约和全量
前端测试、Next production build、bundle branding、Rust fmt/clippy/Windows check 与 workspace tests。

### 8.4 覆盖率

```bash
pnpm test:coverage-policy
pnpm check:coverage:frontend
pnpm check:coverage:rust
# 或
pnpm check:coverage
```

Rust coverage 需要固定 `cargo-llvm-cov 0.8.7` 和 `llvm-tools-preview`。覆盖率阈值不能代替行为矩阵。

### 8.5 Android Companion

```bash
pnpm build:android-companion
pnpm verify:android-companion
```

脚本执行 Companion 测试、lint、Release build、签名/对齐校验并同步 APK。调试单个模块时可在
`android-companion` 内运行对应 Gradle task，但发布证据以仓库脚本为准。

### 8.6 桌面发布构建

```bash
pnpm tauri:build
```

它会先构建 Android Companion，再执行 Tauri release build。构建成功只证明产物可生成；仍需按本节
真实应用测试验证连接、转发和业务响应。

## 9. 评审清单

- 领域规则是否只有一个权威实现？
- HTTP 与 Socket 是否在共享模型下保持协议差异？
- 一个 App connection 是否只有一个 Exchange？
- 请求与响应是否严格配对，是否意外预读下一笔？
- 配置是否以不可变 snapshot 进入运行时？
- revision/epoch/generation 是否各司其职？
- TLS downstream/upstream 和证书用途是否分离？
- 业务失败是否 fail-closed，观测失败是否 fail-open 且有损失计数？
- SQLite/文件/ADB 多步骤失败是否回滚或保留可恢复 ownership？
- 前端是否只提交意图并展示 Rust 结果？
- 文件是否职责单一且不超过 500 行？
- 定向测试、全量门禁、App 构建和真实连接证据是否分别完成？
