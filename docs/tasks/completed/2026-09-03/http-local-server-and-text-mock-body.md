# TASK-20260903-002：支持 HTTP LocalServer 与文本 Mock Body

- 任务 ID：TASK-20260903-002
- 状态：已完成
- 任务日期：2026-09-03
- 创建时间：2026-09-03 13:51:49 +08:00
- 开始时间：2026-09-03 13:51:49 +08:00
- 最后更新时间：2026-09-03 15:04:03 +08:00
- 完成时间：2026-09-03 15:04:03 +08:00
- 创建路径：`docs/tasks/pending/2026-09-03/http-local-server-and-text-mock-body.md`
- 归档路径：`docs/tasks/completed/2026-09-03/http-local-server-and-text-mock-body.md`
- 关键词：`HTTP`、`LocalServer`、`MockResponse`、`BodyCodec`、`MCP`、`Rule`
- 任务优先级：高（HTTP 公共配置、规则合同、持久化兼容和异步连接生命周期）

## 背景、目标与派生关系

当前 HTTP Listener 只公开请求目标/固定真实 Server 路由，运行时始终装配
`BufferedHttpServer` 与 `UpstreamConnector`；Exchange crate 虽已有 `LocalHttpServer`，但生产 HTTP
配置和运行时未接入。与此同时 `MockResponse` 把 `body_bytes: Vec<u8>` 暴露到 Domain、MCP 和 UI，
使用户必须编辑字节数组。

- 原任务：`TASK-20260903-001`
- 原用例：`PAYMENT-DLL-D48-MOCK-001`
- 原证据：`docs/testing/evidence/2026-09-03/TASK-20260903-001/PAYMENT-DLL-D48-MOCK-001/`
- 复用资源：该用例的 D48 Mock 响应、Payment DLL Listener 配置和本机无上游命中验证。

目标：HTTP Listener 可显式选择真实 Server 或进程内 LocalServer；LocalServer 对上行 Context 原样
回环，下行仍经过完整 Pipeline；两个方向继续使用同一套 Rule。MockResponse 公共合同保存文本
`body`，仅在运行时按照响应 BodyCodec 编码为 wire bytes。

## 范围与不在范围

- Domain 增加显式 HTTP topology，区分 RemoteServer 与 LocalServer；RemoteServer 内继续区分请求目标与固定 Server。
- 持久化、Environment/MCP、Application、Runtime、UI 和生成类型同步该合同。
- LocalServer 不创建或调用真实 `UpstreamConnector`，保持每连接单请求/响应背压与现有取消清理语义。
- Proxy→Server 与 Proxy→App Rule 继续按既有优先级、终止动作和观察合同执行。
- `MockResponse.body_bytes` 改为文本 `body`；运行时使用响应 BodyCodec 编码，编码失败明确返回错误。
- 新建规则时，规则名称输入必须在选择 Listener、处理阶段、条件来源/字段和动作来源/类型后保持不变。
- HTTP 请求目标匹配字段显示为 `Path（包含 Query 参数）`，选中值必须在固定高度选择器内单行截断，不得换行溢出。
- `Message.body` 和实际网络收发仍使用 bytes；`InvalidJson` 等原始故障动作不在本次文本化范围。
- 不增加回退、默认成功或第二套规则引擎；不修改 Android App、证书或远端环境。
- 用户明确要求加快并跳过对抗审查；本任务不执行额外对抗审查。

## 需求确认记录与就绪检查

- 2026-09-03：用户确认 HTTP 应与 Socket 一样可选择本地 Mock 或真实 Server。
- 2026-09-03：用户确认 `HttpLocalServer` 为收到什么回复什么，Proxy→Server 与 Proxy→App 均继续经过 Rule。
- 2026-09-03：用户确认 Mock Body 不应使用 bytes 公共表示。
- 2026-09-03：用户补充确认新建规则名称在点击后续选项后不得被清空，要求并入本次修改。
- 2026-09-03：用户补充确认请求目标字段改名为 `Path（包含 Query 参数）`，并要求修复选中长文本换行溢出。
- 输入：HTTP 请求与 Listener topology；输出：真实上游响应或 LocalServer 回环后经下行规则生成的响应。
- 状态变化：Listener 保存 topology；切换 LocalServer 后不再拥有上游连接配置的运行时职责。
- 具体示例：Payment DLL `POST /` 由 MockResponse 返回既有 D48 JSON 文本，不连接真实上游。
- 验收：配置可往返；LocalServer 请求不调用真实 connector；上下行规则均命中；D48 Body 文本与 wire bytes 一致；旧配置可兼容读取。
- 未确认事项：零。需求就绪，2026-09-03 13:51:49 +08:00 进入实现。

## 问题与根因分析

- 实际现象：HTTP UI 无 LocalServer 选择；MockResponse 编辑器和 MCP 暴露 `body_bytes: number[]`；新建规则名称输入后选择后续选项会被清空。
- 预期依据：用户明确要求与 Socket 一致的本地/真实 Server 选择，并要求 Mock Body 使用文本。
- 最小复现：读取默认 HTTP Listener 类型可见只有 `fixed_server`；选择 MockResponse 后动作参数要求 `body_bytes`。
- 当前已验证：`LocalHttpServer` 已存在于 Exchange；HTTP 启动路径固定创建 `BufferedHttpServer`；
  `TerminalAction::MockResponse` 保存 `Vec<u8>`，生成 TypeScript 后成为 `number[]`；规则创建器的异步结构选择回写使用了闭包中的旧 creation 值；
  HTTP metadata 从 URI 的 `path_and_query()` 生成 `request_target`，因此匹配值包含 Path 与 Query；字段选择器固定高度但未限制选中值换行和收缩。
