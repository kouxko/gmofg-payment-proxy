# Intercept Proxy 产品需求与代码设计 v2.0

> 状态：实施基线
>
> 产品：Intercept Proxy
>
> 原则：Rust 是全部业务能力的唯一实现，Next.js 只展示 Rust ViewModel 和提交用户意图。

## 1. 产品目标

Intercept Proxy 是与具体业务应用无关的 HTTP/HTTPS 测试代理。它同时提供：

1. 标准 HTTP 正向代理和 HTTPS CONNECT 隧道。
2. 用户显式允许目标的 HTTPS MITM。
3. 固定上游的反向代理、TLS/mTLS、抓包、断点、规则和故障注入。
4. Android Companion，通过 `VpnService` 只接管指定包名，在 TCP/IP 包层实施弱网。
5. UI 无关的 Rust Host，可被桌面、自动化测试以及未来 TUI/CLI 复用。

新应用不兼容旧数据，不读取旧数据库、证书目录、Keychain 或 DPAPI 命名空间。旧应用与
Intercept Proxy 可以并存。

## 2. 应用身份与默认状态

| 项目 | 固定值 |
| --- | --- |
| 产品名 | `Intercept Proxy` |
| Tauri identifier | `com.interceptproxy.desktop` |
| npm package | `intercept-proxy` |
| Rust crate 前缀 | `intercept_proxy_*` |
| SQLite | `intercept-proxy.sqlite3` |
| Secret namespace | `com.interceptproxy.desktop` |
| Android package | `com.interceptproxy.vpn` |

首次启动创建空 Workspace，以及一个关闭的 `127.0.0.1:8080` 正向代理草稿。不得创建反向
监听器、业务规则、故障模板、Shift-JIS 策略或业务断言。安装包不得携带固定业务 URL、端口、
客户端 P12、业务 CA、业务返回码或测试私钥。

## 3. 架构边界

```text
Next.js + HeroUI
       │ Tauri Command / Channel
       ▼
src-tauri（薄适配）
       ▼
application（Use Case、ViewModel、Port）
       ▼
domain（Workspace、规则、状态机）
       │
       ├── proxy（HTTP/TLS/CONNECT/MITM）
       ├── infrastructure（SQLite、证书、系统密钥、ADB、文件）
       └── android-engine（TUN 包调度和弱网）
```

### 3.1 Rust 边界

- `domain` 不依赖 Tauri、SQLite、WebView 或 Android Framework。
- `application` 只依赖领域类型和抽象 Port，负责校验、筛选、分页和中文错误 ViewModel。
- `proxy` 实现网络协议和连接生命周期，不读取 UI 状态。
- `infrastructure` 实现持久化、系统密钥、证书文件、ADB 和平台适配。
- `android-engine` 是纯 Rust 包调度器；相同 seed 和输入必须产生相同决策。
- `host` 是唯一组合根；Tauri 与未来 TUI/CLI 使用相同 Host。
- `src-tauri` 不放置规则、证书、代理、ADB 或弱网业务逻辑。

### 3.2 前端边界

前端只允许：

- 显示 Rust ViewModel。
- 调用生成的 Command。
- 订阅 Channel。
- 保存当前 Tab、弹窗、选中行、未提交输入和滚动位置。

前端禁止：

- `fetch`、WebSocket、Node 文件/网络/密码 API。
- `localStorage`、`sessionStorage`、IndexedDB 和 Cookie 业务存储。
- 规则匹配、编码转换、Header/JSON 解析、分页、排序或业务状态推断。
- 直接调用 ADB、读取 APK、证书、P12 或私钥。
- 手写与 Rust 重复的 DTO。

全部生产控件使用 HeroUI 官方组件。Next.js 采用静态导出，由 Tauri 加载，不运行 Node 服务。

## 4. Workspace 模型

```text
ProxyWorkspace
├── id / name / revision
├── listeners[]
├── body_codec_policies[]
├── metadata_extractors[]
├── response_assertions[]
├── rules[]
├── fault_presets[]
└── certificate_references[]
```

### 4.1 代理入口（代码模型名：Listener）

