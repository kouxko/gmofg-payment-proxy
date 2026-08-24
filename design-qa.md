# 2026-08-24 规则页统一 HTTP/Socket 列表验收

- 问题参考图：`/var/folders/q8/ztpkwyr54nxgslwrnd8lsjdm0000gn/T/codex-clipboard-WEVW7p.png`
- 验收对象：规则页在一个列表和一个右侧编辑区内统一展示 HTTP 常规、HTTP Body 与 Socket 报文规则。

## 已完成验证

- 组件契约确认页面只有一个“规则”标题、一张规则表和一个“新建规则”入口。
- HTTP 常规、HTTP Body、Socket 报文规则使用不同协议标签显示在同一张表内。
- 选择 HTTP Body 或 Socket 行时，在同一个右侧区域切换到对应协议编辑器。
- 新建规则弹窗统一提供空白 HTTP、HTTP Body、Socket 报文规则和 HTTP 故障预设。
- 规则定向测试 48 项、UI 契约 440 项、完整前端测试 639 项通过；TypeScript、ESLint、前端边界和源码行数门禁通过。
- macOS release `.app` 已成功构建。

## 原生应用视觉验收

- HTTP 与 Socket 规则已显示在同一张规则表中，没有协议切换 Tab，也没有上下分区。
- 选择 Socket 规则后，右侧同一编辑区正确切换为 Socket 条件和动作编辑器。
- 页面只有一个“新建规则”按钮；弹窗统一提供空白 HTTP、HTTP Body、Socket 报文规则与故障预设。
- HTTP 规则截图：`/tmp/intercept-proxy-unified-rules-20260824.png`
- Socket 编辑器截图：`/tmp/intercept-proxy-unified-rules-socket-20260824.png`
- 统一新建入口截图：`/tmp/intercept-proxy-unified-rules-create-20260824.png`

**final result: passed**

---

# Workspace 页面视觉验收

- 参考截图：`/var/folders/q8/ztpkwyr54nxgslwrnd8lsjdm0000gn/T/codex-clipboard-97b5b831-e193-43b8-9f6e-6c139ed2b136.png`
- 参考尺寸：3024 × 1898
- 验收状态：Workspace 页面，默认 Workspace 已选中

## 本次调整

- 删除顶部占据大面积高度的“新建 Workspace”独立卡片。
- 将页面标题、名称输入框、新建按钮和导入按钮合并为紧凑的横向工具栏。
- Workspace 列表紧接工具栏显示，减少首屏无效留白。
- 窄窗口下工具栏自动换行，继续使用现有 HeroUI `Input` 和 `Button`。

## 自动化验证

- Workspace 交互测试通过。
- ESLint 通过。
- TypeScript 类型检查通过。
- Tauri macOS release 构建通过。

## 视觉验证限制

已重新启动打包后的 macOS 应用，但应用窗口位于另一个 macOS Space；系统窗口截图返回全黑图像，因此无法生成可信的同尺寸前后对比图。自动化测试和构建结果不能替代视觉验收，最终布局由用户在当前已打开的应用中确认。

**final result: blocked**

---

# 2026-08-03 代理监听 TLS 与证书详情验收

- 问题参考图：`/var/folders/q8/ztpkwyr54nxgslwrnd8lsjdm0000gn/T/codex-clipboard-c4e0afba-3991-4b05-a3c0-68ab0bd72411.png`
- 验收对象：入口配置中的客户端侧 TLS、上游 Server TLS/mTLS 与证书详情。

## 本次调整

- 将原先并排且含义相近的证书选择器拆为“客户端 → 本机代理”和“本机代理 → 固定 Server”两张连续卡片，明确两次独立 TLS 握手。
- 下游区按“启用 TLS → 代理服务端身份 → 可选客户端证书验证”排列；普通 HTTPS 默认不要求客户端证书。
- 上游区按“主机名校验 → Server CA → 可选 mTLS 客户端身份 → 真实握手测试”排列。
- 每个已选证书后直接显示 Rust 解析的用途、主题、SAN、有效期、SHA-256 指纹和有效状态；单份证书损坏只影响对应详情，不阻塞其他证书。
- 导入 CA 或 P12 后立即返回已解析公开元数据；私钥、密码、原始证书和文件路径不进入前端或 Workspace。

