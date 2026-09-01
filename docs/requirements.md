# Intercept Proxy 产品需求与代码设计 v2.0

> 状态：实施基线
>
> 产品：Intercept Proxy
>
> 原则：Rust 是全部业务能力的唯一实现，Next.js 只展示 Rust ViewModel 和提交用户意图。

## 1. 产品目标

Intercept Proxy 是与具体业务应用无关的 HTTP/HTTPS/Socket 测试代理。它同时提供：

1. 统一代理监听；普通 HTTP 可按请求目标转发，也可转发到固定 Server。
2. 固定 Server 的 TLS/mTLS，以及独立的 Socket 协议处理或透明转发。
3. 每条监听可选固定 Server、TLS/mTLS、抓包、规则和故障注入。
4. Android Companion，通过 `VpnService` 只接管指定包名，在 TCP/IP 包层实施弱网。
5. UI 无关的 Rust Host，可被桌面、自动化测试以及未来 TUI/CLI 复用。

数据库版本 `100` 是产品 `1.00` 的正式兼容基线。合法 Schema 标记 `<100` 的发布前开发数据库在
启动时清空并按版本 `100` 重建；从版本 `100` 开始，后续升级必须使用显式兼容迁移。当前程序遇到
未来版本、缺失/重复标记或损坏数据库时 fail-closed，不得清空或改写。旧应用与 Intercept Proxy
可以并存；证书目录、Keychain 和 DPAPI 命名空间仍只按当前产品身份读取。

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

