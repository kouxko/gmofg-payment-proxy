# 改为单进程 WebAssembly Component 协议包运行时

## 任务信息

- 任务 ID：`TASK-20260901-001`
- 状态：`进行中`
- 任务日期：`2026-09-01`
- 创建时间：`2026-09-01 10:20:26 +08:00`
- 开始时间：`2026-09-01 17:51:23 +08:00`
- 最后更新时间：`2026-09-03 10:46:55 +08:00`
- 完成时间：`N/A`
- 创建路径：`docs/tasks/pending/2026-09-01/add-managed-webassembly-protocol-packages.md`
- 归档路径：`docs/tasks/completed/<完成日期>/add-managed-webassembly-protocol-packages.md`
- 关键词：`WebAssembly Component`、`Wasm`、`Wasmtime`、`WASI Preview 2`、`WIT`、`single process`、`Boa removal`、`Sidecar removal`、`/packages`、`WebSocket`、`protocol package`
- 任务优先级：`高`
- 优先级理由：任务替换协议包公共运行时和 ABI，删除 Boa、Sidecar 进程及跨平台 external binary 打包链，并影响包导入、内置包、生命周期、网络、文件系统、错误传播、备份恢复和资源所有权；任一边界错误都可能导致错误业务报文、主进程失稳或跨平台不可用，必须执行完整流程。

## 背景与目标

当前本地协议包是严格 JavaScript ZIP，由额外的 Boa Sidecar executable 加载。Tauri 主进程按精确包版本启动子进程，Sidecar 再经 `/packages` WebSocket 注册并执行固定 Hook。Windows 安装包因此必须额外 stage `intercept-proxy-package-sidecar.exe`；首次 Windows workflow 已因该文件缺失而无法产出 MSI、NSIS 和 portable。

`2026-09-01 17:23:52 +08:00` 用户推翻此前“保留 Boa、在通用 Sidecar 增加 Wasmtime”的就绪方案，明确要求产品只运行 Wasm，不再使用 Sidecar，并在同一个应用进程内完成协议包执行。此前关于双运行时、JavaScript 兼容和 Sidecar Transport 的范围、验收与小任务自此失效，不得继续作为实现依据。

新目标是由 Tauri/Rust 主进程直接拥有 Wasmtime Component runtime，导入、持久化、实例化并调用单文件 `.wasm` 协议包。Wasm 继续通过版本化 WIT 导出与 Frame、Decode、Encode、Display 等现有业务 Hook 等价的函数；主进程直接提供已确认的 WASI 与 Host WebSocket 能力，不再通过本地 `/packages` JSON-RPC 绕行，也不再打包或启动额外 executable。

远端外部软件包继续使用现有 `/packages` WebSocket、`package.register` 和 JSON-RPC API 1。该路径用于快速调试源语言实现、观察完整日志和源级错误，避免在 Wasm 编译、符号处理或 Guest/Host ABI 适配阶段丢失诊断信息；它是显式选择的远端运行形态，不是本地 Wasm 失败时的自动 fallback。

## 范围

- 删除 JavaScript ZIP、Boa `LocalSidecarRuntime` 和双运行时分发；本地协议包唯一可执行载荷为单文件 Wasm Component。
- 删除 `intercept-proxy-package-sidecar` binary、`LocalPackageSupervisor` 子进程所有权、临时 ZIP 落盘、Sidecar 注册期限和本地 JSON-RPC 回环。
- 在 Tauri/Rust 主进程内由明确的 runtime owner 管理每个 enabled 精确包版本的 Wasmtime Engine、Component、Store、Instance 和顺序 Hook 生命周期；允许使用进程内 worker task/thread，但不得创建额外进程。
- 将现有 Manifest 合同作为顶层 `intercept-proxy:manifest` 自定义 section 嵌入 Component；Proxy 必须在实例化前静态读取并校验该 section。文件名不作为包身份，包身份继续由嵌入 Manifest 的精确 ID 和 SemVer 决定。
- Wasm 包所需静态资源由语言工具链编译进 Component；运行期可变配置继续由 Proxy 管理，不通过额外同目录文件重新形成隐式多文件包。
- 使用 WebAssembly Component Model 和版本化 WIT world 定义 HTTP/Socket Frame、Decode、Encode、Display ABI；不接受不能满足目标 world 的任意 Core Wasm 作为协议包。
- 保持 `manifest.kind` 只表示 HTTP/Socket；产品不展示 Wasm/runtime 标记，因为本地实现只剩 Wasm。
- 按用户结论，不以安全隔离为目标限制 Wasm Host capability；评估并接入 Wasmtime WASI Preview 2、网络、DNS、HTTP、文件系统、环境、stdio、时间和随机数等 Host 能力。
- 主进程提供版本化的外部 WebSocket Client Host WIT，至少保证 Wasm guest 可访问任意宿主可达的 `ws`/`wss` 地址、收发文本与二进制消息、关闭连接并取得真实连接或协议错误。
- 文件系统不增加 Host capability allowlist：Unix/macOS 将宿主 `/` 以 guest `/` 读写 preopen，Windows 枚举可用盘符并稳定映射为 `/host/<小写盘符>`；宿主操作系统、macOS Sandbox 或当前进程权限拒绝时原样返回真实 WASI I/O 失败，不伪造成功或额外回退。
- Wasm 单文件不设置产品级导入字节上限；读取、持久化、编译或实例化因宿主文件系统、地址空间或内存不足失败时必须真实失败。
- 运行时调用链必须承载异步 WASI/Host WebSocket 调用，同时保持每个精确包版本的 Hook 顺序执行；不得阻塞 Tauri UI/event-loop 线程。
- 复用当前原始包 BLOB、精确身份冲突、启用/停用、重启、应用启动恢复、删除和备份恢复语义，但载荷只保存原始 `.wasm` bytes；是否需要 Schema 迁移以源码和 SQLite readback 决定。
- 将内置 JSON、ISO8583、模板和活动 JavaScript fixture 重写或重新构建为满足相同业务向量的 Wasm Component；不保留 JavaScript 执行回退。
- 仓库内 `examples/` 与 `templates/` 的协议包均必须提供可重复构建的 Rust Component 版本，并由同一仓库命令构建、校验和汇总单文件 `.wasm` 产物；无法由原 Python、Deno 或 Rhai 源码直接生成目标 Component 的实现按原行为重写为 Rust。
- Python 与 Deno 外部软件包源码继续保留为 `/packages` 远端源级调试入口；其 Rust Component 版本用于本地单进程导入，两者不得共享本地 JSON-RPC/Base64 Transport 实现。
- 保留远端 `/packages` 服务、外部软件包注册/在线状态和 JSON-RPC API 1；本地 Wasm 与远端外部软件包在应用端口之后共享业务 Hook 语义，但本地调用不经过 WebSocket。
- 更新作者文档、WIT、可复现多语言示例或最小 fixture、架构文档、ADR、测试矩阵和打包说明。

## 不在范围