- 推断：无。
- 未知：无。
- 已确认根因：Endpoint 能力未上提为 HTTP topology；wire bytes 被错误提升为规则公共配置类型；规则创建器把元数据拆成独立 state，却在异步结构更新时用旧快照重建完整输入，覆盖了最新名称；请求目标字段文案过长且选择器值未设置单行截断。
- 影响范围：Domain、Application/MCP、Infrastructure runtime、Proxy HTTP endpoint、UI、生成类型和兼容测试。

## 方案比较与小任务

- 最小改动：在现有 `fixed_server` 外增加布尔开关并在 UI 隐藏字段。拒绝；会产生 LocalServer 与固定上游并存的非法组合。
- 最优设计：使用带标签的 HTTP topology 值对象，使 Remote/Local 互斥；在运行装配边界选择 Endpoint；
  Mock 文本在已有响应 BodyCodec 边界一次性编码。采用。

| ID | 内容 | 依赖 | 可并行 | 状态 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T01 | Domain topology、Mock 文本合同与兼容读取 | 无 | 否 | 已完成 | 类型与校验测试通过 |
| T02 | Application/Environment/MCP 映射 | T01 | 否 | 已完成 | schema 与候选往返通过 |
| T03 | Runtime LocalServer 装配与双向 Rule | T01 | 否 | 已完成 | 零真实 connector 调用、双向规则通过 |
| T04 | UI 选择器、文本 Mock 编辑体验、新建规则名称状态保持与 Path 字段防溢出 | T02 | 可与 T03 并行但本次单 Agent 执行 | 已完成 | 拓扑切换、名称保持、Path 文案和单行截断组件测试通过 |
| T05 | 文档、证据、回归与提交 | T02-T04 | 否 | 已完成 | 针对性测试、静态检查与证据归档通过 |

## 测试、文档与审查计划

- Domain：旧 HTTP JSON 读取、Remote/Local 互斥、Mock Body 文本序列化和阶段兼容。
- Runtime：LocalServer 回环、请求/响应规则、Mock 替换、BodyCodec 编码失败、取消与连接清理。
- Application/MCP：完整候选 schema、投影、预览、应用往返。
- UI：拓扑切换、上游字段显示、Mock 文本输入和保存回读；输入规则名称后依次切换后续选项，名称保持并进入保存参数；Path 字段显示包含 Query 的语义且选中值单行截断。
- 集成：复用 D48 文本向量，断言 wire body 与预期字节一致且真实 connector 调用数为 0。
- 文档：更新架构、需求、用户操作和 MCP 合同。
- 对抗审查：按用户明确要求跳过；保留自动化回归和静态检查。

## 实施记录、修改文件、验收与完成总结

- 2026-09-03：新增带标签的 `HttpTopology`，生产配置只表达 RemoteServer 或 LocalServer；旧顶层
  `fixed_server` 只在读取边界迁移，新旧字段混用 fail-closed。
- 2026-09-03：新增 `LocalHttpServerService` 与 `LocalHttpServerConnector`，复用现有 ConnectionService、
  Exchange、Pipeline、TLS acceptor、取消和关闭所有权；Local 模式不装配真实上游 connector。
- 2026-09-03：`MockResponse` 公共 body 改为 UTF-8 文本，旧 UTF-8 bytes 只在反序列化迁移边界读取；
  wire 写出统一按响应 BodyCodec 编码，Proxy → Server 与 Proxy → App 都可使用 Mock。
- 2026-09-03：Environment/MCP schema、候选 fixture、Application 映射、生成 TypeScript 与 UI 同步；
  Listener UI 可切换 Local HTTP Server，Mock JSON 使用文本 body。
- 2026-09-03：删除 Listener 选择时清空规则名称的状态写入；请求目标显示为
  `Path（包含 Query 参数）`，Select value 使用可收缩单行省略布局，修复固定高度溢出。
- 文档：更新用户操作、Exchange/Pipeline、数据流、规则和安全持久化说明。
- 测试证据：[HTTP-LOCAL-MOCK-001](../../../testing/evidence/2026-09-03/TASK-20260903-002/HTTP-LOCAL-MOCK-001/README.md)。
- 验收结果：PASS。Local HTTP 集成响应 HTTP 200 + D48，两个规则阶段各执行 1 次；Domain、Runtime、
  Infrastructure、MCP schema、43 项相关 UI、生成绑定、typecheck、lint、fmt、Clippy 和架构文档门禁通过。
- CI：NOT_RUN；用户只要求本地测试和提交，未推送、未触发 CI、未发布。
- 对抗审查：SKIPPED_BY_USER；用户明确要求不要执行对抗审查。
- 完成总结：HTTP 现在与 Socket 一样显式选择本地或真实 Server，同时保持单一 Exchange/Pipeline；
  Mock Body 对用户和 MCP 为文本，运行时才编码为 bytes；规则名称与 Path 选择器 UI 回归已关闭。
