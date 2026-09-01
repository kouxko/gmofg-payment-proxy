# Intercept Proxy 用户操作说明

本文按实际桌面 App 的工作顺序说明 Workspace、入口、协议包、规则、抓包、诊断、证书和 Android
网络接管。页面名称以当前中文界面为准。

## 1. 基本概念

### 1.1 Workspace

Workspace 是一套可切换的测试配置，保存 Listener、统一规则定义、Android Profile 和证书引用。
运行时报文、抓包 Exchange 和普通运行日志不属于 Workspace 配置。

切换 Workspace 前应停止当前入口。Listener 启动后使用不可变快照；编辑配置不会偷偷改变正在处理
的连接，需要明确保存并重新启动入口。

### 1.2 Listener

一个 Listener 只有一个本地端点和一种明确数据面：

- HTTP：正向 absolute-form 请求，或固定 Server 转发。
- Socket：RemoteServer 或 LocalServer；按协议转发或透明转发。

一个 App connection 创建一个 Exchange。协议模式按 App 请求、Server 回复的严格顺序推进；
App 断开或 Server 读写失败时，该 Exchange 结束。

### 1.3 规则

App 只有一套 `RuleDefinition` 规则。每条规则只绑定一个 Listener，使用带标签的内容区分能力：

- HTTP 内容可以组合 Method、request target、Header、终端/证书条件与可选的协议 Document 条件，
  并执行 HTTP 修改、故障、Mock 或 Document 动作。request target 始终是原始 `/path?query`，请求与
  对应响应阶段读取同一份请求元数据，不包含 scheme、host 或 port。Header 名称使用单层 `/name`
  手动输入、ASCII 大小写不敏感，重复 Header 任一值命中即成立。
- Socket 内容只处理协议包 Decode 后的类型化 Document 条件与动作，不提供 HTTP 能力。

Method 只支持精确匹配；request target 与 Header 支持精确、包含、前缀、后缀和通配符。Document
有 Schema 时可以从递归路径下拉框选择，也可以手动输入；无 Schema 时只允许手动输入。Document
条件路径使用 RFC 6901，并允许完整路径段 `*` 匹配恰好一层，展开多个值时按 ANY 判断；Set、Clear、
Insert、Append 等动作始终使用精确路径，不接受通配符。旧 Path/JSONPath/Regex 匹配合同已删除，
没有别名、兼容解析或第二执行路径。

规则按固定阶段顺序执行；`priority` 只在同一阶段内排序。保存时由 Rust 使用当前 Listener、阶段、
协议包和 Schema 能力校验整条规则，HTTP 与 Document 条件共同决定同一条 HTTP 规则是否命中。
Listener 绑定以及 Document 的协议包和 Schema 绑定不能通过编辑改换；需要改绑时应复制或新建规则。

## 2. 建立测试 Workspace

1. 打开“工作区”。
2. 新建 Workspace，填写清晰名称和用途。
3. 选中 Workspace 后进入“入口配置”。
4. 配置 HTTP 或 Socket Listener。
5. 保存后检查列表中的本地地址、端口、模式、Server 和安全摘要。
6. 在执行真实交易前先确认测试端口没有被其他进程占用。

开发测试脚本可以重复安装同名 E2E Workspace；安装过程应更新同一套 Listener、规则和软件包，
不能制造重复记录。

## 3. HTTP Listener

### 3.1 正向 HTTP

正向模式接收普通 absolute-form HTTP 请求，并按请求中的 HTTP 目标转发。当前仅支持普通 HTTP：

- CONNECT 返回 `501`；
- HTTP Upgrade/WebSocket 返回 `501`；
- 不创建 tunnel、MITM 或透明转发兜底。

因此不能用“安装 Root CA + CONNECT”测试 HTTPS 抓包。若需要验证 HTTPS Server，应使用固定
Server，并分别配置 App 到 Proxy、Proxy 到 Server 两段 TLS。

### 3.2 固定 Server

1. 选择固定 Server 模式。
2. 填写唯一的 Server origin，例如 `http://127.0.0.1:19080` 或测试 HTTPS origin。
3. 一个 Listener 只绑定一个 endpoint；Host、端口或证书策略变化时建立另一个 Listener。
4. HTTP origin 只执行 TCP 探测；HTTPS origin 额外执行 TLS/hostname/CA 探测。
5. 保存后启动 Listener，再从 App 向本地 Listener 发送请求。

固定 Server 模式不会因为请求携带其他 authority 而改连另一个目标。

### 3.3 下游 TLS/mTLS

这是 App 连接 Proxy 的安全边界。Proxy 作为 TLS Server：

1. 选择 Server Identity。
2. 普通 TLS 不启用客户端认证。
3. mTLS 选择 Client Trust，并设置 Optional 或 Required。
4. Required 时，App 必须出示受信任客户端证书。