- 不保留 Boa、Rhai、Deno、WebView 或 JavaScript ZIP 兼容执行路径。
- 不自动把任意既有 JavaScript 包转换为 Wasm；内置包由本任务提供明确的 Wasm 源码、构建和等价验证，第三方旧包需要作者提供符合新 WIT 的 Wasm Component。
- 不在 UI、公开 ViewModel 或普通协议包操作流程中增加运行时选择器。
- 不允许包提供或选择自定义原生 Sidecar、`.exe`、`.dll`、`.dylib`、`.so` 或外部守护进程。
- 不让 Wasm guest 实现本地 `/packages` 注册、心跳和 JSON-RPC Transport；本地 Hook 由进程内 Rust 端口直接调用。
- 不删除或改变远端外部软件包 `/packages` 公共协议；也不把远端软件包作为本地 Wasm 加载、实例化或调用失败后的回退。
- 不因切换 Wasmtime 顺带改变规则、Listener、Exchange 的业务语义；删除 Sidecar 所必需的端口/适配器重接属于本任务范围。
- 未经用户明确要求，不 push、不创建 PR、不触发远程 CI、不发布、不部署。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-09-01 10:20:26 +08:00` | 用户明确不切换现有运行时：JavaScript 包继续由 Boa 执行，同时新增可直接导入的 WebAssembly 协议包，使其他语言能够实现协议包。 |
| `2026-09-01 10:20:26 +08:00` | 用户明确由 Proxy 管理协议包，目标是避免包以独立外挂软件形式运行；包不得要求安装语言运行时或包专属原生程序。 |
| `2026-09-01 10:20:26 +08:00` | 用户明确产品不展示任何 Wasm 能力或实现差异；实现类型只允许作为内部加载与诊断事实存在。 |
| `2026-09-01 10:20:26 +08:00` | 当前讨论形成推荐方向：通用 Sidecar 保留 `/packages` WebSocket、注册、RPC 和生命周期；Wasm 只导出固定协议函数。用户要求登记任务，但尚未逐项确认该方向的 ABI 和失败语义。 |
| `2026-09-01 10:20:26 +08:00` | 用户明确“安全不做任何限制”。当前按“不通过 capability allowlist 禁止 Wasm 使用网络、文件系统等标准 WASI 外部能力”记录；是否同时取消 CPU、内存、文件大小、RPC 大小和超时等可靠性边界仍需明确。 |
| `2026-09-01 15:47:36 +08:00` | 用户确认 Wasm 协议包不再打包为 ZIP，直接交付单个 `.wasm` 文件；接受把严格 Manifest 嵌入 Component、静态资源编译进 Component，现有 JavaScript 协议包继续保持 ZIP。 |
| `2026-09-01 16:33:34 +08:00` | 用户确认 Sidecar 统一持有 `/packages` WebSocket；Wasm/各语言 guest 只实现版本化 WIT exports，不自行实现注册、心跳或 JSON-RPC Transport。 |
| `2026-09-01 16:33:34 +08:00` | 用户选择由 Sidecar 提供版本化 WebSocket Client Host WIT，跨语言保证 Wasm guest 的外部 `ws`/`wss` 连接、文本/二进制收发、关闭和真实错误能力；不依赖各 guest 语言自行携带兼容 WASI 的 WebSocket/TLS 库。 |
| `2026-09-01 16:33:34 +08:00` | 用户确认文件系统采用完整宿主根映射：Unix/macOS preopen `/`，Windows 枚举可用盘符并映射到 `/host/<小写盘符>`；操作系统或 Sandbox 权限不足时返回真实失败。 |
| `2026-09-01 16:33:34 +08:00` | 用户确认单个 `.wasm` 不设置产品级文件大小上限；保留现有 JavaScript ZIP 和 RPC/Body/Frame/注册/heartbeat 边界，不为 Wasmtime 增加 fuel、内存 limiter、Hook timeout、Busy、自动中断、重试或恢复。 |
| `2026-09-01 16:33:34 +08:00` | 代码合同确认 WIT 分别定义严格 `http-package` 与 `socket-package` world；HTTP 使用 Unicode `string`，Socket 使用 `list<u8>`，递归 Document 使用规范 JSON UTF-8 `string`，Base64 只保留在既有 JSON-RPC wire 适配层；错误继续映射当前稳定错误合同。 |
| `2026-09-01 16:33:34 +08:00` | Wasmtime、`wasmtime-wasi` 与 `wasmtime-wasi-http` 首次实现固定 `=48.0.1`，启用 Component Model、异步执行、WASI Preview 2 和 WASI HTTP 所需 features；WIT 包从 API 1 对应的 `1.0.0` 开始，升级必须通过后续显式兼容任务，不自动接受漂移。安装体积、启动耗时和 RSS 不设产品验收阈值。 |
| `2026-09-01 17:23:52 +08:00` | 用户明确推翻此前保留 Boa/Sidecar 的方案：产品只运行 Wasm，不使用 Sidecar，所有本地 Wasm 协议包在同一个应用进程内执行。此前双运行时、JavaScript 兼容、Sidecar Transport/heartbeat/注册/进程打包验收失效；单文件 Component、版本化 WIT、完整 Host capability、无安全限制、无产品级大小/性能阈值等未冲突结论继续有效。 |
| `2026-09-01 17:32:10 +08:00` | 用户选择方案 B：只删除本地 Sidecar，本地协议包仅以单文件 Wasm 在主进程运行；保留远端 `/packages` 外部软件包，用于快速调试并避免 Wasm 编译或 ABI 适配阶段丢失源级诊断信息。远端路径不是本地 Wasm 失败回退。 |
| `2026-09-01 19:05:26 +08:00` | 用户要求同步检查仓库内全部协议包 examples/templates：能够直接生成 Component 的纳入统一构建，不能直接生成的实现重写为 Rust。源码确认当前共有 AU EFTEX、Nuvei Tango JSON、ISO8583 Deno、Nuvei Tango Rhai 四个 example 与一个 ISO8583 template；全部必须有 Rust Component、局部行为测试和统一构建/Wasmtime 加载验证。Python/Deno 外部调试入口按既有方案 B 保留。 |
| `2026-09-01 22:08:24 +08:00` | 用户明确删除协议包页面“恢复 ISO 8583 示例包”功能，不保留隐藏命令、兼容入口或失败回退；继续保留“导出 ISO 8583 模板”，其产物必须是由仓库 Rust 模板构建的单文件 Wasm Component。并行验证只覆盖现有 Wasm Decode/Display/Encode 与本机重放，不测试文件系统或出站 HTTP。 |
| `2026-09-01 22:45:00 +08:00` | 用户明确本轮不测试文件系统或出站 HTTP Host capability；这些项目从本轮发布候选验收中排除，不得用静态存在性冒充运行验证。 |
| `2026-09-01 23:00:00 +08:00` | 用户授权在本地审查和回归无重大问题后提交并推送当前分支，触发仅 Windows 的 CI，生成未签名 Windows 可执行文件；不得创建 tag、GitHub Release、Android/macOS job 或安装包构建。普通本地/Tauri 构建不得再自动执行 Phase2 release blocker。 |
| `2026-09-02 09:54:16 +08:00` | 用户扩大交付验收：整体审查功能、文档和测试并修复所有确认问题；本地完整验证通过后提交推送，分别触发 `verify-and-build/all` 完整 CI 与 `build-only/windows` Windows-only 快速出包 CI，持续监控到终态并校验 Windows 产物。此授权替代上一条“仅 Windows CI”的限制；仍不创建 tag 或 GitHub Release。 |
| `2026-09-02 11:18:09 +08:00` | 用户确认开发与 CI Rust 工具链升级到正式版 `1.98.0`，并计划卸载本机 `1.97`。活动 toolchain、workspace MSRV、CI 和当前操作文档必须统一到 `1.98.0`；历史测试证据中的实际 `1.97.1` 环境记录保持不可变。 |
| `2026-09-02 12:21:42 +08:00` | `09:54:16` 记录中的旧 `run_mode` 调用方式已被最终 CI 设计替代：完整流程使用 `.github/workflows/windows-release.yml` 并传入 `platform=all`；Windows 快速可执行文件使用独立 `.github/workflows/windows-quick-build.yml`，无输入参数。两条流程使用独立 concurrency group；仍不创建 tag 或 GitHub Release。 |
| `2026-09-02 15:57:27 +08:00` | 用户要求在远端 Windows 设备 `10.0.28.77` 重放以往测试部署并包含 Wasm。范围按本任务既有 `release-app-replay` 与 `wasm-integrated-runtime` 权威用例执行：Plain HTTP 六类有效请求与非法 JSON、内置 ISO8583 Wasm 的 match/miss/非法 Frame，以及可用时的 AU EFTEX Wasm 旧向量；远端拓扑允许把原本单机 loopback 调整为设备监听、当前 Mac 提供受控上游，但输入、规则、阶段和逐字节断言不得改变。MCP 配置写入、远端协议包导入和 Listener 启停属于本次明确授权；不得用配置提交、包在线或 Listener 启动代替数据面 PASS。 |
| `2026-09-02 16:08:24 +08:00` | 用户暂停远端部署，要求测试环境的 AU EFTEX Component 将 BDK 直接写入源码并编译成供外部导入的 Wasm；该产物必须能够重放既有 AU EFTEX 测试记录。现有 71 字节公开旧向量、组件 golden 和 trace verifier 已确认共同使用公开 ANSI 测试 BDK `0123456789ABCDEFFEDCBA9876543210`，因此本轮只嵌入该公开测试值，不写入真实生产 BDK。 |
| `2026-09-02 16:08:24 +08:00` | 验收必须覆盖当前权威旧向量的 Frame、Decode、Display、Encode，并比较请求逐字节往返及 63 字节预期响应；仅编译成功、静态校验成功、导入成功或 Listener 启动均不算完成。完成本地 Wasmtime 数据面验证后再恢复 `10.0.28.77` 的导入与重放。 |
| `2026-09-02 16:29:10 +08:00` | 用户要求修复 ISO Deno Display 的整数兼容问题，并把 Nuvei JSON 的 Display 改为 object/array 递归嵌套 HTML table；不得继续把嵌套 JSON 输出为 `<pre>`。两项修复均需重新构建 Wasm 放入 Downloads 并在远端双向 Display 实测。 |
| `2026-09-02 16:52:48 +08:00` | 用户要求补充测试全部 5 个 Wasm 包的规则。规则验收必须分别证明命中、适用的未命中保持、非法 Frame fail-closed、规则持久化命中计数和上游/客户端实际字节；AU EFTEX 与 Nuvei 的 MAC/只读字段只使用同值动作观测规则命中，不绕过 Encode 合同。 |
| `2026-09-02 17:19:32 +08:00` | 用户要求整理分支、合并应合并内容、删除应删除分支、推送 GitHub、合并到主分支并创建 Release。执行前实时确认 GitHub 当前没有 `main`/`master`，默认分支为 `codex/windows-ci-cache-warmup`；仓库当前版本为 `1.0.0` 且无历史 tag/Release。主分支命名/目标与首个 Release tag 仍需用户确认，确认前可以修复独立的发布阻断、推送当前功能分支并运行验证，但不得猜测创建 `main`、tag 或删除含独立提交/未提交修改的分支。 |
| `2026-09-02 17:24:26 +08:00` | 用户确认一次性执行发布收口方案：以现有默认分支 tip 创建并切换 GitHub 默认分支为 `main`，当前任务分支通过 PR merge 到 `main`，首个 Release 使用 `v1.0.0`；合并与发布验证完成后删除已盘点的 1–6 项安全候选，包括当前任务分支、已被包含的 generalization 分支、干净的 wasm-runtime worktree/本地分支、旧默认分支、失效的 g032 worktree 元数据和未跟踪的根目录 `pnpm-lock.yaml`。保留含 3 个独立提交的 `g032-consolidation` 分支和有未提交修改的 g049 detached worktree。Windows 正式 Release 继续遵守 fail-closed 签名合同；实时 GitHub readback 尚未发现所需 Actions secrets/variable，因此不得在凭据门禁满足前创建 tag。 |
| `2026-09-02 17:32:21 +08:00` | 用户明确暂时不配置 Windows 签名，要求以无签名状态发布。该结论替代上一条的签名门禁：`v1.0.0` tag 在三项签名配置均缺失时必须明确走 unsigned 分支，产物名称和 Release 说明必须标注未签名，不执行或伪造 Authenticode 校验；未来三项配置全部存在时仍执行现有签名与签发者/时间戳校验。部分配置存在属于错误配置并继续 fail-closed，禁止静默降级。 |
| `2026-09-03 09:47:00 +08:00` | 用户要求在 `examples` 中生成 JSON Pretty Wasm 包；随后询问编辑器式语法分色，并明确不得修改 Proxy 代码，无法在当前清洗合同下分色则接受不实现。最终范围只新增 `examples/protocol-packages/json_pretty/` 及对应证据，不修改 `src/` 或 `src-tauri/`。 |
| `2026-09-03 10:09:15 +08:00` | 用户截图显示首次产物导入时报“协议包校验预览数据不完整。未安装任何协议包内容。”。只读定位确认旧包 Manifest 的上下行 Schema 为空，后端返回的两个 Schema 为 `null`，不满足当前前端预览完整性检查；曾临时在包内补充 Schema 以验证该候选原因，未修改 Proxy。 |
| `2026-09-03 10:24:00 +08:00` | 本地 App 使用补充 Schema 的产物仍失败；源码进一步确认后端把 HTTP 方向能力固定投影为 `frame: true`，前端却要求 HTTP 为 `frame: false`。用户随后明确该 HTTP 包本来就没有 Schema，因此撤销临时 Schema，恢复上下行空 Document 元数据。无 Schema HTTP 包在当前 Proxy UI 无法导入；用户此前要求不修改 Proxy，故保留 Host 可运行产物并把 App 导入记为失败。 |
| `2026-09-03 10:33:28 +08:00` | 用户确认该问题严重并明确授权修复 Proxy 中无 Schema HTTP 包的导入合同；该授权替代此前“不得修改 Proxy”的局部限制。HTTP 预览与详情允许上下行 Schema 为 `null`，Socket 仍要求两份合法 Schema；HTTP capability 必须投影为 `frame: false`。用户同时要求 Display 支持自定义 HTML 样式；结合此前编辑器式 JSON 分色目标，本轮只保留经过属性和值白名单过滤的内联视觉 CSS，不开放脚本、事件、外链资源、`<style>`、布局覆盖或全局样式。用户明确要求快速修复、不采用 TDD；本轮直接实现后执行定向回归。 |
| `2026-09-03 10:42:27 +08:00` | 用户截图证明导入成功后入口配置仍报“入口协议包目录数据不完整”；确认第三处目录边界仍无条件要求上下行 Schema。修复范围扩展到 Listener 协议包目录的同一按类型 Schema 合同，不改变代理转发和 Socket 严格校验。 |
| `2026-09-03 10:46:55 +08:00` | 用户在重新启动的本地开发 App 中确认无 Schema HTTP 包导入、目录读取及使用结果“可以了”，并要求提交。本次只创建本地提交，不 push、不触发 CI。 |

## 未确认事项

- 主分支、首个 tag、无签名发布策略和分支清理范围均已确认；实现合同无未确认事项。

## 需求就绪检查

- 问题、用户目标和成功结果：`PASS`，只保留进程内 Wasmtime Component 运行时并删除本地 Sidecar。
- 范围与不在范围：`PASS`，本地只运行进程内 Wasm 并删除 Sidecar；远端 `/packages` 外部软件包继续作为独立调试和接入路径，且不是失败回退。
- 输入、输出和状态变化：`PASS`，本地输入严格为嵌入 Manifest 的单文件 Wasm Component；HTTP、Socket、Document、bytes 和固定 Hook 集合保持既有业务语义。
- 错误行为：`PASS`，guest 错误、trap、非法返回和 WASI I/O 失败映射既有稳定错误；Frame/Decode/Encode fail-closed，Display 保留既有 observation fallback；连接、进程和资源失败不得报告成功。
- 具体示例：`PASS`，以现有 HTTP/Socket fixture 业务向量建立等价 Wasm fixture；HTTP 比较 Unicode/Document/编码后 Body，Socket 比较 Frame、原始 bytes、Document 和编码后 bytes。
- 可重复 PASS/FAIL 验收：`PASS`，本地单文件 Component、进程内 Hook、WASI/Host WebSocket、生命周期、持久化、单 executable 打包和远端 `/packages` 回归均有直接 PASS/FAIL。
- 会改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-09-01 17:51:23 +08:00`。新方案于 `2026-09-01 17:32:10 +08:00` 重新通过需求就绪门禁，并于该时间开始生产实现。