用户界面统一使用“代理入口”，不得直接把英文 `Listener` 作为页面名称或主要操作文案。
“代理入口”表示 Proxy 在本机开放给客户端连接的地址和端口，以及该入口采用的转发方式。

`ForwardProxyListener`：

- 监听地址、认证策略、客户端 CIDR、CONNECT 策略和 MITM allowlist。
- HTTP Basic 用户名/密码只通过 `workspace_secret_store_basic` 交给 Rust；Rust 使用当前用户
  Keychain/DPAPI 保护完整认证值，Workspace 只保存 `SecretReference`。
- 运行时仅在 Listener 启动时解密到自动清零内存，并以常量时间 MAC 校验请求凭据；错误、
  事件和抓包不得包含明文或认证 Header。
- 默认只能监听 loopback。
- 非 loopback 必须同时配置认证与 CIDR allowlist。
- HTTP absolute-form 转换为 origin-form。
- 删除 Proxy-Authorization 和 hop-by-hop Header。
- CONNECT 默认为 Tunnel；只有 allowlist 命中才进入 MITM。

`ReverseProxyListener`：

- 任意数量；每一条都是独立的“本地监听地址/端口 -> 固定上游 HTTP/HTTPS origin”映射。
- 同一 Workspace 可同时配置不同端口、不同主机的多个上游，例如 Transaction 与 DLL
  分别使用各自的本地端口和上游 URL；不得把多个 origin 塞入同一 Listener 后由前端猜测路由。
- 可选下游 TLS/mTLS。
- 可选上游 P12 客户端身份、显式 CA 和主机名校验。
- 下游客户端认证必须按入口支持 `disabled`、`optional`、`required`，不得假设 Android
  或其他客户端一定持有客户端证书；上游客户端身份同样可为空，以支持普通单向 TLS。
- 每条 Reverse Listener 独立引用证书材料。不同入口可以使用完全不同的下游服务端身份、
  下游客户端信任、上游客户端身份和上游 CA，禁止使用一套全局证书覆盖全部入口。
- 入口页面必须提供“测试上游 TLS 握手”。Rust 使用当前入口已保存的上游 origin、CA、
  主机名策略和可选客户端身份执行真实 TCP + TLS 握手，但不发送 HTTP 业务请求；成功结果
  返回解析地址、耗时、TLS 版本、密码套件、Server 证书主题和 SHA-256 指纹，失败返回稳定
  中文错误和建议操作。证书文件格式校验不能替代该测试。
- 独立 Body codec、提取器和响应断言。

#### 4.1.1 UI 与运行时唯一来源

- “入口配置”是监听地址、端口、请求去向、下游 TLS、上游 TLS、保存和启停的唯一 UI。
- “系统设置”只包含全局超时、Body 上限、会话容量、内存容量、数据和应用策略；不得重复
  展示或保存入口字段，也不得提供“保存并重启全部代理”。
- “运行监控”和顶部状态栏只展示 Rust 将当前 Workspace 与实际 Listener 运行状态合并后的
  `ListenerOverviewViewModel`，不得读取旧静态产品通道目录或在 TypeScript 中补齐停止状态。
- 工作区没有入口时显示“未配置入口”；入口停止、启动中、运行中、停止中和故障均由 Rust
  返回稳定状态文案与 UI tone。
- 应用退出时 Rust 必须停止全部动态 Listener 任务；旧全局代理适配器不得成为通用 UI 的第二套
  网络生命周期。

### 4.2 编码、提取和断言

- Codec：Raw、严格 UTF-8、严格 Shift-JIS。
- Extractor：Header、JSONPath、文本、固定值。
- Assertion：HTTP 状态、Header、JSONPath、文本、长度、SHA-256。
- 未修改 Body 使用原始字节透传。
- 修改后由 Rust 重新编码并重算 Content-Length。
- 目标编码不能表示字符时禁止发送，并返回字段错误。

### 4.3 导入导出

Workspace 文件扩展名为 `.intercept-workspace`。导出只能包含配置和秘密引用，不得包含：