### 3.4 上游 TLS/mTLS

这是 Proxy 连接 Server 的独立安全边界。Proxy 作为 TLS Client：

1. 选择 Server Trust 或系统信任策略。
2. 保持正确 hostname/SNI。
3. 只有 Server 明确要求 mTLS 时才选择 Client Identity。
4. 使用“测试连接/TLS”检查 DNS、TCP、CA、hostname 和客户端身份。

私有上游未发送完整签发链时，可把 Intermediate CA 到 Root CA 按顺序放入同一个 PEM
Bundle，再通过现有的单文件 Server Trust 入口导入。Proxy 会解析、保存并加载其中全部 CA；
不提供多文件选择或自动合并。单证书 PEM、DER、CRT 和 CER 仍按原方式使用。

下游认证成功不代表上游认证成功；两边必须分别验证。

## 4. Socket Listener

### 4.1 RemoteServer

RemoteServer 将 App 连接与固定远端 TCP/TLS endpoint 包装进同一个 Exchange：

1. 填写本地监听地址和端口。
2. 填写唯一 Server host/port。
3. 选择“按协议转发”或“透明转发”。
4. 选择 TCP、TCP→TLS、TLS→TCP 或 TLS→TLS 安全拓扑。
5. 保存、启动，再发送测试报文。

按协议转发要求选择协议包。Reader 每次从 Socket 读取数据后调用 Frame；NeedMore 时继续读，一个
完整 Frame 后立即 Decode 并发往 Server，不等待第二个 Frame。

### 4.2 LocalServer

LocalServer 是本地回环服务，不连接上游。它仍使用同一个 Exchange、Reader/Writer/Pipeline 模型：

- Direct/透明模式按字节 echo；
- Scripted/协议模式读取完整报文并通过协议 Pipeline 返回；
- 不生成虚假的上游连接成功证据。

LocalServer 不配置上游 host、上游 CA 或上游客户端身份。

### 4.3 透明转发

透明 Socket 保持原始字节：

- App 读到多少就向 Server 写多少；
- Server 读到多少就向 App 写多少；
- 不执行 Frame、Decode、Document Rules、Encode 或 Display；
- 保持半关闭，不把一侧 EOF 误当成必须立即丢弃另一侧待发送数据。

协议包错误不能自动降级为透明转发。

## 5. 协议包

“协议包”页面统一显示 HTTP Body 与 Socket 本地包；远端包在线状态也在此处体现。

### 5.1 本地单文件协议包

导入 `.wasm` 前应确认：

- 文件是 WebAssembly Component，不是 Core Wasm、ZIP 或脚本源码；
- 唯一顶层 `intercept-proxy:manifest`、目标 WIT world 和上下行递归 Schema 齐全；
- 包 ID + SemVer 是不可变身份，新内容使用新版本；
- HTTP/Socket 的 Frame、Decode、Encode、Display exports 符合 package API 1。

prepare 静态校验 Component、Manifest 和 Schema。commit 会先在主进程内完成 Wasmtime 编译与实例化，
再原子持久化并发布 enabled runtime；任一步失败都不留下记录或缓存，也不会重试、切换远端调试包或
透明转发。停用会回收实例，启用和重启从已保存的同一文件重新实例化。

### 5.2 远端协议包

远端包连接设置页或配置会公布实际 WebSocket `/packages` 地址；监听范围由 `bind_address` 决定，
不得假定 loopback。注册后必须实现：

- `hooks.upstream.<frame|decode|encode>`
- `hooks.downstream.<frame|decode|encode>`
- `document.upstream.display`
- `document.downstream.display`

外部包断线后精确版本变为 offline；绑定它的 Listener fail-closed，不切换其他版本或透明模式。

### 5.3 Display

Display 是协议包生成的 HTML 展示结果。App 会清洗并放入无能力 sandbox iframe：

- 递归 Document、规则过程和最终值按 typed evidence 展示；
- 不执行 script、事件属性、URL 导航或表单；
- Display 失败时 HTTP 回退 Body、Socket 回退 Hex；
- Display 失败属于观测失败，不改变已经成功的网络交易。

## 6. 统一规则页面

HTTP 与 Socket 规则显示在一个列表中，通过“作用范围”和“阶段”区分。点击一行后在右侧固定编辑区
加载并编辑规则；加载期间切换选择时，旧请求返回后不会覆盖当前编辑内容。

### 6.1 新建方式

- 先选择单个 Listener，再选择 Rust 返回的可用阶段；规则创建后不能通过更新切换 Listener。
- HTTP 规则：在同一内容中编辑 HTTP 条件/动作，并在入口绑定协议包时按能力添加可选 Document。
- Socket 规则：只编辑协议 Document 条件/动作，不显示 HTTP 字段或 HTTP 动作。
- 从服务器响应创建：生成未保存、默认停用的 HTTP Mock 草稿，确认后再保存。
- 从故障预设创建：模板生成统一 HTTP 规则草稿，并通过同一保存、列表和停用流程管理。