## 问题与根因分析

JSON Pretty 导入属于本任务新增包暴露出的缺陷：Rust 包合同已经允许 HTTP `document.schema = None`，但前端导入预览、导入结果、详情和 Listener 目录解析无条件要求 Schema 对象；同时后端 ViewModel 对 HTTP/Socket 均固定声明 `frame: true`，而前端合同要求 HTTP 为 `false`。前两项导致合法无 Schema HTTP 包在 Host 可运行但 UI 导入预览被拒绝；修复后又由 Listener 目录的同类校验阻止入口选择。正确修复边界是所有消费端按包类型校验 Schema、后端按包类型投影 Frame capability，不给包补造 Schema，也不放宽 Socket 合同。Display 样式需求的边界是现有沙箱 iframe 内的安全内联视觉样式白名单，仍删除主动内容和可能越界的 CSS。

- 当前已验证：`src-tauri/crates/package-runtime/src/lib.rs` 只接受严格 JavaScript ZIP，路径验证只允许 `manifest.json` 和 `.js`。
- 当前已验证：`src-tauri/crates/package-runtime/src/sidecar.rs` 的 `LocalSidecarRuntime` 持有 Boa Context、已评估的 `protocol.js`/`display.js` Module 和固定 exports。
- 当前已验证：`src-tauri/crates/package-runtime/src/bin/intercept-proxy-package-sidecar.rs` 在加载包后统一建立 `/packages` WebSocket并处理固定 JSON-RPC 请求。
- 当前已验证：`src-tauri/crates/infrastructure/src/adapters/local_package_supervisor.rs` 每个精确本地包版本拥有一个由应用管理的通用 Sidecar 进程，并处理启动、替换、停止和退出。
- 当前已验证：`src-tauri/crates/infrastructure/src/sqlite/external_packages.rs` 已把本地完整 ZIP 保存为 `local_archive BLOB`，相同身份需要 Manifest 和原始 archive bytes 一致才可复用。
- 当前已验证：`src-tauri/crates/infrastructure/src/adapters/bundle.rs` 在 Windows 固定查找主程序同目录的 `intercept-proxy-package-sidecar.exe`，并创建 `LocalPackageSupervisor`；`src-tauri/tauri.conf.json` 把 Sidecar 声明为唯一 `externalBin`。删除 Sidecar 可以直接移除该跨平台额外二进制依赖。
- 当前已验证：在线依赖预检显示 Wasmtime `48.0.1` 的 MSRV 为 Rust `1.95.0`；当前 workspace、独立 test-support manifests、本地工具链和 CI 已统一声明 Rust `1.98.0`。版本门槛满足不单独证明 features、最终体积、运行行为或跨平台验收，相关结论仍以本任务的实际测试和 CI 为准。
- 用户已确认：Wasm 协议包是嵌入严格 Manifest 的单个 `.wasm` 文件，不使用 `manifest.json + component.wasm` ZIP；新结论要求删除 JavaScript ZIP 执行路径。
- 当前已验证：应用流水线已通过 `ExternalPackageRpc` 抽象调用 `direction + Frame/Decode/Encode/Display`；本地实现可替换为进程内 Wasm adapter，而不让领域层依赖 Wasmtime。
- 当前已验证：Domain Document 是递归 JSON，而 Component Model/WIT 当前类型定义无环，因此 WIT 使用规范 JSON UTF-8 `string`；Socket 原始输入输出使用 `list<u8>`，避免把 JSON-RPC Base64 泄漏到 guest ABI。
- 当前已验证：当前 Boa Hook 在 Sidecar loop 内同步执行；新 Wasmtime Host I/O 是异步的，进程内 runtime 必须使用不阻塞 Tauri UI/event-loop 的顺序 worker，而不再需要本地 Transport heartbeat。
- 当前已验证：SQLite `local_archive BLOB` 可以原样保存 ZIP 或 Wasm bytes，按 Manifest 与完整 payload 比较精确身份；当前证据不要求 Schema 迁移，仅需把 Rust 内部 archive 命名泛化为 payload。
- 用户已确认：外部 WebSocket 由 Host WIT 跨语言保证；最新方案改由主进程实现该 Host。Unix/macOS 暴露 `/`，Windows 暴露全部可用盘符；Wasm 文件无产品级大小上限；不新增 Wasmtime fuel、内存、Hook timeout 或 Busy 限制。
- 当前已验证：当前本地在线状态并非单纯由包记录决定；`LocalPackageSupervisor` 通过子进程连接 `ExternalPackageRegistryAdapter` 后才完成 online gate。进程内方案必须把 Registry/RPC 依赖重接到直接 runtime capability，不能只删除 spawn 和 `externalBin`。
- 正确实现边界：由主进程内封闭 Wasm runtime owner 持有 Component/Store/Instance、Host capabilities、顺序调用、停用/重启/关闭清理和在线状态；本地路径不得保留 JSON-RPC、自连接、Sidecar fallback 或额外进程。
- 当前已验证：完整桌面 CI run `33598183857` 的 Android、Windows/macOS Verify、Windows MSI/NSIS/portable 全部成功；唯一失败是 macOS DMG 构建。runner 原始错误为 `Error: Unknown option '--volumeName'`，发生在 `scripts/build-macos-universal.mjs` 调用 `diskutil image create from` 时。
- 已确认根因：脚本使用了当前 GitHub macOS runner 不支持的 `diskutil image create from --volumeName` 参数组合；App universal binary 已完成编译，失败边界只在 DMG 容器创建。正确修复边界是改用 macOS 稳定的 `hdiutil create -volname ... -srcfolder ... -format UDZO` 合同并增加命令形状回归测试，不修改 App、签名、版本或其他发布步骤。
- 当前已验证：完整桌面 run `33613664943` 的 macOS Verify 在 `Verify Rust formatting` 失败；本地对当前 head 执行同一 `cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check` 精确复现为 `repository_components.rs` 中单个 `assert!` 的非标准换行。根因是前一轮测试补充后未对该文件应用 rustfmt，修复边界仅为 rustfmt 机械格式化，不改变断言或业务行为。
- 当前已验证：PR CI run `33615348276` 的 Coverage gates 在 `Verify frontend coverage` 失败。本地先清除旧 pnpm `node_modules` 并执行权威 `deno ci`，随后用同一 `deno task check:coverage:frontend` 复现为 63 files/532 tests 全部通过，但 `src/features/protocol-packages/**` branches 为 `638/710 = 89.859%`，低于 90% 门禁且只缺 1 个分支。根因是详情防御性空安装时间分支没有测试；正确修复边界是补充该已有 UI 行为的回归，不降低阈值、不改生产逻辑。
- 当前已验证：全平台 run `33615357063` 的 rustfmt 已通过，随后 `Verify Rust lints` 失败；本地同一 strict Clippy 命令复现 `repository_components.rs` 的仓库 Component 集成测试函数为 160 行，超过 100 行门禁。根因是新增三套包级 Display/Encode 断言仍内联在统一库存测试；正确修复边界是按包提取测试辅助函数，保持输入、断言和生产代码不变。