- 私钥。
- P12 原文或密码。
- 系统密钥密文。
- 完整抓包 Payload。

## 5. HTTP、CONNECT 与 MITM

- HTTP/1.1 正向请求支持 absolute-form。
- CONNECT authority 必须是合法主机和端口，禁止 userinfo、路径、query 和 fragment。
- 隧道支持背压、双向复制、half-close、空闲超时和 CancellationToken。
- 每安装实例生成独立 Root CA，私钥由当前用户 Keychain/DPAPI 保护。
- UI 只能导出公开 Root CA。
- MITM 叶子按 authority/SNI 动态签发，内存最多缓存 256 个。
- HTTP/2、HTTP/3 和 QUIC 首版只允许 Tunnel，不解析和修改。
- WebSocket 记录 HTTP/1.1 Upgrade 握手，之后帧流透明转发。

## 6. 抓包、断点与规则

阶段：`Connection`、`HttpRequest`、`HttpResponse`。

连接动作：延迟、拒绝、限速、间歇传输、指定字节后断开、half-close、空闲超时。

HTTP 动作：Header 增删改、文本替换、JSONPath 修改、Mock、状态码、延迟、抖动、限速、
错误 Content-Length、截断、断点和丢弃响应。

组合规则：

- Rust 按优先级排序，同优先级按创建顺序。
- 修改、延迟和暂停可以组合。
- Mock、拒绝、断开、丢弃和截断是终止动作。
- 第 N 次命中计数范围由规则明确指定；默认按客户端身份与目标组合计数。
- 规则关闭后重新启用或匹配条件变化时重置计数。
- 每次评估保留轨迹，前端只展示。

报文详情只使用“概览、请求、响应”三个 Tab。HTTP 状态与完整 Header 显示在请求/响应详情
内部，不建立 Header Tab。完整 Payload 只在打开详情时按 ID 获取，关闭后释放引用。

## 7. 证书与秘密

- Root CA、叶子私钥、P12 原文和密码不进入前端。
- SQLite 只保存元数据和受保护密文。
- macOS 使用当前用户 Keychain；Windows 使用当前用户范围 DPAPI。
- 不使用 LOCAL_MACHINE 范围。
- Reverse Listener 可以分别引用下游服务端身份、客户端信任、上游 P12 和上游 CA。
- 普通 TLS 与 mTLS 均为按入口选择：客户端或 Server 未要求双向认证时，不得强制配置
  客户端证书；真实握手测试必须使用该入口的实际选择验证 Server 兼容性。
- Forward MITM 只引用安装实例 Root CA。
- 重置 Root CA 是危险操作，代理必须停止且用户确认。

## 8. Android Companion

### 8.1 平台与控制

Companion APK 包名为 `com.interceptproxy.vpn`。桌面包携带 APK，但不携带 platform-tools。
Companion 使用仓库内固定的项目升级签名身份；该身份用于保证不同开发机和 CI 构建可以
覆盖安装同一个包，不被当作保密发布凭据。私有 keystore 不进入桌面安装包，桌面资源只
包含已经签名并通过包名、唯一 signer、固定证书指纹、四 ABI 和 16 KiB 对齐门禁的 APK。
CI 必须先完成 Companion release APK 的构建与门禁，再启动 Windows 或 macOS 桌面打包；
桌面任务不得自行生成、回退或替换 Companion APK。
桌面只调用系统 `adb`，并允许用户选择设备 serial。

控制通道使用：

1. `adb forward tcp:0 localabstract:intercept_proxy_vpn`。
2. 版本化、长度前缀 JSON。
3. Android 服务端使用 `SO_PEERCRED`，只允许 shell/root。

Kotlin 只负责授权 Activity、VpnService、通知、BootReceiver、TUN、allowlist、JNI 和
`protect(fd)`。所有配置校验、随机、调度、状态和统计由 Rust 完成。

### 8.2 定向应用范围

- 桌面“设备弱网”页面固定使用左右两栏：左侧展示本机连接工具、目标设备与设备端控制；
  右侧展示弱网方案、目标应用、目标地址和全部弱网参数。
