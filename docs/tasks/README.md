# 已完成任务索引

本文按任务最终完成日期记录已经实现并验收的功能。最新日期排在最前面；同一天按完成时间倒序排列。

## 2026-09-03

| 完成时间 | 任务 | 实现功能 | 验收结果 | 优先级 | 关键词 |
| --- | --- | --- | --- | --- | --- |
| 17:10:00 +08:00 | [建立 GitLab CI 验证与 Windows 构建流程](completed/2026-09-03/create-gitlab-ci-pipeline.md) | 为内部 GitLab 项目建立 Android Companion、coverage、Windows 完整验证及明确未签名 Windows 打包四阶段流程，固定工具链版本与下载 SHA，并增加 CI 合同测试和开发文档 | PASS_WITH_REMOTE_PIPELINE_NOT_RUN；GitLab 17.4 CI Lint valid 且无 error/warning，合同测试 8/8、Deno lint、ESLint、YAML、Shell 和 diff 检查通过，独立复审 P0/P1/P2=0；未推送或触发真实 Runner | 高 | GitLab CI、Android Companion、coverage、Windows、unsigned artifact、Deno、Rust |
| 16:48:57 +08:00 | [协议 Display 宽表横向滚动](completed/2026-09-03/make-protocol-display-tables-horizontally-scrollable.md) | 安全清洗时为每张 table 增加独立横向滚动容器，保留原生 table 语义与可读列宽，并覆盖协议包祖先节点继承的任意断行策略 | PASS_WITH_USER_UI_VERIFICATION_PENDING；安全 Display、规则页与帮助 35/35、typecheck、lint、fmt、diff check 和 Next production build 全部通过；UI 横向滚动由用户自行验证 | 高 | 抓包、Exchange、Display、table、横向滚动、iframe |
| 16:44:34 +08:00 | [GMO-FG DLL Display 按 JSON 树折叠](completed/2026-09-03/render-gmofg-dll-display-as-collapsible-json-tree.md) | GMO-FG DLL Display 按 Object、Array、字段与数组下标生成原生折叠树；根“基本信息”默认展开、子容器默认收起，标量保留分色 table，移除 `$` 技术路径 | PASS_WITH_RUNNING_APP_VISUAL_NOT_RUN；包级 14/14、Host 1/1、安全 Display 13/13、fmt、strict Clippy、lint、typecheck、Wasm 与 Next production build 全部通过；运行中桌面 App 点击式视觉验收 NOT_RUN | 高 | GMO-FG、Payment DLL、Display、JSON tree、details、summary、table |
| 16:39:06 +08:00 | [简化可独立运行的 Android 弱网配置界面](completed/2026-09-03/simplify-standalone-weak-network-ui.md) | 首屏收敛为场景与五项常用弱网效果，明确无代理可独立启动；代理、范围、运行保护和专家参数渐进披露，并修正三个设备操作按钮等宽填满 | PASS_WITH_KNOWN_UNRELATED_SOURCE_SIZE_FAILURE_AND_MANUAL_NOT_RUN；前端 90/90、typecheck、lint、Next 生产构建与独立复审通过；界面观感由用户检查，仓库源码大小仅被 6 个无关既有文件阻断 | 低 | Android、弱网、独立运行、场景模板、渐进披露、QNET、PerfDog |
| 16:36:05 +08:00 | [设备端 VPN APK 最低兼容 Android 7](completed/2026-09-03/support-android-7-companion.md) | 主 APK、两个探针和四 ABI Rust 数据面最低版本统一为 API 24；API 24/25 使用普通 Service 与旧通知构造，API 26+ 保持前台 Service 和 Notification Channel | PASS_WITH_ANDROID_7_RUNTIME_NOT_RUN；JVM 28/28、lint 0 error、release/探针构建、固定签名、四 ABI、zipalign、16 KiB 对齐与独立复审通过；无 API 24/25 设备或 AVD，真实安装/VPN 数据面 NOT_RUN | 高 | Android 7、API 24、VPN、Companion、Foreground Service、Notification |
| 16:20:43 +08:00 | [新建规则立即显示完整条件与动作表单](completed/2026-09-03/show-complete-rule-form-before-metadata.md) | 新建规则在 Listener/阶段能力加载后立即展示说明、匹配条件、对应动作和保存按钮，名称/优先级只控制提交有效性，不再控制内容可见性 | PASS；规则 UI 14/14、typecheck、lint、正式 macOS App 构建、签名及安装界面验证通过；CI NOT_RUN、对抗审查按用户要求跳过 | 低 | 规则、新建规则、表单、条件、动作、优先级 |
| 16:10:00 +08:00 | [交付 GMO-FG Payment DLL downstream Wasm 协议包](completed/2026-09-03/gmofg-payment-dll-downstream-package.md) | 交付 `0000/0001/0002` 与 D48 的完整字段 Decode、可编辑 Document、自动 Length Encode 和分色 table Display；Proxy 安全渲染资源门禁调整为 1 MiB / 8192 节点且其他安全边界不变 | PASS_WITH_EXTERNAL_NOT_RUN；包级 13/13、Host 1/1、安全 Display 11/11、strict Clippy、fmt、typecheck、lint、Component 构建与 Manifest 校验通过；真实 Server/Android/CI NOT_RUN | 高 | GMO-FG、Payment DLL、downstream、Wasm、Decode、Encode、table Display |
| 15:46:43 +08:00 | [抓包运行记录按建立时间最新优先](completed/2026-09-03/show-latest-exchange-observations-first.md) | ExchangeObservationStore 在分页前按连接建立顺序倒序投影，抓包第一页顶部稳定显示最新连接并明确表头文案，保持单条事件时序与淘汰逻辑不变 | PASS；Rust 分页 1/1、Store 9/9、抓包前端 10/10、typecheck、lint、fmt、正式 macOS App 构建、签名及安装界面验证通过；CI NOT_RUN、对抗审查按用户要求跳过 | 高 | 抓包、ExchangeObservation、建立时间、倒序、分页 |
| 15:32:32 +08:00 | [统一 HTTP 响应方式三选项下拉框](completed/2026-09-03/align-http-response-mode-selector.md) | HTTP Listener 用单一 Select 表达按请求目标转发、固定 Server 和本机应答，移除独立固定 Server 开关并保留既有 topology/证书合同 | PASS；2 文件 32 项 UI/证书回归、typecheck、lint、正式 macOS App 构建、签名及安装界面验证通过；CI NOT_RUN、对抗审查按用户要求跳过 | 低 | HTTP、响应方式、Select、固定 Server、LocalServer、UI |
| 15:04:03 +08:00 | [支持 HTTP LocalServer 与文本 Mock Body](completed/2026-09-03/http-local-server-and-text-mock-body.md) | HTTP Listener 显式选择 RemoteServer/LocalServer，Local 模式复用双向 Pipeline 且无真实上游；Mock 公共 body 改为文本并按响应 codec 写出，同时修复规则名称清空与 Path 选择器溢出 | PASS；Local HTTP 200/D48、双向规则各 1 次、公共合同/兼容/MCP/UI/绑定/静态门禁通过；CI NOT_RUN、对抗审查按用户要求跳过 | 高 | HTTP、LocalServer、MockResponse、BodyCodec、MCP、Rule、Path |
| 12:37:48 +08:00 | [通过 MCP 验证并配置 Payment DLL 的 D48 Mock](completed/2026-09-03/configure-payment-dll-d48-mock.md) | 从 HTTP 抓包生成启用的 request-stage terminal Mock，保留固定 Root、Listener、Android 路由、上游身份引用和原规则，并完成无真实上游的本机命中重放 | PASS_WITH_MCP_FULL_CANDIDATE_INCOMPATIBILITY；Mock 命中 1 次、HTTP 200/118 字节 D48 响应逐字段一致；MCP 完整候选因动态签发 TLS 表达不兼容而未 apply | 高 | MCP、Payment DLL、HTTP、MockResponse、D48 |