## 最小改动与最优设计比较

| 方案 | 分析 |
| --- | --- |
| 在主进程内实现唯一 `WasmPackageRuntime`，应用端口直接调用 WIT exports | 删除 Windows/macOS/Linux Sidecar staging、签名、启动、自连接、注册和回收链；本地数据流最短，满足用户单进程要求。需要重接在线状态、RPC capability 和进程失败错误模型。采用。 |
| 保留远端 `/packages` 外部软件包，但禁止作为本地失败回退 | 保留源语言快速调试、完整日志和现有 AU EFTEX 等远端接入；本地生产包仍只有进程内 Wasm。用户选择 B，采用。 |
| Wasm 使用单个 `.wasm`，Manifest 置于顶层自定义 section，静态资源编译进 Component | 产物只有一个文件，可直接导入、持久化和校验；导入前可静态读取身份与 Schema，不需要执行 guest。用户已确认。 |
| Wasm 继续使用 `manifest.json + component.wasm` ZIP | 可以复用现有 ZIP 容器，但增加无必要的外层格式和多文件一致性校验；用户已明确不采用。 |
| 在 Sidecar 中运行 Wasmtime | 能隔离主进程，但继续要求额外 executable、跨平台 staging/签名和进程自连接，直接违反最新要求，废弃。 |
| 让 Wasm/每种语言包自行实现 `/packages` WebSocket | 会复制 TLS、JSON-RPC、注册、心跳和退出；本地进程内路径不需要该 Transport，拒绝。外部业务 WebSocket 继续由 Host WIT 提供。 |
| 在主 WebView 执行 Wasm | 仍是单进程，但难以提供统一 WASI、Host WebSocket、文件系统和强类型 WIT owner，并受 WebView 生命周期影响，拒绝。 |

## 小任务与依赖

