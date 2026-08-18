# 产品待办

最后更新：2026-08-17

## 逐任务执行状态机

下表中经批准的 Rxx 切片是独立交付单元；顶层 TODO 是需求聚合，可以映射到多个切片，
不直接作为提交边界。`.omx/ultragoal/ledger.jsonl` 是切片动态状态、审查结论和精确提交 hash
的唯一事实源，每个切片在 ledger 中严格按下列状态机执行：

`PENDING -> IMPLEMENTING -> VERIFYING -> ADVERSARIAL_REVIEW -> REMEDIATING -> RE_REVIEW -> APPROVED -> ACCEPTED`

- 任意时刻只有一个 Rxx 切片可以处于未完成的执行/审查状态；当前切片未收口前
  不得开始下一个。
- 实现与初始验证完成后进入 `ADVERSARIAL_REVIEW`，必须由独立、只读、对抗性审查检查
  正确性、领域契约、安全、并发/生命周期、测试缺口和任务越界。
- 审查结论为 `REQUEST CHANGES` 时，必须在当前切片内修复 finding，重新验证并复审；
  只有最新代码获得 `APPROVE` 才能继续。
- `APPROVE` 前后都不为记录运行态而修改本文档或其他 tracked 计划文档。`APPROVE` 后必须
  重跑定向与受影响门禁，再只提交当前切片范围。禁止先提交后审查、多切片混合提交或用旧审查
  结果批准最新代码。
- 提交成功并核对后，只在 `.omx/ultragoal/ledger.jsonl` 追加该切片的 `ACCEPTED`、精确 hash
  和验证/审查证据；tracked 文档不回填动态状态或 hash。
- 只有 ledger 已记录当前切片 `ACCEPTED` 后，才按下表的拓扑顺序选择下一个依赖已满足的
  `PENDING` 切片。

## 有序交付切片

| 顺序 | 切片 | 映射 TODO | 依赖 | 完成定义 |
| --- | --- | --- | --- | --- |
| 01 | R00 | 全部（TODO 与历史计划卫生） | 无 | 文档矛盾、依赖和历史恢复点全部收口 |
| 02 | R01 | `TODO-ARCH-001`（首批基线） | R00 | As-Is/ADR/追踪骨架可验证并阻止越界实现 |
| 03 | R02 | `TODO-ANDROID-001A` | R01 | selected/runtime owner/epoch 全链正确 |
| 04 | R03 | `TODO-SOCKET-UX-001` | R01 | 三种用户工作方式与现有 wire 精确映射 |
| 05 | R04 | `TODO-CAPTURE-UX-002` + `TODO-RULES-UX-002`（合并交付） | R01 | 共享紧凑切换外壳，HTTP/Socket 状态与 DTO 仍隔离 |
| 06 | R05 | `TODO-WORKSPACE-UX-005` | R01 | legacy extractor 可迁移且生产消费链为零 |
| 07 | R06 | `TODO-PROTOCOL-PACKAGE-002` | R01 | ISO 示例包可打包、恢复、默认绑定并通过生产编译器 |
| 08 | R09 | `TODO-CERT-001` | R01 | 下游 serverAuth 安全导入 P12/PFX/加密 PEM |
| 09 | R07a | `TODO-APP-DATA-001`（ZIP v1 archive/wire validator） | R05, R06, R09 | archive/wire 边界严格、有界且功能入口仍关闭 |
| 10 | R07b | `TODO-APP-DATA-001`（一致性原子导出） | R07a | snapshot/temp/flush/atomic replace 失败不破坏旧文件 |
| 11 | R07c | `TODO-APP-DATA-001`（导入 prepare/preview） | R07a | 完整验证与安全预览完成，无写入 |
| 12 | R07d | `TODO-APP-DATA-001`（导入 atomic commit/compensation） | R07b, R07c | SQLite/缓存/证书全部成功或全部回滚 |
| 13 | R07e | `TODO-APP-DATA-001`（UI、legacy 兼容与跨平台开放） | R07d | 两个顶层入口、legacy 兼容和跨平台交叉恢复通过 |
| 14 | R08 | `TODO-WORKSPACE-UX-003` + `TODO-SETTINGS-UX-004`（合并交付） | R07e | Workspace/Settings 信息架构收敛且窄窗口无页面溢出 |
| 15 | R10 | `TODO-ANDROID-001B` | R02 | 运行端点可见，LAN IP 漂移可幂等恢复且 Reverse 无回归 |
| 16 | R11a | `TODO-MCP-001`（capability inventory + SDK/transport ADR） | R01, R07e, R09, R10 | 只读能力清单、denylist 与 SDK/transport ADR 确定 |
| 17 | R11b | `TODO-MCP-001`（read-only query ports） | R11a | 一致性 snapshot、分页查询和全部 Workspace 读取且无 mutation capability |
| 18 | R11c | `TODO-MCP-001`（loopback transport/lifecycle） | R11b | 默认启用、无认证的本机信任边界、资源限制和有界关停通过 |
| 19 | R11d | `TODO-MCP-001`（协议包资源与操作建议） | R11c | 权威资源/模板/生成类型可读，仍无 mutation |
| 20 | R11e | `TODO-MCP-001`（客户端/打包验证） | R11d | 真实客户端与 Windows/macOS 打包验证通过 |
| 21 | R12 | `TODO-PROTOCOL-PLATFORM-003` | R01, R06 | HTTP/Socket 独立协议包使用同一双方向 Document 执行模型，四阶段规则与抓包 Display 全链一致 |

## TODO-PROTOCOL-PLATFORM-003：统一 HTTP Body 与 Socket 的 Document 处理模型

需求状态：`实施中`（核心服务已实现，真实客户端与跨平台打包验证待完成）

优先级：`P0`

依赖：`TODO-ARCH-001`、`TODO-PROTOCOL-PACKAGE-002`

### 已确认且不得漂移的设计

- HTTP 与 Socket 是两类独立协议包；它们存放在同一应用级注册表中，以不可变 `kind` 隔离，并分别绑定、
  分页展示，不允许一个包同时服务两种数据平面。两类包只复用 Manifest 结构、Schema/Document 规则模型、Rhai Host API、
  Display 沙箱和测试工具。
- Manifest 不声明 `content_types`，不使用 `decode_request`、`encode_response` 等 HTTP 专用函数名。
  包类型由 hook 严格推导：两个方向都声明 `frame` 是 Socket；两个方向都不声明 `frame` 是 HTTP；
  只在单侧声明 `frame` 必须拒绝。
- HTTP 与 Socket 都使用以下双方向结构；Socket 仅在两个 `hooks` 表额外声明同名 `frame`：

```toml
api = 1

[package]
id = "example"
name = "Example"
version = "1.0.0"

[document.upstream]
schema = "schemas/upstream.toml"
display = "display"

[document.downstream]
schema = "schemas/downstream.toml"
display = "display"

[hooks.upstream]
decode = "decode"
encode = "encode"

[hooks.downstream]
decode = "decode"
encode = "encode"
```

- `upstream` Document/Schema 用于 App 发往 Server 的数据；`downstream` Document/Schema 用于
  Server 返回 App 的数据。四个规则阶段必须分别保存和执行：App→Proxy、Proxy→Server、
  Server→Proxy、Proxy→App。前两阶段共享 upstream Schema，后两阶段共享 downstream Schema，
  但规则集合、条件、动作顺序和命中记录互不合并。
- 每条规则的条件按 AND 判断；多条规则按优先级数值从小到大、同优先级按创建顺序逐条匹配；
  每条命中规则的动作从上到下顺序执行，后续规则看到前序动作已经修改后的 Document。
- 选择协议包即固定执行 Decode、规则、Encode 和 Display，不再保存或显示方向开关。Display 只生成
  抓包/会话 UI 的不可信 HTML，不修改线路；Encode 只在线路 Document 实际改变时替换报文。

### HTTP 文本 Body 语义

- HTTP 协议包只处理非空 UTF-8 文本 Body；空 Body 直接通过，不调用 Decode/Encode/Display，
  也不产生错误。