## 2026-09-02

| 完成时间 | 任务 | 实现功能 | 验收结果 | 优先级 | 关键词 |
| --- | --- | --- | --- | --- | --- |
| 18:29:44 +08:00 | [恢复跨平台固定测试 Root CA](completed/2026-09-02/restore-fixed-test-root-ca.md) | Proxy 恢复随包固定测试 Root 与私钥，不同安装共享同一信任锚并继续按监听地址动态签发叶子证书 | PASS_WITH_REMOTE_HANDSHAKE_NOT_RUN；Rust 45 项、前端 8 项及证书逐字节比较通过；用户明确跳过对抗审查，远端部署与真机握手 NOT_RUN | 高 | Root CA、固定测试证书、Android Payment、TLS、证书指纹 |
| 11:03:39 +08:00 | [将完整前端与 Tauri 工具链迁移到 Deno](completed/2026-09-02/migrate-entire-frontend-toolchain-to-deno.md) | 本地开发、测试、正式 Next/Tauri 构建及 CI 配置统一由 Deno 驱动，删除 pnpm 锁、Node overlay 和活动 Node/pnpm 命令，并增加工具链回归合同 | LOCAL_VERIFIED_CI_NOT_RUN_WITH_KNOWN_BLOCKERS；Deno-only 532 个前端测试、Next 13 路由和 macOS App 构建通过；远程 CI NOT_RUN，严格 audit 与品牌扫描失败已保留 | 高 | Deno、Next.js build、Tauri build、deno ci、GitHub Actions |
| 10:35:23 +08:00 | [将 Deno 设为默认开发启动工具链](completed/2026-09-02/default-deno-development-toolchain.md) | 基础 Tauri 开发配置默认执行 Deno，Node.js + pnpm 通过独立 overlay 保持兼容；完整质量门禁与正式构建继续使用 pnpm | VERIFIED_WITH_APP_STATE_BLOCKER；配置路由、两套 Next.js 与 Rust App 启动通过；两套入口主窗口均被本机既有 `CERTIFICATE_ROOT_REVOKED` 状态阻断，未清理应用数据 | 低 | Deno default、Node.js compatibility、beforeDevCommand、Tauri overlay、pnpm |
| 10:22:00 +08:00 | [支持 Node.js 与 Deno 双开发工具链](completed/2026-09-02/support-node-and-deno-development-toolchains.md) | 保留 Node.js + pnpm 权威入口，增加 Deno 官方 npm/Node 兼容配置、锁文件及独立 Next.js/Tauri 开发启动 overlay | VERIFIED；Deno-only 安装、Next/Tauri 主窗口与 Node/pnpm 回归通过；完整门禁、release bundle、CI 和跨平台矩阵按范围 NOT_RUN | 低 | Node.js、Deno、Next.js、pnpm、Tauri CLI、dual toolchain |