| ID | 任务 | 依赖 | 可并行 | 状态 | 验收 |
| --- | --- | --- | --- | --- | --- |
| WPC-01 | 重新关闭单进程/仅 Wasm、远端 `/packages`、错误和生命周期需求 | TASK-20260829-002 当前合同稳定 | 否 | 已完成 | 未确认事项为 0，需求就绪门禁重新 PASS |
| WPC-02 | 新增 superseding ADR、版本化 WIT world 和 HTTP/Socket Wasm fixture | WPC-01 | 否 | 已完成 | ADR 明确替代 Boa/Sidecar 决策；WIT 可生成 Host/guest bindings，fixture 可重建 |
| WPC-03 | 实现单文件 Component 静态 Manifest/world/export 校验和 Wasm-only 持久载荷 | WPC-02 | 否 | 已完成 | 合法文件接受；非法 section/world/export/binary fail-closed；无 ZIP/JS fallback |
| WPC-04 | 在主进程实现 Wasmtime Engine/Store/Instance、WIT、WASI 和 Host WebSocket | WPC-03 | 否 | 已完成 | 八类 Hook、bytes、Document、Display、外部能力和真实错误通过 |
| WPC-05 | 用进程内 runtime owner 替换 Supervisor、本地注册、自连接和 RPC capability | WPC-04 | 否 | 已完成 | 启停、重启、恢复、删除、在线门禁、顺序调用和清理通过，无额外进程 |
| WPC-06 | 将内置包、四个 example、模板和测试向量转换为 Rust Wasm Component，并提供统一构建入口 | WPC-05 | 是（按目录隔离） | 已完成 | 五个仓库 Component 均可一次构建、静态校验、Wasmtime 实例化；局部业务字段和最终 bytes 与当前权威向量一致 |
| WPC-07 | 删除 Boa、Sidecar binary、Tauri externalBin、staging/签名脚本和失效 checker/docs | WPC-06 | 否 | 已完成 | 源码、Cargo、Tauri、macOS/Windows workflow 均无 Sidecar/Boa 活动依赖 |
| WPC-08 | 验证本轮已确认的 Wasm Host、持久化和失败语义 | WPC-04、WPC-05 | 否 | 已完成 | 当前正式 Host runtime 直接加载 Component；AU EFTEX Decode/Display/Encode 旧向量通过；文件系统和出站 HTTP 按用户要求本轮 NOT_RUN |
| WPC-09 | 执行真实 HTTP/Socket 流水线、macOS App 与 Windows 单 executable 验收 | WPC-07、WPC-08 | 否 | 已完成 | macOS Release App 与真实 HTTP/Socket 回放通过；Windows executable-only CI run 33525227567 成功并完成产物校验 |
| WPC-10 | 更新作者指南、ADR、架构、MCP、操作和发布文档并整体对抗审查 | WPC-09 | 否 | 已完成 | 活动文档已同步当前规则和 Wasm-only 合同；独立 code reviewer 与 architect 无提交阻断 |
| WPC-11 | 将本地、workspace MSRV、活动文档与全部 CI Rust 工具链统一到 `1.98.0` | WPC-10 | 否 | 已完成 | `rustc`/`cargo` 实际版本为 1.98.0，活动配置无 1.97，受影响检查和 CI 合同通过 |
| WPC-12 | 在 `10.0.28.77` Windows App 重放历史 HTTP、ISO8583 Wasm 与 AU EFTEX Wasm 部署 | WPC-08、WPC-09、WPC-13 | 否 | 进行中 | 5 个 Socket Wasm 的真实加载、Display、规则命中/miss/fail-closed、客户端/受控上游原始字节、Exchange/diagnostics 和最终运行态已 VERIFIED；历史 HTTP 本轮 NOT_RUN，因此小任务不记为全部完成 |
| WPC-13 | 构建内置公开测试 BDK 的 AU EFTEX 测试 Wasm，并锁定旧向量数据面 | WPC-08 | 否 | 已完成 | Component 不再依赖运行时 BDK 环境变量；组件单测、统一构建、正式 Host Frame/Decode/Display/Encode 和远端 71 字节请求/63 字节响应逐字节断言全部通过，产物 SHA-256 可复核 |
| WPC-14 | 修复 macOS DMG 发布阻断并完成主分支/Release 收口 | WPC-10、WPC-11、WPC-13 | 否 | 进行中 | DMG 使用 runner 支持的命令并有回归测试；当前分支验证通过并推送；PR 合并到用户确认的主分支；tag Release 工作流在无签名配置时明确产出 unsigned 制品并校验全部制品；仅删除用户确认且无独立/未提交工作的分支 |
| WPC-15 | 在 examples 中交付无 Schema 的 JSON Pretty HTTP Wasm 包，并修复导入与安全 Display 样式合同 | WPC-03、WPC-08 | 否 | 已完成 | HTTP null Schema 已在导入、详情和 Listener 目录中通过且 `frame: false`；Socket Schema 严格性不变；Display 保留安全内联视觉样式并继续拒绝主动内容；JSON Pretty 编辑器式分色经用户本地验收 |

此前 WPC-01 至 WPC-10 的双运行时/Sidecar 计划全部失效，由上表替代。共享 `package-runtime`、package contract、规则、文档和 checker 当前仍受 `TASK-20260829-002` 修改；生产实现不得覆盖或撤销工作区现有修改。接口、WIT、Schema、生命周期和文件所有权稳定前不得并行。

## 测试计划

- 旧行为向量：复用现有 JavaScript/Boa 测试输入和 expected 作为迁移前权威业务向量；新生产路径只执行 Wasm，不保留 JS runtime 回归目标。
- WIT/Component：HTTP 与 Socket world 正例；Core Wasm、错误 world、缺 export、类型错误、损坏 binary、缺失/重复/非法 Manifest section、Manifest/WIT 身份不一致以及 ZIP/JavaScript 载荷负例。
- 数据适配：HTTP Unicode string、Socket `list<u8>`、FrameResult 全 variant、Document JSON、Display text 和错误结果逐字段/逐字节比较。
- AU EFTEX 测试 Wasm：嵌入公开 ANSI 测试 BDK `0123456789ABCDEFFEDCBA9876543210`，使用 `release-app-replay` 的 71 字节请求和 63 字节预期响应验证无环境变量条件下的 Frame、Decode、Display、Encode、请求原字节往返及响应原字节生成；产物必须经统一 Manifest 追加入口构建并由正式 Wasmtime Host 加载。
- WASI 外部能力：实际执行 DNS、TCP、UDP、HTTP、Unix/macOS `/` 文件读写、Windows `/host/<盘符>` 文件读写、环境、stdio、时间和随机数；保存目标、输入、响应、文件和错误，不用静态 API 存在替代运行证明。
- Host WebSocket：使用至少两个不同 guest 语言 fixture 实际连接外部 `ws` 与 `wss`，验证文本/二进制收发、关闭和连接/协议失败；证明不依赖 guest 自带 WebSocket/TLS 库。
- 进程内调用：八类 Hook 由应用端口直接调用 Wasmtime runtime，验证顺序执行、异步 Host I/O、停用、重启和关闭；不存在本地 JSON-RPC、注册、Ping/Pong 或 heartbeat。
- 生命周期：导入、复用、身份冲突、启用、停用、重启、应用重启恢复、删除、guest trap 和主进程退出清理。
- 远端外部软件包回归：保留 `/packages` handshake、`package.register`、八类 JSON-RPC、Ping/Pong、断开、重连和完整错误/日志；证明其不是本地 Wasm 失败 fallback。
- 持久化/备份：单文件 Wasm bytes、嵌入 Manifest 和 enabled 状态导出/导入完全一致；当前 Schema 是否无需迁移以 SQLite readback 证明。
- UI：列表、详情、导入预览和操作不出现运行时选择器或旧 JavaScript ZIP 行为。
- 真实流水线：HTTP 与 Socket 各使用一个 Wasm fixture，对 Frame、Decode、Rules、Encode、Server/App 最终 bytes 和 observation 与迁移前权威向量比较。
- 可靠性：验证未配置 Wasmtime fuel、内存 limiter、Hook timeout、Busy、自动中断、重试或恢复；guest trap、Host I/O 失败、宿主资源不足和 App 退出不得被报告为成功。
- 资源与打包：验证 macOS/Windows/Linux Release 只有主 executable、无 Tauri externalBin、无 Sidecar staging/签名/孤儿进程、无需外部 Wasmtime 安装；安装体积、冷启动耗时和 RSS 只作为观察信息。

正式证据保存到 `docs/testing/evidence/<执行日期>/TASK-20260901-001/<用例ID>/`。Component fixture 必须保存源码、WIT、编译工具版本、复现命令、生成 `.wasm`、SHA-256、实际输入输出和跨语言工具链说明；不得只提交不可重建的 binary。

远端 Windows 重放使用 `docs/testing/evidence/2026-09-02/TASK-20260901-001/remote-device-replay-10-0-28-77/`，并以 `derived_from` 同时引用父任务 `TASK-20260901-001`、父用例 `release-app-replay` / `wasm-integrated-runtime` 及其证据路径。执行前保存当前默认 Workspace revision、Listener、规则、已安装包和运行态；远端写入仅使用 MCP 候选的预览/确认/终态流程与 App 现有原生 UI。受控上游在当前 Mac 启动，记录两端 IP/Port；MCP、HTTP、Socket 客户端均绕过系统代理。结束时保留本次部署供检查；若无法完整保留则记录真实最终状态，不静默清理或伪造恢复。

## 对抗审查计划