- 非空 Body 的固定顺序是 Decode→当前方向第一阶段规则→当前方向第二阶段规则→按需 Encode。
  无规则命中或命中后 Document 未改变时，必须保留完全相同的原始 Body 字节，
  并保证 Content-Length 的语义数值与 Body 字节数一致；HTTP 线路库可以规范化 Header 的空白和序列化格式。
  Document 改变后才调用 Encode，并按编码后的 UTF-8 字节长度重算 Content-Length。
- HTTP Header、Cookie、状态码、方法、URL 等仍由 HTTP 专属能力处理，不塞进 Document 规则模型。
  协议包只负责 Body 文本与 Schema 字段之间的转换和展示。

### 抓包、错误和 UI

- HTTP 与 Socket 的四个方向都保存原始/写出报文、方向 Schema、Document、命中规则及 Display 结果；
  抓包详情默认展示协议包 Display，仍提供原始文本/Hex 证据入口。
- Decode、规则、Encode 或 Display 失败必须显示稳定阶段、错误码和安全的详细原因；列表可显示摘要，
  详情不得只写“内部错误”。错误中不得回显密码、私钥或未授权秘密材料。
- HTTP/Socket 页面保持条件挂载和 DTO 隔离；用户界面不出现 `Listener`、`Frame`、`Document`、
  `Decode`、`Encode`、`Schema` 等实现术语，精确技术身份仅放在默认折叠的高级信息/诊断中。

### 实施与验收

- 先完成 strict Manifest、编译器、运行时、持久化和生成绑定，再同步前端；不保留旧格式兼容分支。
  1.0 正式发布前发现旧 Schema 版本时清空旧数据并要求重新配置，不执行历史迁移。
- 定向测试覆盖 HTTP 空 Body、UTF-8 拒绝、未命中精确保留、命中未改变精确保留、改变后重编码、
  Content-Length、四阶段顺序、上下游不同 Schema、Socket Relay/本地应答、Display 与阶段错误详情。
- 新增/重写核心模块以 100% 语句/分支/函数/行覆盖为目标；无法达到的不可达防御分支必须删除或
  明确证明，而不是放宽门禁。最后统一执行架构审查、HTTP/Socket 隔离审查、对抗测试、视觉矩阵、
  全量门禁，再提交推送并要求 Windows CI 对该精确提交成功。

## TODO-ARCH-001：建立 HTTP、Socket 与协议包的整体软件设计基线

需求状态：`待实施`（静态快照，执行中不更新）

优先级：`P0`

依赖：`无`

### 目的

当前仓库已经分别实现 HTTP/HTTPS、CONNECT/MITM、Socket 透明转发、按协议处理、本地应答、
协议包、两类规则和两类抓包，但现有架构文档仍主要描述 HTTP，请求生命周期文档也没有覆盖
Socket 与协议包。继续直接优化代码容易把“减少重复”误解为“混合协议语义”，或只按当前页面
结构重构而破坏运行时、并发、证书和持久化边界。

本项先使用可维护的软件工程文档建立当前事实和目标设计，再依据经过评审的模型分阶段优化代码。
设计基线没有完成并通过对抗审查前，不启动 HTTP/Socket 合并、通用协议包扩展或大范围重构。

### 文档原则

- 明确区分 `As-Is（当前已经实现且有代码/测试证据）`、`To-Be（目标设计）`、`Open Decision
  （尚未决定）`，不能把计划中的能力写成现状。
- 图使用仓库内可版本控制的 Mermaid/文本源，不只保存 PNG；每张图必须附责任说明、关键不变量、
  对应源码入口和验证测试。
- Rust 不照搬面向对象继承式类图。领域部分使用“聚合、值对象、tagged union、trait/port、状态机、
  所有权和依赖方向”建模；只有实际存在的类型和关系才能进入 As-Is 图。
- `DESIGN.md` 负责产品目标、用户任务、信息架构、术语、交互和视觉原则；`docs/architecture/**`
  负责领域、运行时、持久化、并发和安全设计，两者互相引用但不重复维护同一事实。
- 每个目标模型都必须写清不做什么、失败策略、资源上限、敏感数据边界和兼容迁移，不能只画正常
  路径。

### 必须交付的 As-Is 设计

1. **系统上下文与容器图**：桌面 UI、Tauri、host、application、domain、infrastructure、proxy
   runtime、protocol-scripting、SQLite、受保护秘密存储、Android Companion 和外部 App/Server。
2. **领域模型图**：Workspace、Listener、HTTP/Socket data plane、Relay/本地应答、协议包身份与
   Schema、HTTP Rule、Socket Document Rule、Capture、Session、证书引用和运行 epoch；标出聚合
   所有者、精确引用、revision/high-water 和不可表达的非法组合。
3. **HTTP 端到端流程与时序图**：Forward HTTP、固定 Server HTTP/HTTPS、CONNECT Tunnel、MITM
   HTTP/1.1、WebSocket Upgrade、规则/断点/Mock、抓包与会话提交、停止与取消。
4. **Socket 端到端流程与时序图**：透明 Relay、按协议 Relay、本地应答；每个方向的 accept/TLS、
   Frame、Decode、Document Rule、Encode、write/flush、Display、Capture commit 和失败前零输出。
5. **协议包模型与执行图**：ZIP 导入、严格校验、编译、启停/引用、不可变 ID+version、Schema、
   Host API、资源限制、每连接/每方向隔离、应用数据导入导出和缓存代际。
6. **规则与抓包模型图**：HTTP 专属条件/动作、Socket 字段条件/动作、可共享的排序/乐观锁概念，
   以及 HTTP Exchange、Socket Relay Frame、Local Exchange 三种证据模型；禁止用大量 nullable 字段
   伪造统一模型。
7. **并发与生命周期图**：Listener/connection/task ownership、CancellationToken、mutation gate、
   blocking worker/semaphore、事件 replay、capture publisher、clear/reset generation barrier 和关停 join。
8. **持久化与可移植性模型**：SQLite 表/外键/事务、Workspace 与应用级所有权、协议包文件、证书
   材料、应用数据 ZIP、格式版本迁移、原子替换和失败补偿。
9. **安全与信任边界图**：App/Server 两侧 TLS 与 mTLS 角色、MITM Root、协议脚本沙箱、WebView/IPC、
   原生文件对话框、密码/私钥/Payload/脚本正文的允许流向和禁止流向。
10. **可追溯矩阵**：主要用户场景 -> 领域模型 -> Application use case -> Runtime/Adapter -> IPC/ViewModel
    -> 页面 -> 单元/集成/E2E/平台证据，能发现没有消费者的功能和只有 UI 没有运行链路的功能。

### 必须交付的 To-Be 设计与 ADR

- 为“HTTP 与 Socket 是否只统一产品外壳，还是扩展统一协议包平台”建立正式 ADR，至少比较：
  保持完全分离、共享包管理但使用独立 HTTP/Socket ABI、让 HTTP Body 使用协议包 Document 三种方案。
- 明确可以共享的中立内核（Listener 生命周期、transport/TLS 原语、包注册、Schema/Document、分页、
  事件和 UI shell）与必须隔离的协议语义（HTTP parser/CONNECT/MITM/Header/Status、Socket Frame/
  half-close/LocalResponder）。
- 给出目标模块和依赖图、稳定 port/trait、领域 tagged union、错误与事件模型、持久化版本、兼容层
  删除条件和禁止依赖；不得先写一个全局 service locator 或万能 DTO 再补设计。
- 列出每项代码优化的行为保持测试、迁移顺序、回滚点、删除条件和完成证据。目标设计允许结论为
  “当前边界已经合理，无需合并”，不能把重构规模当作成果。

### 建议文档结构

```text
DESIGN.md
docs/architecture/
├── README.md
├── system-context.md
├── domain-model.md
├── http-data-plane.md
├── socket-data-plane.md
├── protocol-package-runtime.md
├── rules-and-captures.md
├── concurrency-and-lifecycle.md
├── persistence-and-portability.md
├── security-and-trust-boundaries.md
└── decisions/
    ├── ADR-001-http-socket-boundary.md
    └── ADR-002-protocol-package-targets.md
```