首次启动创建空 Workspace，以及一个关闭的 `127.0.0.1:8080` 代理监听草稿；该草稿默认按请求
目标转发且不配置固定 Server。不得创建其他监听器、业务规则、故障模板、Shift-JIS 编码配置或
业务断言。安装包不得携带固定业务 URL、端口、
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
       ├── proxy（HTTP/TLS/Socket/Exchange）
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
├── metadata_extractors[]
├── response_assertions[]
├── rules[]
├── fault_presets[]
├── certificate_references[]
└── android_network_profiles[]
```

### 4.1 代理入口（代码模型名：Listener）

用户界面统一使用“代理入口”，不得直接把英文 `Listener` 作为页面名称或主要操作文案。
“代理入口”表示 Proxy 在本机开放给客户端连接的地址和端口，以及该入口采用的转发方式。

所有入口使用同一个 `ProxyListener` 模型，不再把“正向代理”和“固定上游入口”作为两种
需要用户理解的入口类型。每个入口都包含监听地址、端口和一个可选的固定 Server 配置：

- 未启用“转发到固定 Server”时，Rust 根据普通 HTTP absolute-form 动态确定请求目标。
- 启用“转发到固定 Server”时，该入口的全部 HTTP 请求只允许转发到本入口配置的唯一
  HTTP/HTTPS origin。不同 Server、端口或证书策略必须使用不同入口，入口数量不受产品模板限制。
- 固定 Server 是入口的可选路由策略，不是另一种“上游入口”，UI、IPC、Workspace 和运行状态
  均不得再暴露 Forward/Reverse 两套入口概念。

按请求目标转发时：

- 支持认证策略。当前 HTTP CONNECT/Upgrade 在创建 Server connection 和
  Exchange 前统一返回 501；配置模型中即使仍保留相关字段，也不代表运行时已经提供隧道或 MITM。
- HTTP Basic 用户名/密码只通过 `workspace_secret_store_basic` 交给 Rust；Rust 使用当前用户
  Keychain/DPAPI 保护完整认证值，Workspace 只保存 `SecretReference`。
- 运行时仅在 Listener 启动时解密到自动清零内存，并以常量时间 MAC 校验请求凭据；错误、
  事件和抓包不得包含明文或认证 Header。
- 默认只能监听 loopback。
- Listener 不限制客户端 IP；非 loopback 正向代理必须配置认证。
- HTTP absolute-form 转换为 origin-form。
- 删除 Proxy-Authorization 和 hop-by-hop Header。
- CONNECT/Upgrade 当前不会进入 Tunnel、MITM 或透明转发。

启用固定 Server 时：

- 每一条都是独立的“本地监听地址/端口 -> 固定 Server HTTP/HTTPS origin”映射。
- 同一 Workspace 可同时配置不同端口、不同主机的多个上游，例如 Transaction 与 DLL
  分别使用各自的本地端口和上游 URL；不得把多个 origin 塞入同一 Listener 后由前端猜测路由。
- 可选下游 TLS/mTLS。
- 可选上游 P12 客户端身份、显式 CA 和主机名校验。
- 下游客户端认证必须按入口支持 `disabled`、`optional`、`required`，不得假设 Android
  或其他客户端一定持有客户端证书；上游客户端身份同样可为空，以支持普通单向 TLS。
- 每条 Listener 独立引用证书材料。不同入口可以使用完全不同的下游服务端身份、
  下游客户端信任、上游客户端身份和上游 CA，禁止使用一套全局证书覆盖全部入口。
- 下游服务端身份留空时必须使用证书管理页签发并受系统密钥保护的本机叶子证书；仅在当前
  Listener 确实需要另一套 Server 身份时，才导入包含证书链与私钥的独立 PEM。
- 下游独立服务端 PEM、下游客户端 CA、上游客户端 P12 与上游 Server CA 都必须通过 Rust
  原生文件对话框导入受保护存储。UI 不得创建或编辑裸文件路径引用；已有外部引用失效时必须
  显示明确错误，并允许恢复为本机叶子证书或重新导入对应材料。
- 上游 Server CA 的单文件 PEM 可以按顺序包含多张 CA；Rust 必须逐张验证、规范化、完整持久化并
  在 TLS Trust Store 中加载全部成员。不得增加多文件选择或自动合并；单证书 PEM/DER 保持兼容。
- Server CA 与可选 P12 客户端身份必须在目标 Listener 的“固定 Server”编辑区导入和选择；
  导入后 Rust 只把受保护材料的安全引用写入 Workspace。证书管理页不得提供全局上游
  CA/P12 配置，以免用户误认为所有入口共享一套上游 TLS 身份。
- 入口页面必须提供“测试 Server TLS 握手”。Rust 校验当前入口草稿，并使用草稿中的固定 Server
  origin、CA、主机名策略和可选客户端身份执行一次临时 TCP + TLS 握手，但不保存 Workspace、
  不启停任何 Listener，也不发送 HTTP 业务请求；成功结果
  返回解析地址、耗时、TLS 版本、密码套件、Server 证书主题和 SHA-256 指纹，失败返回稳定
  中文错误和建议操作。证书文件格式校验不能替代该测试。
- 独立 Body codec、提取器和响应断言。
- 固定 Server 未启用或 origin 不是 HTTPS 时，不显示 CA、客户端身份和 TLS 握手测试。
- 固定 Server 的 mTLS 是可选能力：普通 HTTPS 不选择客户端身份；只有 Server 明确要求
  客户端证书时才选择受保护的 P12 引用。
- 固定 Server 模式不得通过请求中携带的其他 authority 绕过配置目标。CONNECT/Upgrade 在连接
  固定 Server 前返回 501，并留下可追踪结果。

#### 4.1.1 UI 与运行时唯一来源

- “入口配置”是监听地址、端口、请求去向、下游 TLS、上游 TLS、保存和启停的唯一 UI。
- Listener 的保存、删除、启动、停止和 TLS 测试均以单条 Listener 为边界。A 运行时必须允许
  保存、删除或启动已停止的 B，也必须允许使用 B 的未保存草稿执行 TLS 测试；只禁止修改或
  删除正在运行的目标 Listener。聚合 `workspace_save` 不得作为入口页面的保存路径。
- “系统设置”只包含全局超时、Body 上限、会话容量、内存容量、数据和应用策略；不得重复
  展示或保存入口字段，也不得提供“保存并重启全部代理”。
- 顶部状态栏和“入口配置”只展示 Rust 将当前 Workspace 与实际 Listener 运行状态合并后的
  `ListenerOverviewViewModel`，不得读取旧静态产品通道目录或在 TypeScript 中补齐停止状态。
- 工作区没有入口时显示“未配置入口”；入口停止、启动中、运行中、停止中和故障均由 Rust
  返回稳定状态文案与 UI tone。
- 应用退出时 Rust 必须停止全部动态 Listener 任务；旧全局代理适配器不得成为通用 UI 的第二套
  网络生命周期。

### 4.2 编码、提取和断言

- 每个代理入口分别配置请求正文编码和响应正文编码，不通过 Workspace 级策略间接引用。
- 可选编码：Raw、严格 UTF-8、严格 Shift-JIS。
- Extractor：Header、JSONPath、文本、固定值。
- Assertion：HTTP 状态、Header、JSONPath、文本、长度、SHA-256。
- 未修改 Body 使用原始字节透传。
- 修改后由 Rust 重新编码并重算 Content-Length。
- 目标编码不能表示字符时禁止发送，并返回字段错误。

### 4.3 导入导出

Workspace 文件扩展名为 `.intercept-workspace`。它必须完整包含当前 Workspace
的入口（含请求和响应正文编码）、元数据提取、响应断言、规则、故障预设、证书安全引用以及
Android 设备网络方案及其透明代理路由与弱网参数。

整个应用的备份文件扩展名为 `.intercept-config`，包含：

- 全部 Workspaces（每个 Workspace 包含上述全量可移植配置）。
- 当前选中的 Workspace ID。
- 全局 Settings：超时、Body/会话/内存容量、数据导出策略和 Host 重写等可移植值。

两类导出都必须是单文件可移植文档。除结构化配置外，Rust 可以嵌入导出范围内 Listener 所引用证书材料的副本：

- 导入的下游 Listener 服务端身份、证书链和私钥。
- 下游客户端信任 CA 与上游 Server 信任 CA。
- 上游 mTLS PKCS12/PFX 原文及其明文密码。

这些材料只用于在受控测试环境中恢复 Listener，而不再依赖原外部文件。运行时 Workspace 对象和数据库仍只保存托管引用与受保护秘密。两种导出都必须先显示危险确认，并明确说明文件内容。

两类导出都不得包含：

- 本机 MITM Root CA 私钥或其受保护存储 envelope。
- HTTP Basic 明文密码或系统密钥密文。
- 完整抓包 Payload。
- Android 设备 serial、ADB 转发端口、VPN 授权或运行状态。
- 抓包、会话、统计、运行任务或临时桌面/Android 网络端点。

导入必须先完成 schema、版本、结构引用完整性、内嵌材料大小/哈希/格式/用途校验和禁止字段扫描，
全部通过后才原子替换。Rust 将内嵌 Listener 证书材料恢复到目标机器的受保护存储，并重写托管引用。任何导入失败都不得部分修改当前 Workspace、全局 Settings、当前选择或受保护证书存储。

## 5. HTTP 与当前 CONNECT/Upgrade 边界

- HTTP/1.1 正向请求支持 absolute-form。
- 普通 HTTP 按请求目标或固定 Server 路由，并进入 HTTP Reader/Writer Pipeline。
- CONNECT 与 HTTP Upgrade 当前统一返回 501；不会建立 Server connection，不会创建 Exchange，
  也不会隐式回退到 tunnel、MITM 或 WebSocket 帧透明转发。
- 证书管理、下游 TLS 和固定 Server TLS/mTLS 是当前独立能力，不表示 CONNECT MITM 已启用。
- 后续若支持 CONNECT tunnel、MITM 或 Upgrade，必须通过新的 ADR、生命周期测试和安全边界评审，
  不能仅靠启用已有配置字段改变生产行为。

## 6. 抓包与规则

规则阶段：`Proxy -> Server`、`Proxy -> App`。

连接动作：延迟、拒绝、限速、间歇传输、指定字节后断开、half-close、空闲超时。

HTTP 动作：Header 增删改、文本替换、JSONPath 修改、Mock、状态码、延迟、抖动、限速、
错误 Content-Length、截断和丢弃响应。

规则合同：

- Rust 按优先级排序，同优先级按规则 ID 排序。
- 每条规则必须且只能包含一个条件和一个对应动作。
- 多条规则分别独立匹配并按顺序执行各自唯一动作；当前不提供单条规则内的 AND/OR 条件组合。
- 规则只由显式启用开关控制，不存在单次命中或第 N 次命中生命周期。
- 每次评估保留轨迹，前端只展示。

报文详情只使用“概览、请求、响应”三个 Tab。HTTP 状态与完整 Header 显示在请求/响应详情
内部，不建立 Header Tab。完整 Payload 只在打开详情时按 ID 获取，关闭后释放引用。

## 7. 证书、诊断与密码学材料

- 用户允许测试报文和完整交易证据用于诊断，不要求产品建立隐私承诺。当前实现仍按已有类型和
  存储边界输出：Exchange observation 可展示实际线路 Payload；普通诊断与持久日志使用有界、
  结构化摘要；MCP 的证书接口只返回公开元数据，不返回私钥、密码或原始密钥库。
- SQLite 只保存配置元数据、受保护密文和外部软件包的安全注册元数据；Exchange Payload、活跃
  RPC 调用状态和第三方内部状态不写入 SQLite。
- macOS 使用当前用户 Keychain；Windows 使用当前用户范围 DPAPI。
- 不使用 LOCAL_MACHINE 范围。
- 每条启用固定 Server 的 Listener 可以分别引用下游服务端身份、客户端信任、Server mTLS
  客户端 P12 和 Server CA。
- 普通 TLS 与 mTLS 均为按入口选择：客户端或 Server 未要求双向认证时，不得强制配置
  客户端证书；真实握手测试必须使用该入口的实际选择验证 Server 兼容性。
- Root CA 与叶子证书材料由证书模块管理；当前不用于 HTTP CONNECT MITM。
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

- 桌面“设备网络”页面按从上到下的单列操作流组织：设备与控制、方案基本信息、
  目标应用、透明代理路由、弱网覆盖范围、弱网参数与运行状态；页面与导航统一使用
  “设备网络 / 应用网络接管 / 设备网络方案”，避免把透明代理能力误称为弱网；不得用左右分栏造成大片留白。
- 目标应用支持按包名筛选。前端只提交关键字，筛选、长度限制和结果排序由 Rust 完成。
- Profile 最多选择 64 个包。
- 一个 Profile 可配置 0 到 128 个远端 IP/CIDR 目标，每个目标可指定端口集合；空列表
  表示所选应用访问的全部原始目标，绝不能把应用限制为单一 Server。
- 这些 `destination_targets` 只决定哪些连接实施弱网，不改变请求去向。多个目标按
  “任一命中”执行弱网；未命中的连接仍 fail-open 直连且不实施故障。
- TUN 包不携带可靠域名，因此首版地址范围只接受 IPv4/IPv6 或 CIDR；HTTP 域名级
  选择继续由普通 HTTP 请求目标、固定 Server 路由和 HTTP 规则负责。
- 每个 Profile 可配置 0 到 128 条 `proxy_routes`，每条用“原始目标域名/IP/CIDR +
  一个或多个端口”引用当前 Workspace 中的一条 Listener。
- `proxy_routes.ports` 必须至少包含一个明确端口；透明代理路由不支持空端口集合或
  “全部端口”匹配。弱网覆盖范围的空端口集合仍表示全部端口，两者语义不得混用。
- 业务 App 仍保留原始 Server URL，不填写电脑 IP 或代理端口。Rust 数据面在 TUN 中匹配
  原始目标；命中后才把 TCP 连接透明改送到引用的桌面 Listener。
- 原始域名在每次启动/应用 Profile 时由桌面 Rust 解析 A/AAAA，并把地址快照连同域名一起
  下发；Android 端不得再次依赖设备物理网络 DNS。桌面无法解析时禁止启动，不允许悄然
  绕过。IP/CIDR 直接匹配。
- `proxy_routes` 与 `destination_targets` 独立：前者决定 TCP 是否经桌面代理，后者决定是否
  对该连接注入弱网。不得为了代理而隐式扩大弱网范围。
- 未命中 `proxy_routes` 的所选应用流量使用 `protect(fd)` 连接原始目标；非目标
  应用根本不进入 VPN。Companion 自身和 ADB 不得进入路由表。
- 只对选中包调用 `addAllowedApplication`。
- Companion 自身禁止选择。
- 未选择应用、系统网络和 ADB 不进入 VPN。
- 保存包名、UID、shared UID 和显示名快照，不读取 APK 签名。
- shared UID 只选部分包时拒绝启动；选中整组并确认后才允许。
- shared UID 组只提供聚合统计。
- 目标应用只校验包名、UID 与 shared UID 完整性，不读取或比较 APK 签名。包卸载或 UID 归属变化时立即停止，并在应用升级后重新构建 allowlist。

### 8.3 Rust 数据面

```text
Selected App → VpnService TUN → Rust ImpairedTun
             → tun2proxy 0.8.3 → local Rust SOCKS5
             → proxy route 命中 → 桌面 Listener → 该 Listener 配置的 Server/请求目标
             → proxy route 未命中 → protect(fd) → original destination