- 检查 Boa、JavaScript ZIP、Sidecar binary、Supervisor、自连接 JSON-RPC 和 Tauri `externalBin` 是否从生产路径完整删除，没有隐式 fallback。
- 检查删除本地自连接时是否误删远端 `/packages` server、注册、在线状态、调试日志或 API 1；检查远端路径没有成为本地 Wasm 自动回退。
- 检查公共 UI/API 是否残留运行时选择或已删除的 JavaScript/Sidecar 状态，Wasm-only 内部状态是否足以加载、诊断和 fail-closed。
- 检查 Wasm 导入是否真正只接受一个 `.wasm` 文件，顶层 Manifest section 是否唯一、可静态校验、未被工具链剥离且与目标 WIT world 一致。
- 检查 Wasm 包是否能通过额外 native binary、guest 自建本地 `/packages` 或未登记路径绕过 Proxy 生命周期。
- 检查“安全不限制”与现有可靠性边界是否被错误混为一谈，所有移除或保留的限制是否有用户结论和测试。
- 检查 WASI Host state、异步 Store、ResourceTable、网络/文件资源和进程内 worker 在停用、重启、trap 和 App 退出时是否有明确所有者及清理路径。
- 检查 Wasmtime features、Component Model/WIT 版本、构建工具和 fixture 是否固定且跨平台可重建。
- 检查数据库、备份、身份冲突和内置包流程是否真正复用当前合同，没有新增隐式默认、回退或双写。

## 文档影响

- `docs/README.md`：本任务 pending 入口；完成时按规则移除。
- `docs/architecture/decisions/`：新增 Wasm-only 单进程运行时 ADR，显式 supersede ADR-009 的 Boa/Sidecar 决策，不删除历史 ADR。
- `docs/architecture/rules-and-protocol-packages.md`：记录 Wasm 单文件、嵌入 Manifest、主进程 runtime owner、WIT 和数据适配边界。
- `docs/mcp/external-package-integration-guide.md`：增加 Wasm Component 作者接入、构建和诊断说明，并保留远端 `/packages` 快速调试流程。
- `docs/user-operation-guide.md`：保持 UI 不暴露实现类型，只记录统一导入和运行行为。
- `docs/onboarding-guide.md`：补充 package-runtime、Wasmtime/WASI 和进程内 runtime 所有权。
- `docs/testing/release-validation-matrix.md`：增加 Component、完整 WASI、生命周期、资源和跨平台打包层级。
- 作者模板/fixture/WIT：提供可复现的 HTTP/Socket 最小实现和至少一个非 Rust 语言示例；最终语言矩阵需确认。

## 实施记录

- `2026-09-02 11:33:14 +08:00`：按用户要求将 `rust-toolchain.toml` 固定到 `1.98.0`、workspace MSRV 固定到 `1.98`，并为完整 CI、Windows release 和 Windows quick build 中每个 Rust setup 显式指定 `1.98.0`。本机安装 `rustc/cargo 1.98.0`、rustfmt、Clippy 和 `wasm32-wasip2`；修复 1.98 新增的 Host async lint、统一 Tauri command error 边界 lint 与固定长度 hex 分块 lint，不改变 IPC error wire 或业务行为。
- `2026-09-02 09:54:16 +08:00`：用户要求在现有 Wasm-only/规则/协议包交付上执行全仓功能、文档与测试审查，修复确认问题后运行本地完整验证，并推送同一提交同时触发完整跨平台 CI 与独立 Windows-only 快速出包 CI。需求目标、输入输出、失败标准和外部动作范围明确；保留“不测试文件系统/出站 HTTP”的既有人工验收排除项，但完整 CI 中既有自动化门禁照常执行。开始前已确认工作区仅有与本任务无关的 `docs/README.md` 和 `docs/tasks/pending/2026-08-31/` 脏状态，后续提交不得纳入或撤销。