文件可以在审查后按已有文档合并，重点是责任完整且只有一个事实来源，不强制为了目录形式拆出
空文档。

### 设计驱动的代码优化流程

1. 冻结当前行为清单和差异测试，先证明 HTTP/Socket/TLS/规则/抓包/导入导出当前可观察行为。
2. 完成 As-Is 图和源码/测试映射；对照图识别职责混合、重复实现、死功能、错误依赖和缺失端口。
3. 评审 To-Be 与 ADR；未关闭的关键决策不得进入实现。
4. 按一个边界一个提交实施：先加行为锁，再移动/抽取，再删除兼容层；每个切片都更新对应图和矩阵。
5. 最后才统一页面外壳或公共模型；内部协议类型继续使用严格 tagged union，不能以视觉统一为理由
   混合运行时数据。
6. 独立对抗审查验证实现是否符合文档，同时反向检查文档是否与最新代码一致；不一致即未完成。

### 验收标准

- 新开发者能仅按文档准确说明 HTTP Forward/CONNECT/MITM、Socket 三种工作方式、协议包执行、规则、
  抓包、证书和应用数据恢复的完整路径，并能定位每一步代码所有者。
- 所有 Mermaid 图可渲染，图中类型/trait/命令/事件可在当前源码找到；目标项和未实现项有明确标识。
- 至少用一个 HTTP 场景、一个按协议 Socket Relay、一个本地应答和一个失败/取消场景完成矩阵反向
  追踪，代码、事件、持久化和测试证据闭环。
- 架构门禁能拒绝 HTTP 引入 Socket runtime、Socket 引入 Hyper/HTTP DTO、中立 transport 依赖
  Application/UI、前端复制 Rust 业务校验等违规示例。
- 代码优化计划按风险和依赖排序，明确“不改行为”的部分与产品语义变更部分；设计文档提交与实现
  提交分离，便于审查和回滚。
- `DESIGN.md` 从 Draft 更新为经过确认的 Active 或明确列出未关闭问题；架构索引、需求、用户说明、
  TODO 和代码注释不存在互相矛盾的术语或能力声明。

## TODO-MCP-001：提供本机最大可见范围的只读诊断 MCP

需求状态：`待实施`（静态快照，执行中不更新）

优先级：`P1`

依赖：`TODO-ARCH-001`、`TODO-APP-DATA-001`、`TODO-CERT-001`、`TODO-ANDROID-001B`

### 用户目标

用户在运行 Intercept Proxy 时遇到 Listener 启动失败、TLS/mTLS、上游连接、协议包、规则、抓包、
Android 路由或应用数据恢复问题，可以在已连接的 MCP 客户端中直接询问。MCP 对当前操作系统用户
拥有的 Intercept Proxy 应用数据提供最大读取能力，不做 Payload、凭据、脚本、证书、路径或内部
状态的隐私隐藏、脱敏和按字段授权；但 MCP 不直接操作应用，只分析证据并给出可由用户执行的建议。

MCP 负责把完整工具和结构化上下文提供给 Codex、Claude Desktop 等 MCP Client；它本身不包含
模型，也不等于应用内聊天窗口。若以后需要内置聊天、模型供应商/API Key 或离线模型，仍另建任务。

### 只读权限范围

- MCP 随应用默认启动，只绑定 `127.0.0.1`，不提供认证。产品明确把当前操作系统用户下的本机进程
  视为可信边界；任何能访问该回环端口的本机进程都可以遍历全部 Workspace 和应用级只读数据，
  不受当前 UI 所选 Workspace 限制，也不提供隐私保护承诺。
- 提供完整读取能力，包括设置、Workspace、Listener、HTTP/Socket 规则、故障预设、Android 方案、
  运行状态、runtime epoch、任务/连接/队列/容量、全部日志和诊断、HTTP/Socket 抓包原始字节、
  Header、Cookie、Authorization、Body、Document 字段值、Display HTML、规则轨迹和内部错误文本。
- 协议包读取以现有 `Application` 只读 façade 为边界：提供包清单、Manifest/Schema 投影、能力、精确
  引用和使用情况；官方模板额外提供 Manifest、Schema、全部 Rhai 源码/库和 ZIP Resources。
- 提供与当前应用版本严格匹配的 Socket 协议包编写参考，包括 Manifest、Document Schema、Host API、
  Rhai 沙箱、资源限制、Frame/Decode/Encode/Display 生命周期、Relay 双向语义、LocalResponder 的
  Request/Response Document 隔离、规则执行顺序、错误码和完整 ISO 8583 示例。
- 证书能力仅复用 `Application` 提供的公开证书元数据和 Workspace 引用，不读取私钥、密码或原始
  P12/PFX/PEM。MCP 不直接读取 SQLite、缓存目录或任意文件；需要扩展时必须先增加领域化只读 façade。
- MCP 不提供任何应用 mutation：不得创建、复制、选择、保存、启用、停用、删除、导入、导出、
  清空或重置对象，不得启动/停止/测试 Listener，不得应用 Android 网络方案，不得修改数据库、缓存、
  文件、证书、协议包源码或运行时状态。
- MCP 不主动发起 DNS、TCP、TLS/mTLS、HTTP 或 Socket 网络探测；它只读取应用已经产生的诊断、
  连接证据、错误阶段、抓包和日志，并建议用户在应用 UI 中执行哪项测试或修改。
- “最大访问能力”只表示可以读取 Intercept Proxy 已持有或已记录的完整数据与诊断证据，不包含任何
  主动探测、生命周期控制或写权限，更不扩展为操作系统 Shell、进程控制或文件系统读写。不得用通用 `exec`、SQL、
  `write_file`、网络请求或 Tauri command 透传绕过只读边界。

### 工具与资源模型

- 读取工具覆盖 app snapshot、设置、Workspace、入口/runtime、抓包/session、断点、HTTP/Socket
  规则、协议包投影/使用情况、公开证书元数据、诊断和 Android 路由；列表使用现有 façade 提供的
  有界分页、过滤和精确详情查询。
- MCP 工具表只注册查询、解释和建议类工具；禁止注册 mutation、万能 `execute_action`、Tauri command
  透传、任意 SQL、任意网络请求、任意文件读写和任意 Shell。即使客户端显式要求，也只能返回操作
  建议、风险、预期结果和 UI 路径，不能代替用户执行。
- MCP Resources 可以暴露完整架构文档、用户文档、错误目录、协议包文件和经请求生成的应用数据
  快照；Tools 负责只读查询与解释；Prompts 只提供诊断和操作建议模板。
- 错误界面可以生成“诊断上下文 ID”方便定位当前问题，但它只是快捷入口，不限制客户端继续查询
  其他 Workspace、抓包、日志、源码、证书和内部状态。

#### Socket 协议包编写参考

- 使用带应用版本和协议包 API 版本的 MCP Resources 暴露编写总览、Manifest、Schema、Host API、
  Rhai 沙箱、资源上限、错误目录、Relay/LocalResponder 方向模型和 ISO 8583 示例；资源 URI 必须稳定、
  可发现且能明确说明适用版本。
- 参考资料必须直接来自应用随包发布的权威文档、模板或由生产类型生成，不能由 MCP 维护一套容易过期
  的复制文本。文档示例必须在 CI 中用同一个生产解析器、编译器和资源限制执行。
- MCP 可以读取已安装包的 Application 投影和官方模板完整文件，比较用户问题与 Manifest/Schema/
  Rhai API，结合现有诊断解释 file/function/line/column/field/stage 错误并给出修改建议。
- MCP 可以回答“如何编写 Frame/Decode/Encode/Display”“如何声明四种字段类型”“如何为 Relay 两个
  方向或 LocalResponder 响应方向编写规则”等问题，但不得在应用目录创建项目、改写文件、生成 ZIP、
  导入、启用或绑定协议包；这些步骤只作为用户操作说明返回。