## 自动化验证

- Listener 前端测试：15 项通过，覆盖导入后立即展示详情、持久化引用详情和密码不进入 Workspace。
- Infrastructure 证书测试：3 项通过，覆盖受保护导入、文件引用解析和不回传文件路径。
- Application 测试：63 项通过。
- TypeScript 类型检查、相关 ESLint、Rust Clippy 与 `git diff --check` 均通过。

## 视觉验证限制

运行中的 macOS 应用在验收时处于锁屏状态，Computer Use 无法取得可信的最新应用截图，因此未伪造同尺寸视觉对比结论。代码继续使用现有 HeroUI、卡片边框和间距体系；最终视觉状态需要在解锁后的应用窗口中确认。

**final result: blocked**

---

# 2026-08-01 应用定向弱网双栏布局视觉验收

- 问题参考图：`/var/folders/q8/ztpkwyr54nxgslwrnd8lsjdm0000gn/T/codex-clipboard-32213f0a-e2fa-4012-922c-e54bff3001f5.png`
- 最终实现图：`/tmp/intercept-proxy-device-network-final.png`
- 逻辑视口：macOS 打包应用窗口 1440 × 900
- 图片尺寸：参考图 2880 × 1800（Retina）；自动化实现截图 1229 × 768（工具等比优化输出）
- 验收状态：已选择安卓模拟设备、已新建弱网方案、已输入包名 `android` 并由 Rust 返回匹配结果

## 可见问题与修复

- **P1 信息架构混杂**：原图把 ADB、设备、设备端控制和弱网参数横向铺开，用户难以判断操作顺序。已重排为左侧“连接与设备控制”、右侧“弱网方案与配置”的稳定双栏结构。
- **P1 无法快速定位应用**：原图应用列表没有查询入口。已在目标应用区域加入 HeroUI 输入框与按钮，包名匹配、输入上限和结果返回均由 Rust 完成。
- **P1 概念重复**：原导航中的代理入口与控制台不易区分。已改为“入口配置”和“运行监控”，入口页补充“入口只负责接收与转发流量”的说明及“去添加故障模拟 / 去配置拦截规则”直接入口。
- **P2 加载状态误导**：首次筛选时短暂显示“没有匹配”。已改为“正在由 Rust 筛选包名…”，并在 Rust 应用层缓存当前设备完整包清单，切换或重新选择设备时失效。
- **P2 非中文术语**：设备弱网页面的 `Companion`、`Profile` 等操作文案已改成设备端组件、弱网方案等中文表达。

## 对照结论

- 全视图：导航、左右分栏、卡片边距和滚动区域没有贴边、裁切或大面积无意义留白。
- 聚焦区域：右栏能看到方案基本信息、包名筛选、应用结果和后续 TCP/IP 弱网参数；左栏保持设备选择及网络接管控制，不与配置字段混排。
- 核心路径：选择设备 → 新建弱网方案 → 按包名筛选 → 选择应用 → 配置弱网参数，控件均在打包应用中可达。
- 视觉对比未发现新的 P0、P1 或 P2 阻断问题。

**final result: passed**

---

# 2026-08-22 抓包页统一 HTTP/Socket 运行记录验收

- 问题参考图：`/var/folders/q8/ztpkwyr54nxgslwrnd8lsjdm0000gn/T/codex-clipboard-JM4QO5.png`
- 最终实现图：`/var/folders/q8/ztpkwyr54nxgslwrnd8lsjdm0000gn/T/com.openai.sky.CUAService/Intercept Proxy Screenshot 2026-08-22 at 22.33.45.jpeg`
- 并排对比图：`/tmp/intercept-proxy-unified-comparison-settled.jpg`
- 验收对象：同一个运行记录区域内统一显示 HTTP 与 Socket Exchange。

## 本次调整