- 目标应用支持按包名筛选。前端只提交关键字，筛选、长度限制和结果排序由 Rust 完成。
- Profile 最多选择 64 个包。
- 一个 Profile 可配置 0 到 128 个远端 IP/CIDR 目标，每个目标可指定端口集合；空列表
  表示所选应用访问的全部原始目标，绝不能把应用限制为单一 Server。
- 多个目标按“任一命中”执行弱网；未命中的连接仍经过 fail-open 转发但不实施故障。
- TUN 包不携带可靠域名，因此首版地址范围只接受 IPv4/IPv6 或 CIDR；HTTP 域名级
  选择继续由 Forward Proxy/MITM allowlist 和 HTTP 规则负责。
- 只对选中包调用 `addAllowedApplication`。
- Companion 自身禁止选择。
- 未选择应用、系统网络和 ADB 不进入 VPN。
- 保存包名、签名 SHA-256、shared UID 和显示名快照。
- shared UID 只选部分包时拒绝启动；选中整组并确认后才允许。
- shared UID 组只提供聚合统计。
- 包卸载或签名变化立即停止；同签名升级重新构建 allowlist。

### 8.3 Rust 数据面

```text
Selected App → VpnService TUN → Rust ImpairedTun
             → tun2proxy 0.8.3 → local Rust SOCKS5
             → protect(fd) → original destination
```

- 固定 `tun2proxy = "=0.8.3"`。
- 支持 IPv4/IPv6、TCP、UDP、SOCKS5 CONNECT 和 UDP ASSOCIATE。
- 上行使用原始 destination、下行使用原始 source 作为远端地址，保证同一目标的
  双向流量命中同一组多地址规则。
- 不持久化 Payload。
- 统计只包含方向、协议、长度、TCP flag/sequence 摘要和计数。

### 8.4 弱网能力

- 固定延迟和均匀抖动。
- 上下行 token-bucket 限速。
- 随机丢包和 Gilbert-Elliott 突发丢包。
- 重复、乱序和指定窗口黑洞。
- DNS 53/853 黑洞。
- 第 N 个 SYN/SYN-ACK/ACK/FIN/RST 丢弃。
- MTU/MSS clamp、IPv4 fragmentation-needed、IPv6 PTB、PMTU blackhole。
- TCP/UDP Payload bit corruption。
- 固定 seed、无限运行和开机恢复。

### 8.5 安全恢复

- Android 通知、VPN 设置和桌面 ADB 都能停止。
- Rust/JNI/TUN 失败立即关闭 TUN并 fail-open。
- 5 分钟失败 3 次后禁止自动恢复。
- 用户主动停止后保持停止。
- 重启后等待用户解锁、网络可用，再等待 30 秒恢复。
- 不启用 lockdown kill switch。
- 100% 丢包和全黑洞必须二次确认。
- 发现作用域越界立即关闭 TUN。

## 9. IPC

Command：

```text
workspace_list/get/create/copy/select/validate/save/delete/import/export
workspace_component_new
workspace_secret_store_basic
listener_list/get/save/delete/start/stop
certificate_overview/generate_ca/export_ca/import/reissue/validate
android_adb_get/select
android_device_list
android_package_list/get
android_companion_install/update
android_vpn_open_consent
device_network_target_validate
device_network_profile_list/get/save/delete
device_network_start/apply/stop/emergency_restore/status
```

Channel：

```text
WorkspaceChanged
ListenerStatusChanged
CaptureRowsAdded
SessionUpdated
BreakpointQueued
BreakpointResolved
RuleHit
CertificateStatusChanged
AndroidDeviceChanged
AndroidVpnStatusChanged
AndroidNetworkStatsUpdated
ResourceWarning
OperationFailed
```

所有 Command 返回 `Result<ViewModel, AppErrorViewModel>`。错误由 Rust 提供稳定错误码、中文消息、
字段错误、可重试状态和建议操作。

## 10. 测试与发布门禁

### 10.1 自动化

