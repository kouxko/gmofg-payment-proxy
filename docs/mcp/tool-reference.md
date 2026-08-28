# MCP 工具参考

本文档是 Intercept Proxy 内嵌 MCP 服务的完整工具目录。服务使用 MCP `2026-07-28` 和无会话
Streamable HTTP，在端口 `17653` 监听全接口 IPv4，并在平台支持时监听全接口 IPv6。当前目录包含
36 个只读工具和五个环境配置工具。

服务使用明文 HTTP，且不验证调用方身份或权限。任意语法有效的 `Host`、缺失或任意语法有效的
`Origin`，以及缺失或任意 `Authorization`、API key、Cookie 都会直接进入 MCP 协议处理。不存在
Host/Origin allowlist、来源 IP/CIDR 门禁或凭据认证。任何能够连接端口的主机都能读取公开数据、
创建候选并尝试应用配置；网络观察者也可能读取客户端提交的私钥、密码和确认令牌。这是明确接受的
产品风险，不是安全远程管理边界。

Transport 启动状态如下：

| 绑定结果 | capability / warning | MCP 服务状态 |
| --- | --- | --- |
| IPv4 绑定失败 | `ipv4.available=false`，`IPV4_BIND_FAILED` | 启动失败；不提供仅 IPv6 的部分服务 |
| IPv4、IPv6 独立绑定成功 | IPv4/IPv6 均 available，无 warning | 正常运行 |
| IPv4 socket 同时覆盖 IPv6 | IPv6 available，`ipv6_dual_stack_covered` | 正常运行 |
| IPv4 成功、平台不支持 IPv6 | IPv6 unavailable，`ipv6_unsupported` | 仅 IPv4 运行 |
| IPv4 成功、其他 IPv6 绑定错误 | IPv6 unavailable，`IPV6_DEGRADED` | 仅 IPv4 降级运行，不声称 IPv6 可达 |

## 通用调用契约

- 参数必须是 JSON object。每个工具发布的 input schema 都声明 `additionalProperties: false`；该约束递归应用到 `page`、`package` 等嵌套对象，任意层级的未知字段都会返回 `INVALID_ARGUMENTS`，不会被静默忽略。
- 工具目录同时发布 output schema。成功结果通过 MCP `structuredContent` 返回；运行时会校验 object、array、object/null 根类型与公开 Schema 一致，下表的“成功结果”描述其根类型和语义投影。
- 错误结果同样是结构化对象，至少包含 `code`、`message`，应用错误还可包含 `details`。常见代码包括 `INVALID_ARGUMENTS`、`NOT_FOUND`、`INPUT_BUDGET_EXCEEDED`、`OUTPUT_BUDGET_EXCEEDED`、`OUTPUT_SCHEMA_MISMATCH` 和 `TOOL_DEADLINE_EXCEEDED`。`OUTPUT_SCHEMA_MISMATCH` 表示后端成功值违反公开输出合同，应视为服务端缺陷，而不是客户端重试条件。
- 36 个只读工具和 `mcp_environment_capabilities` 的逻辑 JSON 输入上限为 256 KiB、输出上限
  为 8 MiB、执行期限为 8 秒。环境配置写工具使用下节列出的独立预算。HTTP request body 的 transport
  上限为 2 MiB，不能替代更小的逐工具逻辑预算。
- 分页工具只返回当前保留窗口中的数据；日志、诊断、HTTP 抓包和 Exchange 观察都可能因有界保留策略而淘汰旧记录。调用方应保存稳定 ID、游标和 `runtime_epoch`，并显式处理记录已不在保留范围的情况。
- 所有 UUID 参数均以字符串传入。标为“无”的参数应传 `{}` 或省略 `arguments`。

## 环境配置工具

| 工具 | 参数 | 成功结果 | 注解（read-only / destructive / idempotent） | 逻辑预算 |
| --- | --- | --- | --- | --- |
| `mcp_environment_capabilities` | 无 | object：协议、明文/认证策略、IPv4/IPv6、warning、预算、容量、保留和验证层 | `true / false / true` | 输入 256 KiB；输出 8 MiB；8 秒 |
| `environment_candidate_create` | 必填 `candidate` | object：候选 ID、目标、状态、逐层验证、公开 baseline、预览和仅在可应用时返回的 confirmation token | `false / false / false` | 输入 1 MiB；输出 1 MiB；30 秒总验证期限 |
| `environment_candidate_status` | 必填 `candidate_id` | object：进程内候选状态、公开 baseline/预览、验证层和可选终态 | `true / false / true` | 输入 16 KiB；输出 1 MiB；8 秒 |
| `environment_candidate_cancel` | 必填 `candidate_id` | object：`cancelled`、`apply_in_progress_not_cancellable` 或 `not_found_or_terminal` | `false / true / true` | 输入 16 KiB；输出 1 MiB；8 秒 ack |
| `environment_candidate_apply` | 必填 `candidate_id`、`confirmation_token` | object：`apply_task_id` 与 `apply_queued` ack，或结构化失败 | `false / true / false` | 输入 16 KiB；输出 1 MiB；8 秒 ack |