- 删除旧 HTTP Session 列表，抓包页只挂载 `ExchangeObservationView`。
- HTTP 与 Socket 共用“运行记录”标题、清空操作、分页、空状态和一张表格。
- 表格按协议列区分 `HTTP` 与 `SOCKET`，两类记录继续共用连接级 Exchange 时间线与详情页。

## 功能证据

- UI 定向回归 12/12 通过；完整前端测试 66 个文件、645 项通过。
- Python 真实验收通过：HTTP 请求/响应规则命中，Socket ISO8583 分片组帧和四阶段规则命中。
- Accessibility 控件树只返回一个“统一运行记录工作区”和一张“HTTP 与 Socket 运行记录”表格。
- App 内存观测立即在同一表格返回 HTTP 与 Socket 两条 Exchange；每条均按 `opened → received → sent → received → sent → closed` 排列，未发生覆盖。
- HTTP 详情保留 Header、Body、Display 与规则修改结果；Socket 详情保留字节证据、ISO8583 Display 与上下行规则修改结果。
- macOS release `.app` 构建成功并启动；两个 E2E 监听器保持运行。

## 视觉验收

- 已将问题参考图与真实打包应用的稳定状态截图合并比较。
- 原上下两个独立区域已收敛成一个共享表格，标题、清空操作、分页和空白空间均只保留一套。
- 未发现新的溢出、裁切、错位、异常间距或重复控件问题。

**final result: passed**

---

# 2026-08-03 设备网络命名与透明代理路由验收

- 问题参考图：`/var/folders/q8/ztpkwyr54nxgslwrnd8lsjdm0000gn/T/codex-clipboard-4276ce8f-c475-41c3-b0da-2a0ee012098c.png`
- 验收对象：Android 设备网络页面的功能命名、透明代理路由端口说明和目标应用选中反馈。

## 本次调整

- 导航统一为“设备网络”，页面统一为“应用网络接管”，持久化配置统一为“设备网络方案”，避免把 VPN 的透明代理路由能力误称为单一弱网功能。
- 透明代理路由与“弱网覆盖范围”拆分展示：前者决定连接去向，后者只决定哪些连接实施弱网。
- 透明代理路由必须明确配置至少一个原始端口；页面不再提示端口留空可以匹配全部端口。
- 目标应用仍通过点击整行切换；选中行增加固定占位的勾选图标，不因选中状态改变列布局。

## 自动化与运行态验证

- 前端完整测试 23 个文件、99 项通过；覆盖明确端口、多路由、包名筛选、整行选择和选中图标。
- Rust Domain 49 项、规则 14 项、Android 应用层 8 项、Android 路由 3 项、证书相关 22 项通过。
- TypeScript、ESLint、前端 Rust-only 边界扫描、Rust fmt 与 macOS debug `.app` 构建通过。
- 最新打包应用已重新启动，进程持续运行且未发现 macOS error/fault 日志。

## 视觉验收边界

按既有约定，本轮只打开最新打包应用，不由 Codex 在应用中切换页面或修改配置。最终页面视觉由用户在已打开的应用中确认。

**final result: blocked**

---

# 2026-08-01 应用定向弱网单列布局与包名列表验收

- 问题参考图：`/var/folders/q8/ztpkwyr54nxgslwrnd8lsjdm0000gn/T/codex-clipboard-32213f0a-e2fa-4012-922c-e54bff3001f5.png`
- 最终实现图：`/var/folders/q8/ztpkwyr54nxgslwrnd8lsjdm0000gn/T/com.openai.sky.CUAService/Intercept Proxy Screenshot 2026-08-01 at 21.16.45.jpeg`
- 验收对象：macOS release 打包应用中的“应用定向弱网”页面

## 本次调整