## 2026-08-28

| 完成时间 | 任务 | 实现功能 | 验收结果 | 优先级 | 关键词 |
| --- | --- | --- | --- | --- | --- |
| 21:42:26 +08:00 | [修复外部 Workspace 提交后管理页保持旧快照](completed/2026-08-28/fix-workspace-management-external-refresh.md) | 让 Workspace 管理页消费外部提交事件，刷新列表与详情权威快照，同时保留仅名称草稿并显式标记读取失败与陈旧状态 | PASS_WITH_NOT_RUN；组件回归、全量前端、正式构建和独立审查通过；重启后点击式数据面重放因 UI 自动化通道不可用未执行 | 高 | Workspace 管理、snapshot_required、草稿合并、陈旧状态、真实 App |
| 21:40:47 +08:00 | [统一 HTTP 与 Socket 规则抽象、阶段和持久化合同](completed/2026-08-28/unify-http-socket-rule-abstraction.md) | 使用单一 RuleDefinition、统一阶段、单一持久化集合和公共接口，并在同一 HTTP 规则内联合处理 Header 与 Document | PASS；全栈自动化、正式构建和独立审查通过 | 高 | RuleDefinition、HTTP Document、Socket、统一持久化、MCP |
| 21:39:24 +08:00 | [ADB 或桌面控制失联后可选自动关闭 Android VPN](completed/2026-08-28/android-vpn-stop-on-adb-device-missing.md) | 默认开启逐设备控制租约；连续失联 5 秒后只关闭当前 generation 的 VPN/TUN，并保持其他设备和 Listener 不变 | PASS_WITH_NOT_RUN；自动化与独立复审通过，真实设备拔线与桌面异常退出未执行 | 高 | Android VPN、控制租约、heartbeat、generation、ADB |
| 12:38:59 +08:00 | [修复运行中 App 重放发现的 Mock 草稿与入口刷新问题](completed/2026-08-28/fix-running-app-replay-findings.md) | 排除 Mock 草稿中的托管长度 Header，并在 Environment commit 后以统一事件失效当前 Workspace、入口与规则能力查询 | PASS_WITH_NOT_RUN；自动化与运行中 App 数据平面/刷新重放通过，用户确认无需继续额外按钮探索 | 高 | HTTP Mock、Content-Length、Workspace refresh、MCP apply |
| 12:38:59 +08:00 | [Socket Exchange 连接状态语义修复](completed/2026-08-28/socket-connection-status-semantics.md) | 将抓包状态统一为保持连接、正常结束和异常结束，保留原始错误且不推断业务结果 | PASS；抓包回归、类型检查、lint 和源码大小门禁通过 | 低 | Socket、Exchange、连接状态、UI |
| 10:12:34 +08:00 | [当前运行 App 的 Proxy 与模拟 Server 归档场景重放](completed/2026-08-28/running-app-proxy-archive-replay.md) | 在用户已启动的 Release App 中用临时 HTTP/TCP 模拟 Server 重放真实 Proxy、抓包、Exchange、日志和规则能力，并恢复空 Workspace | FAILED_WITH_NOT_RUN；HTTP/Socket、日志倒序和清理通过；Mock 草稿与 UI 刷新失败；TLS/mTLS、外部包和 Android 真机未运行 | 高 | running App、Proxy、mock server、HTTP、Socket、MCP |