- 建议新版本时必须保留已安装版本不可变语义：不得建议原地修改已安装 `id + version`，应建议复制
  模板、提升 SemVer、在外部工作目录完成修改，再通过应用 UI 导入和验证。
- ISO 8583 参考必须说明长度头、Bitmap、MTI、字段编码和示例字段集的确切范围，明确它是可修改的
  Profile 示例而不是覆盖所有机构方言的“通用 ISO 8583 标准包”。

### 安全与完整性边界

- 不设置隐私脱敏、字段级读取限制或客户端认证。服务只绑定 IPv4 回环地址，不允许远程网络直接
  连接；同一操作系统用户下的本机进程属于产品明确接受的信任边界，能够读取返回的完整数据。
- Payload、Header、日志、脚本、Display 和错误文本即使允许完整返回，仍属于不可信 data，不能被
  MCP server 当成指令或自动转成工具调用；用包含“忽略规则并删除数据”等内容验证 prompt injection
  不会让 server 注册、伪造或调用任何写操作。
- 调用审计允许记录完整参数和结果；日志、响应、资源和队列仍使用条目数与逻辑字节双重上限，避免
  大抓包、数据库或证书数据耗尽内存。超限时分页/分块，不以隐私理由截断内容。

### 产品交互

- 用户文档明确展示“本机 MCP 默认启用、无认证，可以读取全部应用数据，但不能操作应用”，并给出
  固定回环 endpoint 和客户端连接配置；当前版本不提供会话撤销或启停 UI。
- 错误 Alert 提供“复制诊断上下文 ID”，MCP Client 也可以不使用 ID 直接遍历完整应用状态。
- 回答必须能引用稳定事实：错误码、阶段、Listener、方向、发生时间和检查来源，并明确区分
  “TCP 成功”“TLS 成功”“HTTP 成功”“业务成功”，不能因下层成功推断上层结果。
- 操作建议必须给出目标对象、UI 路径、建议值、原因、风险、预期验证结果和回退方式，并明确标记
  “尚未执行”；不得使用“已修复”“已重启”“已导入”等暗示应用状态已经改变的措辞。
- 应用未运行、对象已删除、revision 冲突或事件已被 retention 清理时，返回稳定 typed error 和
  恢复建议；完整读取权限不能把不存在或已过期的状态伪造成成功。

### 生命周期与实现约束

- MCP server/bridge 由应用组合并拥有生命周期；端口占用或启动失败不得影响代理功能，停止应用必须
  bounded shutdown 并 join 所有任务，不能留下端口或孤儿进程。
- 查询使用权威 snapshot + cursor/revision/generation，跨 await 后重新验证对象；MCP 可以访问全部
  Workspace，但只能说明快照版本和状态变化，不能用旧快照覆盖新的配置或运行 epoch。
- 每个工具定义严格 JSON Schema、最大分页、最大字符串/集合/逻辑字节和 deadline；取消请求要传播
  到 Application 查询，不允许慢客户端占满运行时或 SQLite gate。
- MCP SDK、协议版本和传输方式在实现前查阅官方最新规范，并通过 dependency review 决定；不得
  手写一个“看起来像 MCP”的私有 JSON-RPC 协议。
- MCP 适配器优先复用只读 Application query。只有普通 API 为防止前端泄漏而裁剪、但完整诊断确实
  需要的内容，才新增 host-owned privileged read port；该端口必须逐项列出能力，不能直接暴露 Rust
  对象地址、锁、未界定生命周期的引用、写连接或任何 mutation capability。

### 验收测试

- 非回环连接、未知工具、超限参数和高并发均确定性拒绝；MCP 端口占用时应用继续启动，且不影响
  现有入口、规则、抓包或 UI。
- Listener 启动、DNS、TCP、TLS/mTLS、HTTP 写/读、Socket Frame/Decode/Rule/Encode/Write、协议包
  校验和 Android 路由各至少有一个真实错误能通过 MCP 返回稳定阶段和完整底层证据。
- façade 已提供的数据应完整返回，包括 HTTP Header/Cookie/Authorization/Body、Socket
  origin/written、Document、Display、日志、规则与公开证书元数据；MCP 不绕过 façade 读取私钥、
  密码、应用路径或数据库文件。
- 只读负面矩阵枚举所有 Workspace、Listener、规则、协议包、证书、设置、Android、抓包和 reset
  mutation，证明 MCP 工具表无对应操作，伪造工具名、Tauri command、SQL、网络请求、文件写入和
  Shell 均被拒绝，应用数据库、文件、runtime epoch、连接和 UI 状态保持逐字节/逐字段不变。
- 从 MCP Resources 获取 Socket 协议包编写说明和 ISO 8583 示例，证明其中 Manifest、Schema、Host
  API、Frame/Decode/Encode/Display 与 Relay/LocalResponder 示例均能由当前生产解析器和编译器通过；
  故意使用旧 API 的 fixture 必须被契约测试发现，不能发布陈旧参考。
- 用真实协议包编译错误验证 MCP 能关联完整源码并给出精确 file/function/line/column/field/stage、修改
  建议、测试报文、UI 导入步骤和 SemVer 升级建议，同时证明协议包目录、注册表和启用状态零变化。
- 恶意 Header/Body/Socket Payload/Display/规则名称包含 prompt injection、巨大 Unicode、控制字符、
  HTML/脚本和秘密样例时，工具完整返回数据但不会自行触发任何应用操作。
- 两个 Workspace、两个客户端、旧 runtime epoch、应用侧并发 mutation、clear/reset 和重启场景证明
  MCP 查询会标记不稳定快照并重新读取，绝不会执行写入；服务停止后零迟到响应。
- 队列满、客户端停止读取、查询超时、server stop 和应用退出均在界限内释放任务、连接、许可和
  内存；不得阻塞代理数据面。
- 使用至少两个真实 MCP Client 完成连接、完整读取、问题解释、Socket 协议包编写参考、操作建议和
  错误恢复；Windows/macOS 打包产物都验证启动、只读权限、路径和卸载后无残留服务。
- 安全审查确认服务只绑定回环地址且无认证，并把这一产品接受的本机信任边界记录在 ADR；源码扫描
  和负面 fixture 拒绝远程绑定、未登记万能命令、任何应用 mutation、绕过领域校验和任意操作系统
  文件/Shell 后门。

## TODO-SOCKET-UX-001：把 Socket 配置改为三种用户工作方式

需求状态：`待实施`（静态快照，执行中不更新）

优先级：`P0`

依赖：`TODO-ARCH-001`

### 问题

当前页面同时要求用户理解 `Direct / Scripted` 和 `Relay / LocalResponder` 两组代码模型，
说明中还直接出现 Frame、Document、Decode、Encode 和 Display。这些概念对实现是必要的，
但不是配置代理监听时应先理解的产品概念；两组下拉还会让用户误以为任意组合都合法。

### 设计结论

普通界面只保留一个“工作方式”选择，并由前端严格映射到现有领域模型：

| 用户看到的工作方式 | 用户含义 | 内部映射 |
| --- | --- | --- |
| 透明转发 | 收到什么字节就转发什么字节 | `Direct + Relay` |
| 按协议转发 | 解析报文，可按规则修改，再转发到 Server | `Scripted + Relay` |
| 本地应答 | 不连接 Server；解析请求并在本机生成响应 | `Scripted + LocalResponder` |

- 普通页面不再展示 `Direct`、`Scripted`、`Relay`、`LocalResponder`、Frame、Document、
  Decode、Encode 或 Display；默认折叠的“高级技术信息”、协议包详情与诊断详情可以保留精确
  package ID/version、Schema 和稳定技术标识，但不能把它们作为完成普通配置的前提。
- 产品文案改为“透明转发”“按协议转发”“本地应答”“解析报文”“按字段重新生成报文”
  和“协议预览”。“Document”统一显示为“报文字段”或“解析后的字段”。