“新建规则”只负责选择 Listener 和阶段；随后在规则页右侧固定编辑区填写并保存。选择规则或新建草稿
不会自动保存，规则编辑本身不使用弹窗。

左侧只显示一个规则列表，不再按阶段分区。每张规则卡使用“上行”或“下行”标识实际阶段；上行对应
Proxy → Server，下行对应 Proxy → App。

### 6.2 阶段能力

编辑器选项由 Rust 返回，保存时领域层再次校验：

| 统一阶段 | 可以配置 | 不能配置 |
| --- | --- | --- |
| Proxy → Server | 请求字段、上行延迟/限速、Mock、上游连接/读写故障，以及入口支持时的 Document | HTTP 响应状态、响应损坏、下行限速 |
| Proxy → App | 响应字段、状态码、下行延迟/限速、截断/错误长度/下行断连，以及入口支持时的 Document | Mock、上游超时、上行断连 |

额外约束：

- 限速与间歇通断方向由阶段固定，界面不允许手工选错；
- 一条规则最多一个终止动作；
- 终止动作必须是最后一个动作；
- 添加终止动作后，“添加动作”禁用；
- 一条规则的条件使用轻量扁平列表，所有条件固定为 AND；需要 OR 时创建多条规则；
- Header 名称和 HTTP/Socket Document 路径直接使用 `/a/b` 输入，有 Schema 时可以下拉选择或手工
  输入，无 Schema 时只手工输入；
- 每条规则读取当前 working Document，命中的有序 actions 立即修改它并对后序规则可见，最终只
  Encode 一次；
- 终止动作命中后停止当前规则剩余动作和后续规则。

“Proxy → Server 设置 response code”在概念上不成立，因此请求阶段不会显示自定义 HTTP 状态码；
该动作只属于 Server response 经过 Proxy 返回 App 的响应阶段。

### 6.3 Document 规则

Document 规则可以选择当前 Listener、精确协议包版本与递归 Schema 路径，也可以通过 Rust capability
显式创建首个规则本地路径。值类型包括 null、boolean、number、string、object 和 array，不做隐式转换。
Schema array items 只表示元素模板，不会自动生成 index 0；只有显式创建索引后才显示具体 index。

Document 规则只在两个写出阶段执行：

1. Proxy → Server
2. Proxy → App

每个阶段从 Decode 结果创建私有 working Document；规则条件和 actions 按顺序读取/更新它，前序修改
对后序规则可见。一次读取产生一个 Envelope，长连接中的新报文追加新事件，不覆盖之前数据。

## 7. 实时抓包

HTTP 和 Socket 同时显示在一个页面，不通过协议 Tab 隐藏另一类记录。

### 7.1 HTTP

HTTP 表格显示时间、终端 IP、通道、方向、方法、路径/请求类型、状态码、结果、耗时、规则和大小。
页面停留时新请求应立即追加，不需要切换页面。

在 Exchange 详情中，服务器返回的完整 HTTP 响应会显示“用此服务器响应创建 Mock 草稿”。该操作
使用配对请求的完整 request-target，并复制状态码、可保留 Header 与 UTF-8 Body；草稿默认禁用且
不会自动保存。压缩、二进制、非 UTF-8 或证据不完整的响应会被拒绝。打开规则编辑器后仍需人工
检查并保存，保存前不会改变代理运行行为。

### 7.2 Socket

Socket 表格一行对应一个 App connection/Exchange。详情按发生顺序显示：

1. Opened
2. App → Proxy Received
3. Proxy → Server Sent
4. Server → Proxy Received
5. Proxy → App Sent
6. Failed（如有）
7. Closed

协议模式详情还显示 received Document、逐规则 typed operation summary、final working Document、
Encode/result/process evidence 和 stable error code。`changes_truncated=true` 表示逐规则摘要达到观测预算；
它不代表业务处理、最终 Document 或 Encode 被截断。没有执行的人机、真实设备或外部网络验证必须
记录为 `NOT_RUN`，不能用单元测试 PASS 替代。

详情显示原始字节/Hex、递归 Document、固定 Display、规则过程和失败阶段，不显示无意义的字节
上一页/下一页按钮。长连接中 D2 追加在 D1 后面。

“暂停列表滚动”只暂停视图滚动，不暂停网络、规则或记录；“清空当前显示/运行记录”只清理内存
展示，不修改 Workspace 或数据库配置。

## 8. 故障

延迟、抖动、限速、间歇通断属于可调度故障；Mock、拒绝、断开、丢弃和截断属于终止动作。故障
预设最终生成普通规则，后续仍在统一规则页面编辑和审计。