```

- 固定 `tun2proxy = "=0.8.3"`。
- 支持 IPv4/IPv6、TCP、UDP、SOCKS5 CONNECT 和 UDP ASSOCIATE。
- 透明桌面代理首版只改送 TCP；UDP 保持原目标直连，不得把 UDP 误送到 HTTP Listener。
- 设备与电脑处于同一 IPv4 子网且 Listener 对 LAN 地址开放时，透明代理
  路由优先使用 LAN；否则桌面 Rust 为每个被引用 Listener 创建
  `adb reverse tcp:<device-temporary-port> tcp:<listener-port>`，Android 只连接
  `127.0.0.1:<device-temporary-port>`。因此设备没有 Wi-Fi/蜂窝外网时仍可经电脑访问上游。
  这些端点只在启动时下发，不写入 Workspace；电脑自身仍须能够访问上游 Server。
- TUN 使用虚拟 DNS，不从 Android 当前物理网络读取 DNS Server。ADB 控制命令使用
  `adb forward`，业务数据使用方向相反的 `adb reverse`；UI 统一称为“USB/ADB 通道”。
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
- 重启后等待用户解锁及桌面 USB/ADB 控制通道恢复，再等待 30 秒恢复；不把 Android
  Wi-Fi/蜂窝网络可用作为透明代理启动前提。
- 不启用 lockdown kill switch。
- 100% 丢包和全黑洞必须二次确认。
- 发现作用域越界立即关闭 TUN。

## 9. IPC

Command：

```text
workspace_list/get/create/copy/select/validate/save/delete/import/export
application_configuration_import/export
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
- 完整应用配置的全量 roundtrip、敏感/运行字段拒绝和导入失败原子性。
- HTTP absolute-form 连续 100 次；CONNECT 与 Upgrade 的 501 边界回归。
- 非 loopback 无认证配置拒绝。
- Reverse TLS/mTLS、Raw/UTF-8/Shift-JIS。
- 全部规则和终止动作。
- Android IPv4/IPv6、TCP/UDP 和全部弱网动作。
- Android 目标应用保持原始 URL 时，多条原始地址/端口可分别透明命中对应 Listener；
  未命中路由直连，弱网范围不得改变路由决策。