- 工作方式切换必须原子完成底层模式转换并清除不再适用的字段，不能组合出
  `Direct + LocalResponder` 等非法或不可运行状态。
- “基本配置”只展示监听地址/端口、工作方式、App 接入方式，以及 Relay 时的 Server 地址。
  读取/写入超时、CIDR、最大并发、TLS 证书细节收进“高级设置”，默认折叠但保持可访问。
- 透明转发不显示协议包、方向解析开关和规则说明；按协议转发显示 Server 与协议处理；
  本地应答完全不显示 Server 卡片和上游测试入口。
- 方向能力改为面向结果的开关，例如“解析客户端请求”“按字段生成发往 Server 的报文”
  “解析 Server 响应”“按字段生成返回 App 的报文”；不可用原因用中文说明。
- 已保存 Listener 的底层拓扑、精确协议包版本和运行锁语义保持不变；本次是产品交互简化，
  不能削弱后端校验、持久化兼容、运行快照冻结或错误的 fail-closed 行为。

### 验收测试

- 新建 Socket Listener 只出现一个工作方式选择，并能分别生成上述三种精确底层配置。
- 三种方式来回切换不会残留 Server、证书、协议包或方向开关的旧值，也不会产生非法组合。
- 普通配置 DOM、可访问名称和帮助文案中不出现 Frame、Document、Decode、Encode、Display、
  Direct、Scripted、Relay 或 LocalResponder。
- 透明转发、按协议转发、本地应答分别只渲染所需卡片；LocalResponder 页面没有任何
  Server 主机、Server 端口、DNS、上游 TLS 或连接测试控件。
- 运行中的 Listener 仍锁定全部影响运行快照的配置；停止后恢复编辑，原配置不丢失。
- 1024×720 与 1440×900、亮色与暗色下完成键盘、焦点、无横向滚动和视觉验收。

## TODO-PROTOCOL-PACKAGE-002：内置可恢复、可导出的 ISO 8583 示例解析器

需求状态：`待实施`（静态快照，执行中不更新）

优先级：`P0`

依赖：`TODO-ARCH-001`

### 问题

仓库已经有 `templates/socket-protocol/iso8583-standard`，并用于编译与一致性测试，但产品首次
启动后不会自动拥有一个可选的 ISO 8583 解析器。现有模板采用“2 字节大端长度头、ASCII MTI、
主位图以及 DE3/DE4/DE7/DE11/DE41/DE49”的明确示例 Profile，并不是适用于所有收单系统的
完整 ISO 8583 方言。用户目前必须先理解 ZIP 结构并自行导入，也没有从产品导出该示例进行
学习、修改或创建新协议包的入口。

### 目标

- 桌面应用随包携带经过当前编译器验证的 `iso8583-ascii-standard@1.0.0` 示例协议包；首次创建数据库、
  应用数据重置后以及升级到该功能的既有数据库，都能幂等地恢复为“已安装、校验有效、已启用”。
- 用户第一次选择“按协议转发”或“本地应答”时，如果没有既有精确绑定，默认选中该官方版本；
  已保存 Listener 继续绑定原精确版本，升级时不得静默换包。
- 协议包列表与详情显示“内置示例”标识；协议包页面提供“导出 ISO 8583 模板 ZIP”，直接导出
  应用编译期携带并通过校验的精确模板。统一的“导出应用数据”ZIP 也必须把该模板作为完整协议包
  目录导出，用户可从任一入口取得、学习和修改模板。
- 应用数据 ZIP 中的模板目录至少包含 `manifest.toml`、`document.toml`、`protocol.rhai`、
  `display.rhai`、`libraries/iso8583.rhai`、README 和完整请求/响应示例。
- 页面与导出 README 必须明确提示：“这是起始示例，接入真实系统前必须按对端长度头、位图、
  字段编码和私有域规格修改”，不能宣传成不需要配置即可适配所有 ISO 8583 系统。
- 只有用户主动确认生成的最终应用数据 ZIP 才显式包含全部已安装协议包的原始包内文件
  （包括源码）。普通列表、详情、导出/导入预览、错误和日志均不得返回或显示协议包源码或
  本机路径。
- 内置模板被删除、损坏或版本不可用时，协议包页面提供“恢复内置模板”，恢复前重新走完整
  ZIP、Manifest、Schema、Rhai 与 API 校验，不能直接信任编译期资产。
- 内置包身份必须有稳定且受保护的精确 ID/版本规则；用户导入同身份不同内容时 fail-closed，
  不覆盖官方模板，也不破坏引用它的 Listener、规则或历史抓包。

### 验收测试

- 全新安装、既有数据库升级、应用数据重置和重复启动均恰好得到一个有效的官方 ISO 8583 版本。
- 新建“按协议转发”和“本地应答”默认绑定官方包；已有精确绑定在应用升级后保持不变。
- 统一应用数据 ZIP 可被“导入应用数据”流程恢复，内置模板的身份、Schema、四类字段、
  Frame/解析/组包/协议预览能力及示例均一致；损坏任一文件时导入失败且不产生部分写入。
- 删除或破坏内置版本后可以显式恢复；恢复失败时原注册表、启用状态和引用关系不被部分修改。
- 导出/导入预览、错误和日志中不包含数据库路径、应用安装路径、临时 token、第三方包源码或
  其他本机信息；用户确认生成的最终 ZIP 按上述契约包含协议包原始文件。
- macOS/Windows 打包产物都包含同一模板，构建门禁验证资产存在且实际可编译，不只检查文件名。
- 协议包页的模板 ZIP 导出支持原生保存对话框取消、覆盖确认与原子替换；导出的 ZIP 可被当前
  “导入协议包 ZIP”流程重新校验和导入。

## TODO-CAPTURE-UX-002：重做抓包页 HTTP/Socket 切换条

需求状态：`待实施`（静态快照，执行中不更新）

优先级：`P1`

依赖：`TODO-ARCH-001`

### 问题

抓包页当前使用横跨整个内容区的大号圆角 Tabs，形成一条独立的厚重横幅，与页面标题、筛选区、
表格和左侧导航的视觉层级不协调；选中块过宽，切换控件比页面主要操作更抢眼。

### 目标

- 页面主标题统一为“实时抓包”，在标题旁或标题下使用内容宽度自适应的紧凑协议切换：
  `HTTP | Socket`，不再显示横跨全页的“HTTP 抓包 / Socket 抓包”大胶囊。
- 切换条高度、圆角、选中态、焦点环和左右留白与现有 HeroUI 小型控件一致；选中态可以使用
  细下划线或轻量底色，不能形成第二条页面顶栏。
- 标题、暂停/恢复、清空和刷新操作共用清晰的一层页面 Header；切换条不额外占用大块垂直空间。
- HTTP 与 Socket 内容继续条件挂载，Socket 页面不得出现关键字/状态码/Header/Cookie/JSONPath
  等 HTTP 专属筛选或列；切换只改变协议表面，不混用查询状态和详情。
- 抽取一个共享的紧凑“协议类型切换”组件，供抓包页与规则页使用，避免两页再次产生视觉漂移。

### 验收测试

- 1440×900 和 1024×720、亮色与暗色下，切换条宽度随内容而不是随页面拉伸，无横向溢出。
- 鼠标、Enter/Space、左右方向键、Home/End 均可切换；ARIA tablist/tab/tabpanel 关系正确。
- 切到 Socket 后 HTTP 表单和详情从 DOM/可访问树卸载；切回 HTTP 后 Socket 内容同样卸载。
- 页面首屏能同时看到标题、主要操作和第一行筛选/空态，不因切换条浪费一整行大高度。
- 截图回归覆盖 HTTP、Socket、空态、列表态以及窄窗口，不出现当前截图中的整页大胶囊横幅。

## TODO-RULES-UX-002：重做规则页 HTTP/Socket 切换与页面层级

需求状态：`待实施`（静态快照，执行中不更新）

优先级：`P1`

依赖：`TODO-ARCH-001`

### 问题