- `2026-09-01 10:20:26 +08:00`：读取当前包合同、Boa runtime、Sidecar WebSocket、Supervisor、ZIP BLOB 持久化、Tauri external binary 配置和既有任务历史；登记高优先级待确认任务。未修改生产源码、配置、运行时或测试。
- `2026-09-01 15:47:36 +08:00`：按用户确认将 Wasm 交付格式固定为嵌入严格 Manifest 的单个 `.wasm` 文件，删除 Wasm ZIP 方案和对应未确认事项；现有 JavaScript ZIP 保持不变。任务仍有 7 项会改变实现方向的未确认事项，状态保持 `待确认`。
- `2026-09-01 16:33:34 +08:00`：在完整读取当前 Manifest、RPC、Document、Sidecar、Transport、Exchange、Supervisor、SQLite 和导入调用链并核对 Wasmtime/WASI/Component Model 官方合同后，用户确认 Host WebSocket WIT、全宿主文件系统映射和 Wasm 文件无产品级大小上限。其余 WIT、Base64、错误、版本、异步 Transport 和既有边界由当前代码合同闭合；未确认事项降为 0，需求就绪门禁 PASS，任务转为 `待实现`。未修改生产源码、配置或测试。
- `2026-09-01 17:23:52 +08:00`：用户因 Windows 额外 executable 无法运行，明确改为只运行 Wasm、删除 Sidecar 并在同一应用进程执行。源码确认 Windows 主程序固定定位 `intercept-proxy-package-sidecar.exe`、Tauri `externalBin` 固定打包该 binary，且本地在线/RPC/lifecycle 依赖 Supervisor 与自连接 Registry。已重写任务目标、范围、方案和小任务，旧需求就绪结论失效；因远端 `/packages` 是否保留仍会改变删除范围，任务退回 `待确认`。未修改生产源码、配置或测试。
- `2026-09-01 17:32:10 +08:00`：用户选择方案 B，保留远端 `/packages` 外部软件包用于快速调试和保留源级诊断信息，只删除本地 Sidecar 并把本地包收敛为主进程内 Wasm。已明确远端路径不是本地失败回退；未确认事项降为 0，需求就绪门禁重新 PASS，任务转为 `待实现`。未修改生产源码、配置或测试。
- `2026-09-01 17:51:23 +08:00`：在独立 worktree `gmofg-payment-proxy-wasm-runtime`、分支 `codex/task-20260901-001-wasm-runtime` 开始实现。先以 TDD 新增单文件 Component 静态合同测试；测试首先因缺少 `read_package_component` 失败，完成唯一 Manifest section、Component encoding、完整 binary 校验后 3 个用例通过。随后新增 world exports 加载负例，测试因缺少 `WasmPackageRuntime` 再次进入 RED。未修改原工作区。
- `2026-09-01 19:05:26 +08:00`：按用户新增范围并行迁移四个 example。ISO8583 Deno、Nuvei Tango Rhai 与 Nuvei Tango JSON 已新增 Rust Component 并通过各自单元测试、Clippy、`wasm32-wasip2 --release` 构建；AU EFTEX 仍在执行。新增 `scripts/build-protocol-package-components.mjs` 与 `pnpm build:protocol-packages`，自动发现 `templates/**/Cargo.toml` 和 `examples/**/component/Cargo.toml`，逐一使用锁文件构建、校验 Component 文件头与嵌入 Manifest，并汇总到忽略提交的 `dist/protocol-package-components/`。新增 package-runtime 集成测试，最终将通过统一命令构建并由 Wasmtime 实例化全部仓库 Component。完整 Proxy、真实链路和打包测试按用户要求等待合并回原工作区后执行。
- `2026-09-01 20:56:42 +08:00`：单进程 Wasmtime runtime、WASI HTTP/文件/环境、Host WebSocket、启停/重启/冷启动、原子启用、稳定 runtime Arc、单文件备份导入导出与 SQLite 原子替换已完成。最终审查发现停用仅摘除外层实例但未取消旧 generation 调用；随即为每代 runtime 增加取消 owner，停用/重启/删除/替换会取消正在执行及排队调用。阻塞于 WebSocket receive 的真实 guest 调用已证明在停用后 100ms 内返回错误。五个仓库 Component 统一测试构建通过；Host 两次启动测试已从旧 ZIP 改为真实 Component。完整 Proxy、真实 wss、跨平台文件系统和安装包验收继续按用户要求延期到合并后。
- `2026-09-01 22:08:24 +08:00`：按最新产品结论物理删除内置 ISO 示例恢复 UI、前端状态/校验、Tauri command、Application facade/port 方法、Infrastructure restore 实现和对应测试；保留 `builtin_archive` 单一导出边界。导出文件名为 `iso8583-ascii-standard-1.0.0.wasm`，字节来自 `templates/socket-protocol/iso8583-standard` Rust crate 经 `wasm32-wasip2` 编译并追加内嵌 Manifest 的 Component，不是源码目录或 ZIP。
- `2026-09-01 22:40:00 +08:00`：直接使用当前 `dist/protocol-package-components/intercept-proxy-au-eftex-component.wasm` 和正式 Wasmtime Host runtime 重放公开上下行旧向量；Frame、Decode、Display、Encode 均通过，编码结果逐字节保持。现有 App 进程未配置 AU EFTEX BDK，因此 App 数据面按真实 `PROTOCOL_PACKAGE_INVALID` fail-closed，不将配置缺失误报为算法回归。
- `2026-09-01 22:55:00 +08:00`：构建并保持运行当前 macOS Release App；真实 HTTP 规则覆盖 Method、Header、Request target wildcard、Plain Body RFC6901、miss 和非法 JSON，真实 Socket Schema 规则覆盖 match/action、miss 和非法 Frame。受控 Server 的实际接收数据、客户端结果和诊断已归档到 `release-app-replay` 证据。
- `2026-09-01 23:05:00 +08:00`：Windows workflow 曾新增 `build-only/windows` raw executable job；后续最终 CI 设计已把该路径迁移为独立 `.github/workflows/windows-quick-build.yml`。当前快速流程不依赖 Android、Verify、installer 或 macOS job，只构建 `intercept-proxy.exe`、检查 OpenSSL 动态依赖并上传未签名 artifact。普通 Tauri build 只执行前端 build，Phase2 release blocker 保留为显式发布门禁而不再自动重复执行。
- `2026-09-01 23:08:58 +08:00`：完成共享源码收口：Wasm Host/Registry 超长文件按职责拆分，托管 Component detail 不再读取远端连接元数据并有回归测试，活动架构/MCP/发布文档删除旧 Nth/one-shot/TLS 规则描述。Workspace check、strict Clippy、Rust fmt、bindings、typecheck、lint、Next production build、source-size 和 diff-check 均通过；进入最终独立审查。
- `2026-09-01 23:18:03 +08:00`：补充托管 Component 详情派生证据：精确 Rust 查询测试 1/1 与协议包前端 7 files/77 tests 通过；详情不调用远端 connection metadata port。同步修正规则文档，明确多条规则独立匹配，不能把它们描述成单条规则 AND/OR 的等价表达。
- `2026-09-02 12:21:42 +08:00`：最终 CI 调用合同收敛为两条独立 workflow：完整流程使用 `windows-release.yml` 的 `platform=all`，Windows 快速可执行文件使用无参数的 `windows-quick-build.yml`。本地 Rust、全部活动 manifest、CI 与操作文档统一到 `1.98.0`；Windows workflow 合同、Deno/Rust pin 合同和最终独立审查通过。
- `2026-09-02 14:15:00 +08:00`：完整 CI run 33590640554 的 Android、macOS Verify、Windows bindings/前端/架构/rustfmt/Clippy/全量 Rust tests 均通过；Windows Verify 在 90 分钟 job 上限到达时取消了正在执行的 independent runtime gates，后续 Windows/macOS 打包因此跳过。根因是 Rust 1.98 Windows 冷缓存完整验证超过既有时间预算，不是测试失败。将 `ci.yml` 与 `windows-release.yml` 的 Verify 上限统一提高到 150 分钟，并增加 workflow 合同断言；不删除或跳过任何验证步骤。
- `2026-09-02 17:38:48 +08:00`：无签名发布合同本地定向测试与 YAML 解析通过后，全平台 run `33613664943` 暴露 `repository_components.rs` 的单点 rustfmt 失败；已在修复前本地用相同命令复现并锁定为纯格式问题，后续仅应用 rustfmt 并重新跑同一门禁。
- `2026-09-02 17:58:52 +08:00`：PR CI run `33615348276` 暴露协议包前端聚合分支覆盖率只差 1 个分支；532 项前端测试本身全部通过。选择给详情视图既有的空安装时间占位符补回归测试，保持 90% 门禁和生产源码不变。
- `2026-09-02 15:57:27 +08:00`：开始 WPC-12 远端 Windows 重放。MCP `tools/list`、`mcp_environment_capabilities` 与 `workspace_list` 已实时成功；远端当前只有默认 Workspace revision 1、一个 disabled HTTP Listener、无运行 Listener。内置 `iso8583-ascii-standard@1.0.0` 为 managed Wasm、enabled/online/valid，Frame/Decode/Encode/Display 与双向 Schema 可读；`au-eftex@1.1.0` 未安装。远端日志路径确认 Windows 用户数据目录，RDP 3389、SMB 445、MCP 17653 与外部包 8765 可达。以上只读盘点不算部署或重放完成。
- `2026-09-02 16:43:08 +08:00`：修复 ISO Deno Host 整数规范化兼容并发布 `1.0.1`；Nuvei Rhai `1.0.1` 将 object/array 递归渲染为 nested table。远端五包逐字节重放通过后，用户截图证明另一个 `nuvei-tango-json@1.0.0` 仍在输出原始 JSON；随后只修改其 Display renderer、升级到 `1.0.1` 并增加正式 Host 回归，重新统一构建 5 个 Wasm。
- `2026-09-02 17:02:32 +08:00`：导入最终 Wasm 后通过 MCP 候选预览/提交创建 `Remote Wasm Rules Replay 20260902`。5 条 Listener 全部 running 且无 fault；5 条规则各命中 1 次，ISO 两条规则把 MTI `0200` 改为 `0100`，AU 与两套 Nuvei 使用同值动作并以持久化命中计数证明执行。4 条 miss 原字节保持且不增加计数；4 条非法 Frame 在上游前按预期 `DECODE_FAILED`/`PROCESSING_FAILED` fail-closed。13 条 Exchange 中 9 completed、4 expected failed；两套 Nuvei 的命中/miss、上下行 Display 均为递归 nested table 且无 `<pre>`。证据见 [`remote-device-replay-10-0-28-77`](../../testing/evidence/2026-09-02/TASK-20260901-001/remote-device-replay-10-0-28-77/README.md)。
- `2026-09-03 10:46:55 +08:00`：新增独立 `examples/protocol-packages/json_pretty/` HTTP Component、包级锁文件、构建器、编辑器式 JSON 分色和单文件产物。修复后端 HTTP `frame: false` 投影，以及前端导入预览/结果、详情和 Listener 目录的 HTTP nullable Schema 合同；Socket 仍要求双向合法 Schema。Display 沙箱保留安全内联视觉样式并删除主动内容和越界 CSS。包级 4/4、相关前端 69/69、typecheck、Rust 导入测试和开发构建通过；最终产物 `161042` bytes，SHA-256 `5b7ebda09f3c71c79837e4df6447bafd04a92a57e5421ef283d25f152b897ee3`。用户在重新启动的本地 App 中确认导入、目录与使用“可以了”。证据见 [`json-pretty-wasm-example`](../../../testing/evidence/2026-09-03/TASK-20260901-001/json-pretty-wasm-example/README.md)。

## 修改文件

- `docs/README.md`
- `docs/tasks/pending/2026-09-01/add-managed-webassembly-protocol-packages.md`
- `examples/**/component/`
- `templates/socket-protocol/iso8583-standard/`
- `scripts/build-protocol-package-components.mjs`
- `src-tauri/crates/package-runtime/tests/repository_components.rs`
- `examples/protocol-packages/json_pretty/`
- `docs/testing/evidence/2026-09-03/TASK-20260901-001/json-pretty-wasm-example/`
- `docs/testing/evidence/README.md`
- `src/features/protocol-packages/protocol-package-import-model.ts`
- `src/features/protocol-packages/protocol-package-model.ts`
- `src/features/listeners/socket-listener-model.ts`
- `src/features/shared/protocol-safe-display.tsx`
- `src-tauri/crates/infrastructure/src/adapters/external_package_registry/views.rs`

## 附加文件

- 当前依赖任务：`TASK-20260829-002`，其现有 JavaScript/Boa/统一 `/packages` 实现和测试向量是本次替换基线；不得撤销该任务的无关工作区修改。
- Wasmtime Component API：<https://docs.rs/wasmtime/latest/wasmtime/component/index.html>
- Wasmtime WASI Preview 2：<https://docs.rs/wasmtime-wasi/latest/wasmtime_wasi/p2/index.html>
- Wasmtime WASI HTTP：<https://docs.rs/wasmtime-wasi-http/latest/wasmtime_wasi_http/p2/index.html>
- WebAssembly Component Model 语言支持：<https://component-model.bytecodealliance.org/language-support.html>
- WebAssembly metadata：<https://docs.rs/wasm-metadata/latest/wasm_metadata/>

