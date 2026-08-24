# Intercept Proxy 用户操作说明

本文按实际桌面 App 的工作顺序说明 Workspace、入口、协议包、规则、抓包、诊断、证书和 Android
网络接管。页面名称以当前中文界面为准。

## 1. 基本概念

### 1.1 Workspace

Workspace 是一套可切换的测试配置，保存 Listener、HTTP 基础规则、协议 Document 规则、Android
Profile 和证书引用。运行时报文、抓包 Exchange 和普通运行日志不属于 Workspace 配置。

切换 Workspace 前应停止当前入口。Listener 启动后使用不可变快照；编辑配置不会偷偷改变正在处理
的连接，需要明确保存并重新启动入口。

### 1.2 Listener

一个 Listener 只有一个本地端点和一种明确数据面：

- HTTP：正向 absolute-form 请求，或固定 Server 转发。
- Socket：RemoteServer 或 LocalServer；按协议转发或透明转发。

一个 App connection 创建一个 Exchange。协议模式按 App 请求、Server 回复的严格顺序推进；
App 断开或 Server 读写失败时，该 Exchange 结束。

### 1.3 规则

App 有两套目的不同的规则：

- HTTP 基础规则：匹配 Method、Path、Header、JSONPath、证书指纹等，并执行修改、故障或 Mock。
- 协议 Document 规则：匹配协议包 Decode 后的 Schema 字段，并顺序修改 Document。

两类规则可以在统一规则列表看到，但运行阶段和保存校验互相独立。

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

“协议包”页面统一显示 HTTP Body 与 Socket 内置包；Socket 外部包在线状态也在此处体现。

### 5.1 内置 Rhai 包

导入 ZIP 前应确认：

- `manifest.toml`、`protocol.rhai`、`display.rhai` 和上下行 Schema 齐全；
- Socket 包上下行都声明 Frame；HTTP 包两边都不声明 Frame；
- 包 ID + SemVer 是不可变身份，新内容使用新版本；
- Decode/Encode/Display 函数签名符合当前 API。

导入会先完成 ZIP、Manifest、Schema、Rhai 和入口签名校验；任一步失败都不会留下半安装包。

### 5.2 外部 Socket 包

外部包连接设置页公布的 loopback WebSocket `/packages`，注册后必须实现：

- `hooks.upstream.<frame|decode|encode>`
- `hooks.downstream.<frame|decode|encode>`
- `document.upstream.display`
- `document.downstream.display`

外部包断线后精确版本变为 offline；绑定它的 Listener fail-closed，不切换其他版本或透明模式。

### 5.3 Display

Display 是协议包生成的 HTML 展示结果。App 会清洗并放入无能力 sandbox iframe：

- 不显示 Document JSON；
- 不执行 script、事件属性、URL 导航或表单；
- Display 失败时 HTTP 回退 Body、Socket 回退 Hex；
- Display 失败属于观测失败，不改变已经成功的网络交易。

## 6. 统一规则页面

HTTP 与 Socket 规则显示在一个列表中，通过“作用范围”和“阶段”区分。点击一行后在右侧编辑。

### 6.1 新建方式

- 空白规则：新建 HTTP 基础规则。
- Body 报文规则：新建 HTTP Body 的协议 Document 规则。
- Socket 报文规则：新建 Socket 四方向 Document 规则。
- 从故障预设创建：把常见故障模板转换为普通 HTTP 规则。

“新建规则”对话框关闭后可以再次打开；选择规则或新建草稿不会自动保存。

### 6.2 HTTP 阶段能力

编辑器选项由 Rust 返回，保存时领域层再次校验：

| 阶段 | 可以配置 | 不能配置 |
| --- | --- | --- |
| 请求 | 请求字段、上行延迟/限速、Mock、上游连接/读写故障 | HTTP 响应状态、响应损坏、下行限速 |
| 响应 | 响应字段、状态码、下行延迟/限速、截断/错误长度/下行断连 | Mock、上游超时、上行断连 |
| TLS 握手 | 证书指纹、第 N 次命中、拒绝 TLS 握手 | HTTP 字段和其他内容/网络动作 |

额外约束：