规则页复用了同样的全宽大号 Tabs，导致协议切换与“拦截规则”、新建/导入/导出、规则列表和
编辑区彼此割裂。空列表时尤其显得顶部很重、主体很空，Socket 规则也容易被误认为 HTTP
规则编辑器中的一个附属页签。

### 目标

- 使用与抓包页完全一致的共享紧凑切换组件，页面标题统一为“规则”，协议选择显示为
  `HTTP | Socket`；当前内容区再显示“HTTP 拦截规则”或“Socket 报文规则”的明确副标题。
- 新建、导入、导出等操作与当前协议内容区对齐。仅 HTTP 支持的导入/导出不得在 Socket
  规则页面占位；Socket 页面只展示其真实支持的操作。
- 保留 HTTP 与 Socket 编辑器条件挂载、各自草稿/异步校验和 fail-closed IPC 边界，不能为了
  视觉统一把两套规则 DTO 或字段混在一起。
- 空态同时说明当前协议类型和下一步操作，例如“暂无 Socket 报文规则，选择新建规则后绑定
  一个按协议处理的 Socket Listener”；避免右侧只显示孤立的“选择规则进行编辑”。
- 桌面宽屏保留列表/编辑双栏；窄窗口改为自然上下流并在选择后将焦点/视口移动到编辑器。

### 验收测试

- 抓包页和规则页的协议切换在尺寸、间距、选中态、键盘行为和焦点样式上完全一致。
- HTTP 页面不渲染 Socket Listener、协议包、报文字段控件；Socket 页面不渲染 HTTP Header、
  Cookie、状态码、JSONPath、请求体或响应体专属控件。
- 切换协议时未完成的异步响应不能覆盖另一个协议页面，隐藏编辑器不留在 DOM/可访问树。
- HTTP 空态、Socket 空态、有规则列表、编辑中、保存失败和运行锁定均有 1024/1440、亮/暗截图。
- UI contracts、前端边界扫描和定向覆盖率门禁锁定共享组件，禁止以后恢复成全宽大胶囊 Tabs。

## TODO-APP-DATA-001：统一为一个 ZIP 应用数据导出与导入

需求状态：`待实施`（静态快照，执行中不更新）

优先级：`P0`

依赖：`TODO-WORKSPACE-UX-005`、`TODO-PROTOCOL-PACKAGE-002`、`TODO-CERT-001`

### 问题

当前产品同时提供“导入单个 Workspace”“导出当前 Workspace”“导出完整应用配置”和“导入完整
应用配置”，文件语义和入口过多。完整应用配置虽然已经携带协议包数据，但用户无法从界面判断
各文件的包含范围，也不能把它们理解为一次完整、可恢复的应用数据备份。

### 设计结论

- 普通界面只保留两个数据迁移入口：“导出应用数据”和“导入应用数据”。两者统一使用
  `.zip`，建议文件名为 `intercept-proxy-backup-YYYYMMDD-HHmmss.zip`。
- 两个入口放在 Workspace 页顶部，与“新 Workspace 名称 / 新建”组成同一条单行操作栏；移除
  “导入单个 Workspace”“导出当前 Workspace”“导出完整应用配置”“导入完整应用配置”四个旧入口。
- “导出应用数据”ZIP 必须包含：
  - 全部 Workspace、当前选择、Listener、HTTP/Socket 规则、策略和 Android 设备网络方案；
  - 可移植的全局设置；
  - 全部已安装协议包的精确 ID/版本、启用状态和经过验证的原始包内文件；
  - 内置 ISO 8583 示例协议包的 README、脚本、Schema、库和样例；
  - Listener 恢复所必需、且现有可移植策略允许导出的外部证书和身份材料。
- ZIP 不包含抓包 Payload、会话历史、日志、诊断历史、运行状态、临时导入 token、SQLite 数据库
  原文件、Keychain/系统密钥、本机 MITM Root CA 私钥或其他不可移植机器状态。
- 备份格式不增加 SHA256 清单、数字签名或自定义签名检查。安全边界依靠严格 ZIP 路径、类型、
  条目数、单文件/总大小、嵌套深度、重复/大小写冲突、压缩比和解压后领域校验。
- ZIP 顶层使用固定、可版本化的结构，例如：

```text
intercept-proxy-backup.zip
├── application.json
├── protocol-packages/
│   └── <package-id>/<version>/...
└── portable-materials/
    └── ...
```

- `application.json` 只保存结构化配置和对 ZIP 内文件的相对引用，不嵌入本机绝对路径，不把
  协议包二进制或证书原文重复编码成巨大 JSON。
- 普通“导入应用数据”只接受该备份 ZIP。单个第三方协议包仍通过协议包页面的“导入协议包 ZIP”
  安装，因为它是新增一个协议能力，不是恢复整个应用数据；两种入口必须使用不同标题和预览。
- 导出先使用原生对话框选择目标，再在 Application mutation gate 内取得一致快照；写入临时文件
  成功并完成 flush 后原子替换目标，取消或失败不得留下半个 ZIP。
- 导入采用 prepare/preview/commit：prepare 完整读取和校验 ZIP、恢复并编译全部协议包、校验
  Workspace/规则/证书引用及应用格式版本；preview 展示 Workspace 数、协议包版本数、启用数、
  证书材料数和将被替换的数据；只有用户确认后才 commit。
- commit 在同一应用 mutation gate 和 SQLite 原子事务内替换 Workspace、选择、设置、协议包及
  启用状态；证书恢复必须有补偿清理。任一最后阶段失败时，数据库、缓存、证书材料和当前运行
  配置均保持导入前状态。
- 导入应用数据属于替换操作，不与当前数据静默合并；运行中的 Listener 或设备网络接管存在时
  必须先阻止导入并说明原因，不能一边运行一边替换其配置来源。

### 验收测试

- 空应用、多个 Workspace、多个协议包版本、启用/停用混合、Relay/本地应答、Socket 规则、
  Android 方案和可移植证书材料都能导出 ZIP，并在全新数据库中精确恢复。
- 导出 ZIP 内实际存在全部协议包文件；ISO 8583 示例目录可单独取出并重新打包为合法协议包。
- 导出取消、目标已存在但未确认覆盖、磁盘写入失败和最后 rename 失败均不破坏旧文件。
- 导入预览准确显示替换范围；取消预览、重复确认、过期 token 和 Workspace 在预览后变化均
  fail-closed，不产生部分写入。
- 非 ZIP、普通协议包 ZIP、旧 JSON 配置、路径穿越、绝对路径、symlink、重复条目、大小写冲突、
  file-parent 冲突、压缩炸弹、超限文件和未知顶层文件全部以稳定错误拒绝。
- 第一个包有效而最后一个包损坏、最后一条规则 Schema 不匹配、最后一个证书不可恢复、数据库
  提交失败等顺序均验证零部分写入和零缓存污染。
- 列表、详情、导出/导入预览、错误、日志和 IPC DTO 中不出现协议脚本正文、证书私钥、密码、
  本机路径或 Payload；只有用户明确确认并选择位置后生成的最终 ZIP 包含协议包源码和其他
  被确认导出的敏感材料。
- macOS 与 Windows 的原生打开/保存对话框只提供 ZIP 过滤器；重复导出结果结构一致，两个平台
  产出的 ZIP 可以交叉导入。

## TODO-WORKSPACE-UX-003：Workspace 创建操作栏保持单行并支持局部横向滚动

需求状态：`待实施`（静态快照，执行中不更新）

优先级：`P1`

依赖：`TODO-APP-DATA-001`

### 问题

Workspace 页头当前在外层和操作区都使用自动换行。窗口宽度不足时，“导入完整应用配置”等按钮
会单独掉到下一行，输入框与操作失去顺序关系，页头高度也会随窗口宽度突然变化。统一应用数据
ZIP 后，Workspace 页仍需展示创建与应用数据迁移入口，因此必须锁定“不自动换行、局部横向滚动”
的响应式规则。

### 设计结论

- Workspace 页头按固定顺序展示“新 Workspace 名称、新建、导出应用数据、导入应用数据”；
  四项组成不可换行的操作栏，始终保持同一行。