- 取消左右双栏，将本机连接、目标设备、设备端控制、弱网方案及所有配置卡片改为全宽单列逐行排列。
- 卡片边框恢复为项目其他页面统一使用的 `telemetry-line`，并保留相同的 `shadow-sm`，不再使用单页特有的深色边框。
- 目标应用表格设置固定可视高度，并为 HeroUI `Table.ScrollContainer` 显式启用纵向滚动、滚动边界和稳定滚动条槽位。
- 删除每行重复的“选择”按钮，改为点击整行切换应用；选中行使用统一浅色背景和“已选中”状态标记。
- 删除表格内重复的“点击整行选择”状态列，将操作说明统一放在“目标应用”卡片标题下；表格仅保留包名、UID 和签名，选中状态由整行高亮表达。
- 将“本机连接工具”“目标设备”“设备端控制”三个独立卡片合并为一个“设备连接与控制”卡片；目标设备改用 HeroUI 下拉选择，ADB 状态、刷新设备和设备端操作按使用顺序集中展示。

## 功能与视觉证据

- 包名列表最初显示 `android`、`android.ext.services` 等靠前条目；滚动后显示 `com.genymotion.genyd`、`com.qwant.liberty`、`org.chromium.webview_shell` 等后续条目，证明列表可以独立查看后续包名。
- 最终截图一次可见 6 行应用，不再只露出一行或少量内容。
- 全宽单列卡片没有左右内容竞争，未发现重叠、横向溢出、贴边或异常留白。
- 卡片圆角、浅灰边框和轻阴影与项目现有页面保持一致。

## 自动化验证

- Android 弱网页面测试：8 项通过，包含 20 条应用清单、纵向滚动容器、单列布局、整行选择和设备下拉选择回归断言。
- TypeScript 类型检查通过。
- 相关 ESLint 检查通过。
- Tauri macOS release 重新构建通过，并生成 `.app` 与 `.dmg`。

## 本轮人工验收边界

- 最新问题参考图：`/var/folders/q8/ztpkwyr54nxgslwrnd8lsjdm0000gn/T/codex-clipboard-19490ef4-0f2c-488d-90d0-89dbfd3bc11d.png`。
- 最新实现已合并三张设备卡片、改为 HeroUI 设备下拉框，并重新生成 macOS `.app` 与 `.dmg`。
- 按用户要求，本轮只打开最新打包应用，不由 Codex 点击、切换页面或继续截图判断；最终视觉验收由用户完成，因此没有伪造最新实现截图或等尺寸对照结论。

**final result: blocked**

---

# 2026-08-03 代理入口正文编码与 TLS 客观文案验收

- 问题参考图：`/var/folders/q8/ztpkwyr54nxgslwrnd8lsjdm0000gn/T/codex-clipboard-d3c1227d-95c3-441d-a948-7e851bf9827a.png`
- 最终实现图：`/var/folders/q8/ztpkwyr54nxgslwrnd8lsjdm0000gn/T/com.openai.sky.CUAService/Intercept Proxy Screenshot 2026-08-03 at 15.29.25.jpeg`
- 对比图：`/tmp/intercept-codec-comparison.png`

## 本次调整

- 删除 Workspace 级 Body Codec 策略、方向和引用 ID；请求/响应正文编码直接属于每个代理入口。
- 代理入口可分别选择原始字节、UTF-8 或 Shift-JIS；动态目标和固定 Server 使用相同模型。
- 删除 Workspace 页面中的 Body Codec 编辑入口和代理入口中的“管理编码策略”按钮。
- TLS 说明统一改为协议职责描述：普通 TLS 不请求客户端证书；可选/必需模式验证客户端证书链；上游 Server 证书链验证与代理 mTLS 客户端身份分开配置。

## 视觉与功能证据

- 新界面仅显示“HTTP 正文编码”及请求/响应两个直接选择器，不再显示 Workspace 策略 ID 或管理入口。
- 代理入口页继续使用现有 HeroUI Card、Select、Switch 和项目统一边框、间距；未发现新增贴边、溢出或异常留白。
- Rust Workspace 测试 11 项通过；基础设施正文编码测试通过；前端相关回归 34 项通过。
- TypeScript、ESLint、Rust workspace check、模拟器门禁程序 check、`git diff --check` 和 macOS debug 应用构建通过。

**final result: passed**