## 2026-08-27

| 完成时间 | 任务 | 实现功能 | 验收结果 | 优先级 | 关键词 |
| --- | --- | --- | --- | --- | --- |
| 22:27:08 +08:00 | [最终归档场景复跑与 MCP 验证经验指南](completed/2026-08-27/final-archive-replay-and-mcp-validation-playbook.md) | 重跑归档和运行中 App 场景，并发布 MCP 只读验证指南，完成完整 CI 与 Windows 测试包交付 | PASS_WITH_NOT_RUN；自动化、App/MCP、完整 CI 与 Windows 构建通过；真机和授权外部交易保持 NOT_RUN | 高 | final validation、replay、MCP、Windows CI |
| 22:27:05 +08:00 | [规则创建入口与协议入口绑定修复](completed/2026-08-27/rule-creation-entry-requirements.md) | 删除空白规则并要求普通 HTTP/Body/Socket 规则绑定当前 Workspace 的兼容 Listener，完成旧数据原子迁移 | PASS；全量回归、对抗审查、完整 CI 与 Windows 构建通过 | 高 | 规则、Listener、Workspace、SQLite 迁移 |
| 22:27:02 +08:00 | [Android 目标设备后台刷新抖动修复](completed/2026-08-27/android-device-list-refresh-jitter.md) | 保留每秒设备发现并分离首次加载与后台刷新，消除已有选择的周期性禁用抖动 | PASS；前端回归、完整 CI 与 Windows 构建通过 | 低 | Android、ADB、后台刷新、轮询、UI |
| 15:27:02 +08:00 | [Proxy 上游多 CA PEM Bundle 支持](completed/2026-08-27/upstream-multi-ca-pem-bundle.md) | 一个上游信任 PEM 文件可严格承载多个 CA，并完整保持规范化、持久化、恢复和 Socket TLS Trust Store 成员 | PASS；解析/持久化/运行时/真实 First Data TLS 分层验收通过，未发送业务报文 | 高 | Socket、上游 TLS、多 CA、PEM Bundle、OpenSSL |
| 14:50:20 +08:00 | [日志倒序与 HTTP 抓包响应生成 Mock 规则草稿](completed/2026-08-27/log-order-and-http-capture-mock-draft.md) | 诊断日志稳定倒序；从完整服务器 HTTP 响应生成未保存、禁用且经过 Header/编码校验的 Mock 草稿 | PASS；Application、UI 与全量门禁通过；对抗审查 finding 已关闭 | 高 | 日志、Exchange、HTTP、MockResponse、规则草稿 |
| 14:49:30 +08:00 | [Android 多设备 VPN 并行运行与逐设备管理](completed/2026-08-27/android-multi-device-vpn-management.md) | 将单一 owner 升级为最多 8 台按 serial+epoch 的独立运行集合，覆盖 ADB Reverse/LAN/device-only、逐设备命令、断线重连、Environment 与 UI 隔离 | PASS；自动化与本地门禁通过；真机 A/B 场景因无设备保持 NOT_RUN | 高 | Android、VPN、多设备、serial、epoch、ADB、重连 |
| 12:17:47 +08:00 | [MCP 对话式完整环境配置](completed/2026-08-27/mcp-environment-configuration.md) | 新增完整环境候选、七层验证、公开预览、一次性确认、原子应用、全接口明文 MCP 传输与打包 App 重启恢复 | PASS；整体审查 APPROVE；独立复验 VERIFIED；打包 App 完整资源提交与重启恢复 PASS | 高 | MCP、环境配置、Workspace、TLS、原子应用、候选生命周期 |