- 不再显示单 Workspace 文件和“完整应用配置”文件按钮；复制、选择、保存、删除继续属于右侧
  当前 Workspace 操作，不混入顶部创建与迁移栏。
- 宽窗口继续允许标题/说明位于左侧、操作栏位于右侧；窄窗口可以把完整操作栏整体移动到标题
  下方，但操作栏内部不能换行。
- 操作栏自身使用局部横向滚动；输入框保持可用的最小宽度，所有按钮 `shrink-0` 并显示完整
  标签。不得通过缩短按钮文字、只留图标或把最后一个按钮挤到第二行解决空间问题。
- 横向滚动只发生在操作栏容器，不能使整个 Workspace 页面、右侧详情栏或应用根布局产生横向
  滚动。滚动区域左右边缘使用轻量渐隐或其他视觉提示，表明右侧仍有操作。
- 键盘 Tab 到达当前不可见的输入/按钮时，容器应自动把该控件滚入可视范围；触控板、Shift +
  滚轮和键盘操作均可访问全部按钮。
- pending/disabled 和错误处理语义保持不变；应用数据导入/导出的预览、确认与敏感材料说明由
  `TODO-APP-DATA-001` 定义的 Dialog 负责。

### 验收测试

- 在 1440、1024、800 和最小支持窗口宽度下，输入框、新建、导出应用数据和导入应用数据按
  固定顺序位于同一条 flex row，任何控件的 `offsetTop` 不得因宽度不足变成第二行。
- 窄窗口时操作栏 `scrollWidth > clientWidth` 且可以滚动到最后一个按钮；页面根节点仍满足
  `scrollWidth == clientWidth`，右侧 Workspace 详情不被推出视口。
- 输入框和按钮不被压缩、截字或互相覆盖；聚焦最后一个按钮时它完整进入可视区域。
- pending 时全部创建与迁移操作仍统一禁用，重复点击不产生第二次 IPC；滚动不会关闭预览或
  确认 Dialog。
- 亮色/暗色下分别完成宽窗口与窄窗口截图，确认标题区高度稳定、操作栏没有自动换行。

## TODO-SETTINGS-UX-004：删除重复的设置校验摘要并移出数据迁移

需求状态：`待实施`（静态快照，执行中不更新）

优先级：`P1`

依赖：`TODO-APP-DATA-001`、`TODO-WORKSPACE-UX-003`

### 问题

设置页右侧“配置摘要与校验”重复展示左侧表单中已经可见的值。“已保存的全局设置”、
“保存与生效状态”和“校验结果”还会占用固定宽度，却不能替代字段旁的即时错误或保存时的
后端校验；用户也很难判断该摘要对应当前草稿还是上一次已保存状态。与此同时，“数据与导出”
属于整个应用数据的备份与恢复，不是超时、容量等系统行为设置，把它放在设置页会与 Workspace
页的导入导出入口形成两套心智和两套文件格式。

### 设计结论

- 删除设置页右侧“配置摘要与校验”面板，以及“已保存的全局设置”、“保存与生效状态”和
  “校验结果”三个折叠区；不要只用 CSS 隐藏，不再使用的查询、DTO 和组件应删除或收敛。
- 删除设置页“数据与导出”页签及对应说明。应用数据的“导出应用数据”和“导入应用数据”只在
  Workspace 页顶部单行操作栏出现，并统一使用 `TODO-APP-DATA-001` 定义的 ZIP 格式、预览和
  原子恢复流程。
- 设置页改为单列、充分利用可用宽度的编辑区域，只保留真正属于全局行为的设置分类，例如
  “超时与容量”和“应用”。移除右栏后不能留下大块空白或人为限制表单宽度。
- 不删除 Rust/Application 层的严格校验。点击“保存设置”时仍必须在持久化前执行完整校验；
  字段错误显示在对应输入项或分组附近，无法定位到单字段的错误使用页内 Alert，校验失败零写入。
- “有未保存更改”、“已保存”和确实需要时的“重启后生效”只在底部固定操作区靠近保存按钮
  紧凑展示，不再另设一个摘要卡片。不得把“全部检查通过”长期作为占据版面的成功状态。
- 用户修改相关字段后清除已经过期的字段/全局错误；保存 pending 时锁定所有设置控件、恢复默认、
  放弃更改和重复保存。迟到响应必须以草稿代次隔离，不能覆盖更新后的设置。
- “恢复默认值”、“清除全部配置与数据”、“放弃更改”和“保存设置”的现有确认、危险级别和
  原子性语义保持不变；本项只调整信息架构与错误呈现，不放宽任何安全校验。

### 验收测试

- 设置页的可访问 DOM 中不再出现“配置摘要与校验”、“已保存的全局设置”、“保存与生效状态”、
  “校验结果”或“数据与导出”；Workspace 页恰好存在一组“导出应用数据/导入应用数据”。
- 无效设置保存时，Rust 返回的精确字段路径和安全错误文案显示在正确控件/分组，持久化调用为零；
  修正后保存成功，草稿状态变为已保存且错误清除。
- IPC 拒绝、校验结果迟到、保存期间继续输入、切换设置分类和恢复默认值等交错场景不会让旧响应
  覆盖新草稿，也不会错误显示“已保存”。
- 1440 和 1024 宽度、亮色和暗色下完成截图：移除右栏后表单合理使用宽度，内容密度一致，底部
  操作区不遮挡字段，页面根节点无横向溢出。
- 更新设置页单元测试、UI contracts、前端边界扫描和文案检索；删除失效的摘要测试，并继续满足
  typecheck、ESLint、覆盖率和单文件不超过 500 行门禁。

## TODO-WORKSPACE-UX-005：删除低价值的 Workspace 元数据提取器

需求状态：`待实施`（静态快照，执行中不更新）

优先级：`P1`

依赖：`TODO-ARCH-001`

### 现状审查

“元数据提取”不是空壳：当前运行时会按 Listener 从 HTTP Header、JSONPath、Body 文本或固定值
提取少量字符串，并把结果复制到 HTTP 会话详情和 HTTP 抓包详情。但是它不参与规则匹配、不用于
列表筛选、不支持 Socket 协议字段、不改变转发结果，也没有被其他业务流程消费。用户已经可以在
请求/响应详情中查看原始 Header、正文和解析后的内容，因此独立配置一组提取器只是在详情中重复
展示少量值；“固定值”来源尤其没有实际提取意义。

### 设计结论

- 删除 Workspace 页“元数据提取”页签、“新增提取器”和全部提取器编辑控件，不再把该能力作为
  用户需要理解和维护的 Workspace 策略。
- 删除 HTTP 运行管线中的元数据提取执行、会话/抓包详情中的“提取的元数据”区域，以及只为该
  功能存在的 Application DTO、容量计算和前端渲染；不要留下不可达的隐藏配置或空对象占位。
- 规则如果需要匹配 Header、正文或协议字段，应继续通过各自正式的 HTTP/Socket 规则条件完成，
  不把元数据提取器改名后重新包装成第二套规则变量系统。
- 响应断言、证书引用和连接故障预设不属于本次删除范围；它们仍保留在 Workspace，并分别按自身
  产品价值继续评估和简化。
- 已保存的数据库以及 legacy `.intercept-workspace` / `.intercept-config` v2、v3、v4
  可能包含 `metadata_extractors`。实现时必须保留私有严格 legacy wire 并提供显式版本迁移：
  旧格式可导入，提取器被确定性丢弃，并在导入预览中说明“不再支持且不会恢复”。
  新的应用数据 ZIP v1 从第一版开始就不得写出 `metadata_extractors`，并必须对该字段执行
  unknown-field reject。不能静默改变 legacy 文件，也不能因为迁移中其他未知字段而放宽严格解析。
- 删除前再次执行全仓消费者扫描；如果发现外部产品插件或尚未纳入当前仓库的稳定 API 消费者，
  应停止删除并把具体消费者、输入输出和保留价值写回本 TODO，不能仅凭类型仍存在就保留。