create 依次执行 schema、domain、material、package projection、DNS/TCP/port、TLS/mTLS 和
preview/baseline 验证。各层预算依次为 1 秒、1 秒、6 秒、4 秒、8 秒、10 秒和 2 秒，网络探针并发为
4；30 秒总期限优先于各层期限。create 返回前，候选仍归当前请求所有：请求取消或连接断开会取消
创建并清理未提交私有材料，不留下可查询候选。

apply 原子消费一次性 confirmation token，并只负责在 8 秒内把任务移交给 Application、返回
`apply_queued`。只有 owned worker 出队并取得清理所有权后，状态才进入 `apply_in_progress`；ack 后
调用方断开不会取消任务。cancel 可在线性化竞争中取消 `validating`、`preview_ready` 或尚未由 worker
取得所有权的 `apply_queued`；进入 `apply_in_progress` 后不会中断准备或提交。调用方应持续查询 status，
直到 `committed`、`rolled_back`、`failed_before_commit` 或其他终态。

候选状态与公开终态只在当前 App 进程内有界保留：最多 32 个终态候选且最多 4 MiB 公开序列化数据，
按 oldest-first 淘汰。活动候选最多四个、每个目标最多一个；全局和每个目标都最多一个活动 apply。
MCP 不会自动停止、启动或重启 Listener，也不会中断活动连接；受影响运行态不安全时 apply 拒绝。

环境工具接受客户端直接提交的私有材料，但不会在预览、status、terminal result、错误、日志或诊断中
返回私钥、密码、confirmation token、保护后字节、原始请求体或本机秘密路径。成功提交后只返回安全
引用和公开证书元数据；这项输出限制不改变明文 HTTP 输入在网络上可被观察的风险。

## 通用、诊断与工作区

| 工具 | 参数 | 成功结果 | 说明 |
| --- | --- | --- | --- |
| `application_snapshot` | 无 | object：Application 在 mutation gate 内单次编排的设置、工作区、运行态、协议包、规则和诊断快照 | `consistency.generation` 是用于比较返回内容的观察指纹，不是数据库 revision 或强冲突令牌；Workspace 摘要与详情来自一次聚合读取。 |
| `application_log_query` | 可选 `level`、`target`、`keyword`、`occurred_from`、`occurred_to`、`before_log_id`、`limit` | object：稳定游标分页的 Rust/Tauri 持久化日志与保留元数据 | `limit` 默认 200，最大 500；返回 `evicted_count` 和容量/字节满、consumer 断开、producer 竞争三个 `queue_dropped_*` 计数。 |
| `application_log_get` | 必填 `log_id` | object：一条保留的运行日志 | 已淘汰的 ID 返回 `NOT_FOUND`。 |
| `exchange_observation_query` | 必填 `workspace_id`、`page`；可选 `listener_id` | object：连接级 Exchange 页及保留信息 | `page` 含必填的 `page`、`page_size`，页大小最大 200；分别返回 producer `dropped_events` 与 consumer/store `ignored_events`。 |
| `exchange_observation_get` | 必填 `exchange_id` | object：一条完整的连接级 Exchange 观察 | 已淘汰的 ID 返回 `NOT_FOUND`。 |
| `reproduction_report` | 必填 `workspace_id`、`listener_id` | object：`bundle`、有界 `application_logs` 和可复制 `markdown` | 只汇总配置、监听器运行态、结构化诊断、日志与协议包现场；不读取 `ExchangeObservationStore`，也不包含 HTTP 抓包。Exchange 和 HTTP 证据必须分别调用对应查询工具。 |
| `settings_get` | 无 | object：已保存的全局设置 | 读取持久化投影。 |
| `workspace_list` | 无 | array：全部工作区摘要 | 不包含完整子项。 |
| `workspace_get` | 必填 `workspace_id` | object：完整工作区及 entry、Android profile、证书引用和协议规则绑定 | 不返回私钥材料。 |
| `entry_overview` | 必填 `workspace_id` | object：工作区配置 entry 与当前运行态的合并视图 | 用于判断配置与活动监听器是否一致。 |
| `entry_status_list` | 无 | array：全部 entry 的当前运行状态 | 运行时投影。 |
| `diagnostics_query` | 可选 `keyword`、`after_event_id`、`limit` | object：最新优先的结构化诊断与保留信息 | `limit` 默认 300，最大 500；返回 `oldest_retained_event_id`，游标早于 EventHub 保留窗口时 `snapshot_required=true`。 |
| `diagnose_recent_failures` | 同 `diagnostics_query` | object：证据、确定性排障建议和验证步骤 | 只生成建议，不执行修复。 |