- 限速与间歇通断方向由阶段固定，界面不允许手工选错；
- 一条规则最多一个终止动作；
- 终止动作必须是最后一个动作；
- 添加终止动作后，“添加动作”禁用；
- 条件按 AND 匹配，动作按声明顺序执行；
- 终止动作命中后停止当前规则剩余动作和后续规则。

“Proxy → Server 设置 response code”在概念上不成立，因此请求阶段不会显示自定义 HTTP 状态码；
该动作只属于 Server response 经过 Proxy 返回 App 的响应阶段。

### 6.3 Document 规则

Document 规则只能选择当前 Listener、精确协议包版本、方向 Schema 中存在的字段。值必须与
Schema 类型一致，不执行字符串到整数或 Blob 的隐式转换。

Socket 四阶段依次是：

1. App → Proxy
2. Proxy → Server
3. Server → Proxy
4. Proxy → App

后一个阶段看到前一个阶段修改后的 Document。一次读取产生一个 Envelope，长连接中的新报文追加
新事件，不覆盖之前数据。

## 7. 实时抓包

HTTP 和 Socket 同时显示在一个页面，不通过协议 Tab 隐藏另一类记录。

### 7.1 HTTP

HTTP 表格显示时间、终端 IP、通道、方向、方法、路径/请求类型、状态码、结果、耗时、规则和大小。
页面停留时新请求应立即追加，不需要切换页面。

### 7.2 Socket

Socket 表格一行对应一个 App connection/Exchange。详情按发生顺序显示：

1. Opened
2. App → Proxy Received
3. Proxy → Server Sent
4. Server → Proxy Received
5. Proxy → App Sent
6. Failed（如有）
7. Closed

详情显示原始字节/Hex、固定 Display 和失败阶段，不渲染 Document JSON，也不显示无意义的字节
上一页/下一页按钮。长连接中 D2 追加在 D1 后面。

“暂停列表滚动”只暂停视图滚动，不暂停网络、规则或记录；“清空当前显示/运行记录”只清理内存
展示，不修改 Workspace 或数据库配置。

## 8. 断点与故障

规则命中“暂停”后进入断点队列。可以查看当前消息并选择继续、修改后继续或终止。断点超时、容量
和取消都由 Rust 管理；页面关闭不能绕过既定处理结果。

延迟、抖动、限速、间歇通断属于可调度故障；Mock、拒绝、断开、丢弃和截断属于终止动作。故障
预设最终生成普通规则，后续仍在统一规则页面编辑和审计。

## 9. 诊断日志与 MCP

排障建议顺序：

1. 确认 Workspace、Listener、模式和 runtime epoch。
2. 检查抓包是否有 Opened。
3. 对照同一 Exchange 的 Received/Sent。
4. 若失败，先看 Failed stage 和稳定错误码。
5. 再查询普通运行日志、外部包 generation/RPC request ID。
6. 检查 evidence dropped/ignored/evicted 计数。
7. 最后生成复现报告。

内嵌 MCP 只绑定 loopback，提供只读应用快照、日志、ExchangeObservation、诊断和复现报告。它不能
启停 Listener、修改规则、重放交易或写数据库。

## 10. 应用数据导入导出

应用导出 ZIP 包含 Workspace、可移植 Settings、精确内置协议包源文件，以及用户选择的 Listener
TLS 可移植材料。它不包含运行时报文、ExchangeObservation 或本机安装级 Root CA 私钥。

导入步骤：

1. 选择 ZIP。
2. App 有界读取并校验路径、大小、压缩比、Manifest、Schema、Rhai 和证书材料。
3. 查看替换预览。
4. commit 前再次比较 Workspace/Settings revision、包与证书 generation。
5. 原子替换成功后重启 App。

本机已存在相同协议包不会因唯一键直接失败；完整应用导入以备份注册表为候选，事务内替换，失败
则完整回滚。

## 11. Android 应用网络接管

1. 连接 Android 设备并允许 ADB。
2. 安装/更新 Companion。
3. 选择需要接管的应用 allowlist。
4. 生成 Profile，确认桌面 Listener 地址可被设备访问。
5. 启动设备侧 VPN，并在 Android 权限弹窗中确认。
6. 检查 TUN、SOCKS5、ADB reverse 或 LAN 路由状态。
7. 完成后停止 Profile 并清理遗留 reverse/owner 状态。

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