## 9. 诊断日志与 MCP

诊断日志默认按发生时间从新到旧排列；时间相同的记录按事件 ID 从大到小稳定排列，因此刷新和分页
不会把较旧记录插到较新记录上方。

排障建议顺序：

1. 确认 Workspace、Listener、模式和 runtime epoch。
2. 检查抓包是否有 Opened。
3. 对照同一 Exchange 的 Received/Sent。
4. 若失败，先看 Failed stage 和稳定错误码。
5. 再查询普通运行日志、外部包 generation/RPC request ID。
6. 检查 evidence dropped/ignored/evicted 计数。
7. 最后生成复现报告。

内嵌 MCP 在端口 `17653` 监听全接口 IPv4，并在平台支持时监听全接口 IPv6。它使用明文 HTTP，
不校验 Host/Origin、来源 IP、Authorization、API key、Cookie 或其他调用方身份。任何能够连接该端口
的主机都可以读取公开数据并调用环境配置工具；网络观察者也可能看到提交的私钥、密码和确认令牌。

现有 34 个工具继续只读。完整环境配置使用五步工具合同：

1. 调用 `mcp_environment_capabilities`，确认 IPv4/IPv6 capability、warning、预算和 schema。
2. 调用 `environment_candidate_create` 提交明确的现有或新 Workspace 目标；等待全部验证层完成并检查
   公开预览。create 返回前断开会取消候选并清理未提交私有材料。
3. 使用 `environment_candidate_status` 复查候选仍为 `preview_ready`。配置、运行态或 baseline 变化会
   使候选 stale，必须重新创建。
4. 确认预览后，把一次性 confirmation token 传给 `environment_candidate_apply`。成功响应只表示
   `apply_queued`；响应后断开不会取消 Application 已接管的任务。
5. 持续查询 status，直到 `committed` 或明确失败终态。MCP 不会自动停止、启动或重启 Listener；
   受影响 Listener 仍在运行或存在活动连接时 apply 拒绝。

预览、status、终态、错误、日志和 diagnostics 不返回私钥、密码、confirmation token、保护后字节或
原始请求体。MCP 仍不能重放交易，候选技术验证也不证明业务报文或交易成功。完整参数、注解、预算、
结果和错误契约见 [MCP 工具参考](mcp/tool-reference.md)。复现报告本身不包含 ExchangeObservation 或
HTTP 抓包，线路证据需要通过对应查询工具单独获取。

## 10. 应用数据导入导出

应用导出 ZIP 包含 Workspace、可移植 Settings、精确内置协议包源文件，以及用户选择的 Listener
TLS 可移植材料。它不包含运行时报文、ExchangeObservation 或本机安装级 Root CA 私钥。

导入步骤：

1. 选择 ZIP。
2. App 有界读取并校验路径、大小、压缩比、Manifest、Schema、JavaScript 包和证书材料。
3. 查看替换预览。
4. commit 前再次比较 Workspace/Settings revision、包与证书 generation。
5. 原子替换成功后重启 App。

本机已存在相同协议包不会因唯一键直接失败；完整应用导入以备份注册表为候选，事务内替换，失败
则完整回滚。

## 11. Android 应用网络接管

1. 连接一台或多台 Android 设备并允许 ADB；桌面最多保留 8 台设备的运行 owner。
2. 在目标设备卡片安装/更新 Companion，所有安装、授权和包查询都只作用于该 serial。
3. 为目标设备选择需要接管的应用 allowlist；包名、UID 和 shared UID 校验读取同一设备清单。
4. 选择该设备使用的 Profile，确认桌面 Listener 地址可被设备访问。
5. 启动设备侧 VPN，并在对应 Android 权限弹窗中确认。不同设备可使用各自 Profile 并行运行。
6. 在每台设备卡片分别检查 TUN、SOCKS5、ADB reverse 或 LAN 路由状态；离线 owner 会保留并等待同
   serial 重连，不影响其他设备。
7. 应用、停止和紧急恢复都绑定设备 serial 与当前 runtime epoch；完成后逐设备停止并清理其
   forward/reverse/owner 状态。

设备能连接桌面端口只证明网络可达；还要分别证明 TLS、代理转发、规则命中和业务回复。

## 12. 完成一次测试的判定

一次完整验证至少应分别记录：

- Listener 已启动且端口正确；
- App 请求进入同一 Exchange；
- Proxy 实际向 Server 写入；
- Server 回复被 Proxy 读取；
- Proxy 实际写回 App；
- 规则命中和实际 wire 内容符合预期；
- TLS/mTLS 两侧身份分别通过；
- 成功或失败记录实时出现在抓包页；
- 没有被忽略的观测丢失；
- 停止后端口可重新绑定。

完整场景、固定端口、脚本和判定标准见
[发布级验证矩阵](testing/release-validation-matrix.md)。