- 目标应用 100% 丢包时，两款非目标应用和 ADB 正常。
- shared UID、卸载、UID 归属变化、应用升级、重启和 fail-open。
- Windows/macOS 构建；Android ABI、签名和 16 KiB page size。

### 10.2 Android 架构门禁

正式扩展前必须证明：只接管目标应用、非目标应用正常、Companion 绕过、ADB 正常、共享 UID
拒绝部分选择、双栈 TCP/UDP 转发、指定 TCP sequence 丢弃产生真实重传、停止后 5 秒恢复。

### 10.3 真实上游兼容验收

具体业务测试不得进入产品默认模板。测试人员从空 Workspace 手工配置两个代理 Listener，分别
启用各自的固定 Server，继续配置
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
经 `adb reverse` 进入正式 `ApplicationHost`、空 Workspace 中临时创建并启用固定 Server 的 Listener，
再到本地独立 upstream fixture。fixture 可以返回测试期望 `D48`，但必须同时验证：

1. Android 客户端观察到 HTTP 状态、完整 Header 和 Shift-JIS 解码后的 `D48`。
2. Rust 会话详情的通用响应断言通过。
3. 未修改响应 Body 与上游 fixture 的字节完全一致。
4. 配置和结果只写测试临时目录，不进入安装包或首次启动数据。