## 运行态、Android 与证书

| 工具 | 参数 | 成功结果 | 说明 |
| --- | --- | --- | --- |
| `external_package_service_status` | 无 | object：外部协议包 WebSocket 地址、固定路径、绑定状态、认证边界和在线数 | 是服务可达性的权威运行态。 |
| `android_adb_get` | 无 | object：当前 ADB 与选中设备状态 | 不会连接或切换设备。 |
| `android_device_list` | 无 | array：已连接 Android 设备 | 只读实时查询。 |
| `android_package_list` | 必填 `serial` | array：指定设备的缓存包清单 | 不依赖当前选中设备。 |
| `android_package_get` | 必填 `serial`、`package_name` | object：指定设备的一个 Android 包详情 | 未找到时返回应用错误。 |
| `android_profile_list` | 无 | array：当前选中工作区的 Android 网络 profile | 依赖应用当前选择。 |
| `android_profile_get` | 必填 `profile_id` | object：一个 Android 网络 profile | 未找到时返回应用错误。 |
| `android_network_status` | 必填 `serial` | object：指定设备的 Android 网络状态 | 不触发网络变更。 |
| `android_runtime_owner_list` | 无 | array：全部持久化 Android 运行 owner，按设备序列号稳定排序 | 空数组表示没有 owner。 |
| `android_network_endpoints` | 必填 `serial`，可选 `profile_id` | object：指定设备的配置端点与活动运行端点 | 不修改设备，不依赖当前选中设备。 |
| `certificate_overview` | 无 | object：Root/leaf 公共证书元数据与就绪状态 | 永不暴露私钥。 |
| `workspace_certificate_overview` | 必填 `workspace_id` | array：工作区托管证书引用的公共元数据 | 永不暴露私钥。 |

## HTTP 抓包与断点

| 工具 | 参数 | 成功结果 | 说明 |
| --- | --- | --- | --- |
| `http_capture_query` | 可选 `page`、`page_size`、`keyword`、`terminal_ip`、`channel`、`stage`、`result`、`rule_id`、`after_event_id`、`sort`、`direction` | object：有界 HTTP 抓包页与分页/保留信息 | `page_size` 最大 200；默认按工具 schema。 |
| `http_capture_get` | 必填 `session_id`、`runtime_epoch` | object：精确运行 epoch 中的一条完整 HTTP 抓包 | epoch 不匹配可防止读取重启前的陈旧记录。 |
| `breakpoint_query` | 可选 `runtime_epoch` | array：当前待处理 HTTP 断点 | 不会继续或解决断点。 |
| `breakpoint_get` | 必填 `breakpoint_id`、`runtime_epoch` | object：一个待处理 HTTP 断点详情 | 不会修改断点。 |

## 规则与协议包

| 工具 | 参数 | 成功结果 | 说明 |
| --- | --- | --- | --- |
| `rule_list` | 无 | array：当前选中工作区的统一 HTTP/Socket 规则摘要 | 依赖应用当前选择，不执行规则。 |
| `rule_get` | 必填 `rule_id` | object：一条完整统一规则定义 | 内容以 `http` 或 `socket` 标签区分。 |
| `workspace_rule_list` | 必填 `workspace_id` | array：指定已保存工作区的统一规则定义 | 不依赖当前选择。 |
| `protocol_package_list` | 无 | array：所有已安装不可变协议包版本及使用数 | 安装源文件不在 Application facade 中暴露。 |
| `protocol_package_catalog` | 无 | object：已启用且已完成能力描述的协议包、方向能力、Schema 和目录校验信息 | 在线不等于能力描述成功。 |
| `protocol_package_detail` | 必填 `package.id`、`package.version` | object：精确版本的 manifest 投影、方向能力、Schema 与 entry 使用情况 | 不返回安装源文件。 |
| `protocol_package_usage` | 必填 `package.id`、`package.version` | array：引用该精确版本的工作区/entry | 版本是不可变身份的一部分。 |

## 证据组合建议

一次可复现排障通常先调用 `reproduction_report` 固定工作区和监听器上下文，再按故障类型补充 `exchange_observation_query` / `exchange_observation_get`、`http_capture_query` / `http_capture_get` 或 `application_log_query` / `application_log_get`。不要把“工具调用成功”误认为业务请求成功；监听器运行、外部包在线、解码成功、上游响应和完整 Exchange 都是独立证据门。