### 验收测试

- Workspace 页及可访问 DOM 不再出现“元数据提取”、“新增提取器”、Header/JSONPath/Body/固定值
  提取来源；其他三个 Workspace 策略页签仍可编辑和保存。
- HTTP 请求/响应、规则、抓包、会话、断点和转发行为保持不变；详情中不再返回或显示
  `extracted_metadata`，且没有为兼容而伪造空提取结果。
- 包含四类旧提取器的数据库记录、`.intercept-workspace` / `.intercept-config` v2、v3、v4
  均能通过迁移恢复其他配置；预览明确报告被移除的提取器数量。新 ZIP v1 中该字段从未存在，
  含有该字段的 ZIP v1 必须被严格拒绝。
- 畸形旧提取器仍按旧格式边界安全解析，不能借迁移绕过路径、大小、类型或未知字段校验；迁移
  失败保持零部分写入。
- 删除所有仅服务于提取器的测试后，补迁移和无回归测试，并继续通过 Rust/TypeScript 类型检查、
  Clippy、ESLint、前端边界扫描、覆盖率和单文件不超过 500 行门禁。

## TODO-CERT-001：下游服务端身份原生支持 P12/PFX 与加密 PEM

需求状态：`待实施`（静态快照，执行中不更新）

优先级：`P0`

依赖：`TODO-ARCH-001`

### 背景

当前 Proxy 入口的“本监听服务端证书身份”只接受包含证书链与未加密私钥的 PEM。
真实代理环境通常交付受密码保护的 P12/PFX，或包含加密 PKCS#8 私钥的 PEM。用户目前必须
在应用外解密、重排证书链并生成临时兼容文件，既容易配错上下游身份，也会扩大未加密私钥
在磁盘上的暴露范围。

### 目标

- “本监听服务端证书身份”支持 `.p12`、`.pfx` 和现有 `.pem`。
- P12/PFX 与加密 PEM 在导入对话框中安全接收密码；密码不进入 Workspace、前端日志、诊断、
  DTO 或普通数据库字段。
- 服务端身份按 `serverAuth`、有效期、DigitalSignature、证书与私钥匹配及完整签发链校验，
  不复用要求 `clientAuth` 的上游客户端身份解析规则。
- 导入时按叶子证书到根证书的签发关系规范化证书链，不依赖 P12 bag 或 PEM 块的原始顺序。
- 导入成功后材料进入现有系统受保护存储；原有未加密组合 PEM 导入保持兼容。

### 验收测试

- P12/PFX：正确密码、错误密码、空密码、现代算法、受支持的既有 RC2/3DES 包、多个私钥身份、
  无私钥、私钥不匹配、缺链、乱序链。
- PEM：未加密 PKCS#8、加密 PKCS#8、错误密码、PKCS#1、乱序证书链、缺少中间证书。
- 服务端用途：有效 `serverAuth` 通过；仅 `clientAuth`、CA 证书、过期/尚未生效证书、缺少
  DigitalSignature 均拒绝。
- 安全边界：密码、私钥和 P12 原文不进入 IPC 返回、日志和错误文本；取消、失败和页面卸载
  清理暂存材料。
- 前端：文件类型、密码输入、忙碌/取消/错误状态、成功后的证书详情与当前 Listener 自动绑定。
- 可移植配置：显式确认导出后可以往返恢复，下游服务端身份不得被误标为上游 mTLS 客户端身份。

### 当前临时兼容方案

在该 TODO 实现前，将服务端 P12/PFX 在应用外转换为：叶子证书、中间证书、可选根证书、
未加密 PKCS#8 私钥组成的组合 PEM；文件仅用于受控测试，权限必须限制为当前用户可读写，
导入并确认受保护引用可用后删除不再需要的明文私钥副本。

## TODO-ANDROID-001A：修复设备 VPN 选择与运行所有者归属

需求状态：`待实施`（静态快照，执行中不更新）

优先级：`P0`

依赖：`TODO-ARCH-001`

### 审查确认

- VPN 的停止、紧急恢复和状态查询当前都以可变的 `selected_serial` 为目标，而不是以实际启动
  VPN 的设备为目标。ADB Reverse 模式只在持有活动 Reverse 映射时阻止切换设备；LAN 直连
  模式不持有该映射，因此运行中仍可切换选择。设备断开或选择改变后，旧设备的 VPN 可能无法
  从桌面端停止，操作还可能错误指向新选择的设备。

### 目标

- 将“当前选择设备”和“VPN 实际运行设备”建模为不同状态；停止、状态查询、紧急恢复及资源
  清理必须始终绑定运行所有者，禁止对新选择设备误执行旧设备操作。
- 设备切换或断开时自动协调生命周期：旧设备仍可达时先安全停止；不可达时保留运行归属并显示
  明确的“设备已断开、等待恢复”状态，设备重连后可自动完成或一键完成停止与清理。
- 明确定义自动停止策略，避免把短暂 USB 抖动误判为永久断开；任何自动恢复都必须幂等，并且
  不得在证据不足时把 VPN 标记为已停止。

### 验收测试

- 设备归属：设备 A 在 LAN 模式运行时选择设备 B，停止/状态/紧急恢复仍只作用于 A；确认 A
  停止并清理后才允许 B 接管。ADB Reverse 模式保持同样语义。
- USB 生命周期：A 运行时拔线、短暂断开、重新连接、序列号仍相同和序列号变化等场景均有确定
  状态；不得误报已停止、不得误停 B，重连后的清理可幂等重试。
- 页面交互：没有当前在线选择设备但仍存在活动运行所有者时，停止/恢复入口仍可见；按钮状态和
  文案反映“可直接执行、等待重连或需要人工确认”，而不是简单按 `selected_serial` 禁用。

## TODO-ANDROID-001B：展示真实转发端点并处理主机 IP 漂移

需求状态：`待实施`（静态快照，执行中不更新）

优先级：`P1`

依赖：`TODO-ANDROID-001A`

### 审查确认

- 页面仅展示方案引用的 Listener 名称及其监听地址，没有展示启动/应用配置后真正发送给设备的
  `proxy_host:proxy_port`，也无法区分当前使用 LAN 直连还是 `ADB reverse -> 127.0.0.1`。
- LAN 直连的桌面 IP 只在启动或应用配置时解析并写入设备路由。电脑网络从 IP A 切换到 IP B
  后，没有网络变化监听、端点重新解析或自动重新应用；LAN 模式的运行就绪检查也不会验证旧
  IP 是否仍属于当前主机或仍可达。ADB Reverse 模式不依赖桌面 LAN IP，不属于该故障范围。

### 目标

- 在设备网络页面展示每条实际运行路由的原始目标、Listener、真实 `proxy_host:proxy_port`、
  传输方式（LAN 或 ADB Reverse）、运行设备序列号和最后解析时间。配置值与运行值必须分开展示。
- 监听电脑接口/IP 变化；LAN 端点失效时重新选择当前可达地址并安全重新应用。重新应用失败时
  将运行状态标记为故障并给出可执行恢复提示，不得继续显示为正常运行。
- LAN 模式的就绪检查加入当前主机地址归属及设备到 Listener 的可达性校验；ADB Reverse 模式
  继续校验 Reverse 映射，不因普通 LAN 地址变化触发不必要的重建。

### 验收测试

- 端点展示：LAN 模式显示实际桌面 IP 与 Listener 端口；Reverse 模式显示设备连接
  `127.0.0.1:<设备端口>` 以及映射到的桌面 Listener；多路由、多 Listener 均逐条准确展示。
- IP 漂移：运行时 Wi-Fi/有线切换、DHCP 续租、IP A 变 IP B、接口暂时无地址后恢复；旧端点
  不再可用时自动重新应用到 B，失败则进入可诊断故障状态。
- 回归：仅使用 ADB Reverse 时电脑 LAN IP 变化不影响现有转发；方案持久化仍不保存一次性的
  桌面 IP 或 ADB 端口，运行端点只作为受控运行事实保存和展示。