该回归证明“Android 客户端 → 动态 Listener（固定 Server）→ 上游 → 原样返回”的通用能力，
不能替代 10.3 的 A920MAX、真实证书、真实上游和真实业务响应。

## 11. 需求追踪矩阵

| 需求 | UI | Rust Use Case / 模块 | IPC | 测试 |
| --- | --- | --- | --- | --- |
| Workspace | Workspace/Listener 页面 | application + domain | `workspace_*`, `listener_*` | workspace roundtrip |
| 入口配置唯一来源 | 入口配置/顶部状态栏 | application listener overview | `listener_overview`, listener events | overview + UI boundary |
| HTTP 请求目标/固定 Server | Listener 状态与抓包 | proxy | listener/status events | 100× HTTP + fixed Server |
| CONNECT/Upgrade 边界 | Listener 状态与诊断 | proxy | listener/status events | 501 before Server/Exchange |
| 固定 Server TLS/mTLS | 同一入口配置（Server 材料按入口导入） | proxy + infrastructure | `listener_*` | TLS matrix |
| 规则处理 | 规则/抓包详情 | application + proxy | rule | rule semantics |
| Android 定向 VPN | Android 弱网页 | application + android-engine | `android_*`, `device_network_*` | scope gate |
| 弱网 | Profile/实时统计 | android-engine | profile/status events | deterministic vectors |
| 真实上游兼容 | 无默认业务 UI | generic fixed-server route | existing generic IPC | real-device report |

## 12. 实施和停止条件

按身份与存储、Workspace、HTTP/Socket Exchange、通用 UI、Android 架构门禁、完整弱网、
真实设备兼容、跨平台 CI 的顺序实施。只有通用代理、Android 应用网络接管和真实设备兼容三类
验收全部通过，版本才可以发布。
