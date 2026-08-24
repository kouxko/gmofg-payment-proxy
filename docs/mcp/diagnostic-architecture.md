# MCP 诊断证据与代码结构

这份资源帮助 MCP 客户端在故障发生后回答三件事：现场怎样配置、运行时在哪个阶段失败、代码由哪一层负责。
不要只根据一条错误文案推断根因；先使用 `reproduction_report` 取得同一入口的聚合现场，再按关联 ID 深挖。

## 一、三类证据

- `application_log_query` / `application_log_get`：持久化 Rust/Tauri 运行日志。用于查看模块、线程、底层错误、启动和关闭过程。日志带稳定 ID、保留范围和淘汰计数，应用重启后仍可查询。
- `diagnostics_query`：应用已经分类的结构化阶段与结果。用于定位 Listener、Android、TLS、Socket、规则和 external_package 阶段。
- `exchange_observation_query` / `exchange_observation_get` 与 HTTP capture：线路证据。用于查看同一连接按顺序追加的实际收发、Document、Display、失败和关闭事件。

三者用途不同。运行日志不能证明线路写出了什么；抓包不能证明内部所有生命周期；结构化诊断也不等于完整底层日志。
运行日志默认覆盖应用 Debug 及以上、第三方 Info 及以上；第三方逐帧 Trace 因高频且会挤占固定容量而不持久化。

## 二、推荐排障顺序

1. 调用 `reproduction_report`，传入故障 Workspace 与 Listener。
2. 核对报告中的入口数据平面、监听地址、连接/转发方式、上游目标、TLS、精确协议包版本和规则绑定。
3. 沿报告时间线读取 `application_log_query`，用 Listener、connection、exchange、request 或 package ID 过滤。
4. 用 `application_log_get` 读取关键日志完整内容；再用 `diagnostics_query` 核对稳定错误码和阶段。
5. 用 `exchange_observation_get` 读取连接时间线中的实际收到、实际写出、Document 和 Display。比较“读取成功”“写出成功”“业务成功”，不要混为一谈。
6. 按报告中的复现步骤重放同一连接方式和测试数据。修复后再次生成报告，对比阶段、错误码、字节和运行状态。

## 三、代码所有权地图

- `src-tauri/crates/domain`：Workspace、Listener、Socket topology、协议 Document、external_package 注册与 wire 合同。
- `src-tauri/crates/application`：只读/写入用例、DTO、规则能力、复现报告组合、错误与诊断投影。MCP 和 UI 都应复用这里的权威业务模型。
- `src-tauri/crates/exchange`：协议中立的 `Exchange`、Reader/Writer `Pipeline`、`Envelope`、方向类型与透明 Socket 字节交换核心。
- `src-tauri/crates/infrastructure`：SQLite 配置/协议包元数据、ListenerRuntime、ADB、证书、external_package WebSocket/JSON-RPC，以及有界内存 `ExchangeObservationStore`。
- `src-tauri/crates/proxy`：具体 HTTP/Socket transport、能力装配、连接接入、读写和超时；不得依赖 UI 或 SQLite。
- `src-tauri/src/mcp`：loopback 只读 MCP transport、工具目录和资源；组合 Application 只读 facade、`RuntimeLogStore` 与 `ExchangeObservationStore` 两个窄只读句柄。
- `src/features`：Tauri/React 展示与用户操作；不能自行推导另一套业务状态。
- `examples/external-packages`：第三方 WebSocket 软件包示例及可独立运行的测试客户端。

## 四、Socket 与外部软件包调用链

`ListenerRuntime` 根据 Workspace 快照创建透明 Socket Exchange 或协议 Socket Exchange；`LocalServer` 是协议模式中的进程内精确 Echo Server。Scripted Socket 绑定精确包版本；内置包走 Rhai，
external_package 走 `/packages` WebSocket 与 JSON-RPC。Proxy 负责 TCP 分帧和业务连接生命周期，外部包只实现注册时声明的
`hooks.upstream.*`、`hooks.downstream.*` 与 `document.*.display` 方法。

同一故障至少要关联：Workspace ID、Listener ID、runtime epoch、业务 connection ID、可选 exchange ID、精确 package id/version、
方向、stage、JSON-RPC request ID、抓包 ID。缺少这些字段时，应在对应生产日志处补充结构化字段，而不是把上下文只写进中文描述。

## 五、复现报告内容

`reproduction_report` 返回结构化 bundle 和 Markdown，包含：构建/系统环境、设置、Workspace、精确 Listener、运行状态、连接与转发模式、
Android 网络状态与 endpoint、规则、协议包来源/能力/Schema、外部包连接和方法映射、最近运行日志、结构化诊断、代码路径和逐步复现说明。
它当前不聚合 Exchange observation 或完整抓包；这两类线路证据通过各自的只读查询独立取得。MCP 工具只返回内容，不写本机文件；桌面端“导出 Markdown”使用原生保存对话框。

报告与日志都有明确上限。复现 Markdown 会同步输出 `has_more`、保留日志 ID 范围、
`evicted_count`、损坏行、持久化错误、容量、文件字节上限，以及本页消息在存储时和
报告展示时的截断统计。任一字段表示证据不完整时，不能表述为“没有发生”。需要证明线路实际
输入或写出时，应继续查询 Exchange observation 或 HTTP capture，并自行核对 Workspace、Listener
与关联 ID；不能把复现报告中没有该字段解释成“没有发生”。