- Workspace 任意监听器、敏感字段排除和 revision 冲突。
- HTTP absolute-form 与 CONNECT 各连续 100 次。
- MITM allowlist 命中/未命中。
- 非 loopback 无认证配置拒绝。
- Reverse TLS/mTLS、Raw/UTF-8/Shift-JIS。
- 全部规则和终止动作。
- Android IPv4/IPv6、TCP/UDP 和全部弱网动作。
- 目标应用 100% 丢包时，两款非目标应用和 ADB 正常。
- shared UID、卸载、签名变化、同签升级、重启和 fail-open。
- Windows/macOS 构建；Android ABI、签名和 16 KiB page size。

### 10.2 Android 架构门禁

正式扩展前必须证明：只接管目标应用、非目标应用正常、Companion 绕过、ADB 正常、共享 UID
拒绝部分选择、双栈 TCP/UDP 转发、指定 TCP sequence 丢弃产生真实重传、停止后 5 秒恢复。

### 10.3 真实上游兼容验收

具体业务测试不得进入产品默认模板。测试人员从空 Workspace 手工配置两个 Reverse Listener、
TLS/mTLS、P12、CA、Shift-JIS 和通用响应断言。测试配置与报告不得被桌面安装包打包。

在 A920MAX `2740072778` 上，DLL 请求必须经过 Intercept Proxy 到达真实上游，并将真实响应
完整返回设备。当前测试环境的必需基线标记为 `D48`；它表示真实上游往返完成，不代表业务
授权成功。通过报告必须分别提供：

1. 下游与上游 TLS/mTLS。
2. HTTP 状态、完整 Header 和响应 Body。
3. 设备端真实 `D48`。
4. 非目标应用隔离。

Mock、固定返回、仅 HTTP 200 或仅抓到报文不能代替该验收。

### 10.4 Android 模拟器代理回归

CI/开发机必须提供一个只存在于 `test-support` 和 `androidTest` 的类 DLL 场景：模拟器客户端
经 `adb reverse` 进入正式 `ApplicationHost`、空 Workspace 中临时创建的 Reverse Listener，
再到本地独立 upstream fixture。fixture 可以返回测试期望 `D48`，但必须同时验证：

1. Android 客户端观察到 HTTP 状态、完整 Header 和 Shift-JIS 解码后的 `D48`。
2. Rust 会话详情的通用响应断言通过。
3. 未修改响应 Body 与上游 fixture 的字节完全一致。
4. 配置和结果只写测试临时目录，不进入安装包或首次启动数据。

该回归证明“Android 客户端 → 动态 Reverse Listener → 上游 → 原样返回”的通用能力，
不能替代 10.3 的 A920MAX、真实证书、真实上游和真实业务响应。

## 11. 需求追踪矩阵

| 需求 | UI | Rust Use Case / 模块 | IPC | 测试 |
| --- | --- | --- | --- | --- |
| Workspace | Workspace/Listener 页面 | application + domain | `workspace_*`, `listener_*` | workspace roundtrip |
| 入口配置唯一来源 | 入口配置/运行监控/顶部状态栏 | application listener overview | `listener_overview`, listener events | overview + UI boundary |
| Forward/CONNECT | Listener 状态与抓包 | proxy | listener/status events | 100× HTTP/CONNECT |
| MITM | 证书与 allowlist | proxy + infrastructure | `certificate_*` | tunnel/MITM split |
| Reverse TLS/mTLS | Listener/证书 | proxy + infrastructure | listener/certificate | TLS matrix |
| 规则与断点 | 规则/断点/详情 | application + proxy | rule/breakpoint | rule semantics |
| Android 定向 VPN | Android 弱网页 | application + android-engine | `android_*`, `device_network_*` | scope gate |
| 弱网 | Profile/实时统计 | android-engine | profile/status events | deterministic vectors |
| 真实上游兼容 | 无默认业务 UI | generic reverse stack | existing generic IPC | real-device report |

## 12. 实施和停止条件

按身份与存储、Workspace、Forward/CONNECT、MITM、通用 UI、Android 架构门禁、完整弱网、
真实设备兼容、跨平台 CI 的顺序实施。只有通用代理、Android 定向弱网和真实设备兼容三类
验收全部通过，版本才可以发布。