## 验收结果

- `VERIFIED`：本地 Component 静态读取仅接受合法 Component encoding、唯一合法 `intercept-proxy:manifest`，并在实例化时核验 Manifest 选择的 HTTP/Socket world exports。
- `VERIFIED`：Rust HTTP fixture、内置 Socket template 与 Host WebSocket 由 Wasmtime 在当前进程实际加载调用；package-runtime 合同测试 11/11 通过。
- `VERIFIED`：AU EFTEX 5、ISO8583 Deno 3、Nuvei Tango Rhai 5、Nuvei Tango JSON 5、ISO8583 模板 2 项逻辑测试全部通过；五个 release Component 已统一构建并由 Wasmtime 实际加载调用。
- `VERIFIED`：Application 414/414、Infrastructure 463/463、备份归档 24+7+8、相关 UI 97/97、严格 Clippy、typecheck、lint、bindings 和架构门通过。
- `VERIFIED`：当前 macOS Release App 中 HTTP Method/Header/path wildcard/Plain Body 条件与动作、miss、非法 JSON fail-closed，以及 Socket ISO8583 Schema match/action、miss、非法 Frame fail-closed 均通过；App 保持运行供用户检查。
- `VERIFIED`：当前 AU EFTEX Component 经正式 Host runtime 的上下行旧向量 Frame/Decode/Display/Encode 通过；当前 App 数据面因未配置 BDK 明确失败，属于配置阻断，不是成功验收。
- `VERIFIED`：Windows executable-only CI run [`33525227567`](https://github.com/kouxko/gmofg-payment-proxy/actions/runs/33525227567) 在提交 `bbdd7eb848178d7516b8091140f86d5d8d420f65` 上成功；仅 `build-windows-executable` 执行，Android、Verify、installer 和 macOS 均跳过。Artifact `Intercept-Proxy-unsigned-executable-x64` 未过期，下载后的 `intercept-proxy.exe` 为 PE32+ x86-64 GUI，80,178,176 bytes，SHA-256 `09918b5754f65516cd52a7edcd060ec5bae2de3d2b61875249b758cba5542e91`。
- `VERIFIED`：远端 `10.0.28.77` 的 5 个 Socket Wasm、5 条规则命中、4 条 miss、4 条非法 Frame fail-closed、两套 Nuvei 递归嵌套 Display、13 条 Exchange 和 5 条 running Listener 均有可重放证据；AU 71/63 bytes 使用内置公开测试 BDK 完成真实数据面。
- `NOT_RUN`：远端历史 HTTP 本轮未重跑；WPC-12 因此保持进行中，不把 Socket/Wasm PASS 扩大为 HTTP PASS。
- `NOT_RUN`：文件系统和出站 HTTP Host capability 按用户明确范围不执行。
- `VERIFIED`：`json-pretty@1.0.0` 的最终无 Schema 单文件 HTTP Component 为 `161042` bytes、SHA-256 `5b7ebda09f3c71c79837e4df6447bafd04a92a57e5421ef283d25f152b897ee3`；Display 输出编辑器式类型分色，当前正式 Host runtime 实际 Decode/Display/Encode 通过。
- `VERIFIED`：HTTP nullable Schema 与 `frame: false` 在导入预览/结果、详情、Listener 目录及 Rust ViewModel 统一；Socket 双向 Schema 与 `frame: true` 合同保持严格。用户在当前本地开发 App 实际确认无 Schema HTTP 包可导入、目录可读取并可使用。

## 测试结果

- `PASS`：Rust `1.98.0` 下 workspace all-target/all-feature check、strict Clippy `-D warnings` 与 rustfmt；Deno 工具链/CI pin 合同 4/4、Windows workflow 合同 2/2、package-runtime 14/14、exchange UI layer 16/16。
- `PASS`：`cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-package-runtime`，repository 1/1、合同 11/11。
- `PASS`：`cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application --lib`，414/414。
- `PASS`：`cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --lib`，463/463；新增 SemVer 排序定向测试另行 PASS。
- `PASS`：Host 两次启动 Component 恢复测试 1/1；备份归档测试 24+7+8。
- `PASS`：`pnpm test:protocol-packages`，五个 Component 的逻辑测试、release 构建和汇总校验全部通过。
- `PASS`：相关前端 Vitest 97/97；typecheck、lint、bindings、architecture、严格 Clippy 全部通过。
- `PASS`：当前 macOS Release App HTTP 与 Socket 真实数据面回放，证据见 [`release-app-replay`](../../testing/evidence/2026-09-01/TASK-20260901-001/release-app-replay/README.md)。
- `PASS`：五个仓库 Component 的正式 Host runtime/集成证据见 [`wasm-integrated-runtime`](../../testing/evidence/2026-09-01/TASK-20260901-001/wasm-integrated-runtime/README.md)。
- `PASS`：托管 Component 详情隔离回归见 [`managed-component-detail-regression`](../../testing/evidence/2026-09-01/TASK-20260901-001/managed-component-detail-regression/README.md)。
- `PASS`：远端 5 个 Wasm 与规则完整重放见 [`remote-device-replay-10-0-28-77`](../../testing/evidence/2026-09-02/TASK-20260901-001/remote-device-replay-10-0-28-77/README.md)；规则 hit 5/5、miss 4/4、非法 Frame fail-closed 4/4，远端 Listener 5/5 保持 running。
- `PASS`：Workspace all-target/all-feature check 与 strict Clippy、Rust fmt、bindings、typecheck、协议包 UI 77 项、Next production build、source-size、diff-check。
- `PASS`：Windows executable-only CI run 33525227567；Cargo Release、OpenSSL DLL 依赖拒绝门和 artifact upload 均通过，本地下载产物类型、大小与 SHA-256 已核验。
- `NOT_RUN`：真实 wss、跨平台文件系统和出站 HTTP按本轮用户范围排除。
- `PASS`：`deno run -A examples/protocol-packages/json_pretty/build.mjs`，Rust 逻辑测试 4/4、`wasm32-wasip2 --release`、严格 Manifest section 校验和产物生成通过。
- `PASS`：JSON Pretty 包的 rustfmt、Clippy `-D warnings`、`deno check`、SHA-256 校验与当前 Wasmtime Host 加载；最终产物为 `161042` bytes。
- `PASS`：协议包导入/详情/Display 定向前端测试 20/20，Listener 目录与 HTTP 入口相关前端测试 49/49，`pnpm typecheck`，Rust HTTP Manifest 导入投影定向测试，以及本地 `deno task tauri:dev` 编译启动和用户验收。

## CI 情况

- `SUCCESS`：[`Windows signed release and cache warmup / run 33525227567`](https://github.com/kouxko/gmofg-payment-proxy/actions/runs/33525227567)，head `bbdd7eb848178d7516b8091140f86d5d8d420f65`。`build-windows-executable` 30m29s 成功；Android、Verify、installer、macOS 均 `SKIPPED`。唯一 artifact 为 `Intercept-Proxy-unsigned-executable-x64`（GitHub archive 26,879,183 bytes，artifact digest `sha256:d498f8b47c2fcd3f3315ae1c68c8a18701a7ea8dc624d98fd10d7be0932fa601`，expires 2026-11-30）。未创建 tag 或 GitHub Release。
- `CANCELLED`：[`Desktop release / run 33590640554`](https://github.com/kouxko/gmofg-payment-proxy/actions/runs/33590640554)，head `50288ddb495bb8279c95a4ef7dcce003590de373`。Android 与 macOS Verify 成功；Windows Verify 的 Rust tests 成功，independent runtime gates 在 job 启动满 90 分钟时被强制取消，Windows installer/portable 与 macOS Universal 均因此跳过。已按实际耗时修正 Verify timeout，等待新提交复跑；未创建 tag 或 GitHub Release。

## 完成总结

- `当前 Wasm/规则交付完成`：AU EFTEX 已内置公开测试 BDK，5 个最终 Wasm、两项 Display 修复和远端规则数据面均通过。远端 5 条 Listener 与受控上游保持运行供检查。历史 HTTP 未按本轮范围重跑，完整 CI 的 Windows 冷缓存超时复跑也尚未完成，因此总任务继续保持进行中，不归档。
- `JSON Pretty 示例包完成`：无 Schema 单文件 HTTP Component、导入/详情/入口目录合同、安全自定义 Display 样式和编辑器式分色已通过定向自动化与本地 App 用户验收。总任务因 WPC-12/WPC-14 仍在进行中而继续保持进行中，不归档。