## 2026-08-26

| 完成时间 | 任务 | 实现功能 | 验收结果 | 优先级 | 关键词 |
| --- | --- | --- | --- | --- | --- |
| 16:54:53 +08:00 | [交付 Nuvei Tango 只读 Rhai 协议包](completed/2026-08-26/nuvei-tango-rhai-read-only-package.md) | 为长度前缀 Tango JSON Socket 报文提供 Rhai 严格拆帧、六字段原文展示、只读逐字节 Encode、确定性 ZIP 和 Python parity 测试 | PASS；6/6 包级测试；Python/Rhai 同输入输出一致；确定性 ZIP PASS | 低 | Nuvei、Tango、Rhai、协议包、ZIP、Python parity、只读解析 |
| 15:42:02 +08:00 | [交付 Nuvei Tango 只读 Python 外部协议包](completed/2026-08-26/nuvei-tango-read-only-python-package.md) | 为长度前缀 Tango JSON Socket 报文提供严格拆帧、掩码展示、只读逐字节编码和安全诊断日志，并修复 external Document int wire 合同 | PASS；14/14 测试；真实双向 Exchange PASS | 低 | Nuvei、Tango、Python、外部协议包、只读解析、InvalidResponse |
| 12:38:41 +08:00 | [建立快速配置验证流程](completed/2026-08-26/rapid-configuration-validation-workflow.md) | 为证书、URL、Host、Port 和临时 Proxy 配置建立独立 QV 档案、八层结论、三段式恢复、清理门禁和正式任务升级合同 | PASS；整体对抗审查 APPROVE | 高 | 快速验证、证书、URL、TLS、QV、证据复用 |
| 10:39:05 +08:00 | [优化需求分析与测试验证流程](completed/2026-08-26/agent-workflow-governance-optimization.md) | 建立需求就绪、根因分析、高低优先级、风险分级测试和统一锁工具方向 | PASS；整体对抗审查 APPROVE | 高 | 需求就绪、根因分析、任务优先级、测试验证、任务锁 |
| 00:01:51 +08:00 | [使用单工作区全局锁串行化任务管理](completed/2026-08-26/task-management-global-lock.md) | 为任务登记、状态、归档和索引建立原子目录锁、显式所有权、fail-closed 恢复和多代恢复链 | PASS；code reviewer APPROVE；architect APPROVE/CLEAR | 未记录（历史任务） | 任务管理、并发、全局锁、恢复链、任务索引 |

## 2026-08-25

| 完成时间 | 任务 | 实现功能 | 验收结果 | 优先级 | 关键词 |
| --- | --- | --- | --- | --- | --- |
| 23:30:42 +08:00 | [完成架构优秀化优先任务](completed/2026-08-25/architecture-excellence-delivery.md) | 完成 Rust 规则合同、Listener CIDR 删除、SQLite executor/聚合快照、Infrastructure 收窄、确定性生命周期、可观测性和受信任外部包故障隔离 | PASS；code reviewer APPROVE；architect APPROVE/CLEAR | 未记录（历史任务） | 整洁架构、Rust 合同、Listener CIDR、SQLite、生命周期、故障隔离 |
| 17:07:48 +08:00 | [建立测试资源归档与跨任务复用规范](completed/2026-08-25/archive-reusable-test-resources.md) | 将证书、报文、配置、步骤和结果纳入任务归档，并建立派生需求复用关系 | PASS | 未记录（历史任务） | 测试资源、证书、报文、复测、归档、派生需求 |
| 16:52:35 +08:00 | [调整小任务对抗审查门禁](completed/2026-08-25/optional-subtask-adversarial-review.md) | 将小任务对抗审查改为风险触发的可选项，并保留整体任务最终强制审查 | PASS | 未记录（历史任务） | 对抗审查、小任务、风险门禁、AGENTS |
| 16:41:05 +08:00 | [生成项目执行治理规范](completed/2026-08-25/generate-project-agents-governance.md) | 固化任务登记、零假设、测试证据、对抗审查、文档同步和 CI 执行边界 | PASS | 未记录（历史任务） | AGENTS、任务治理、测试证据、对抗审查、CI |
