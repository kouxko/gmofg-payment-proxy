# MCP 只读工具参考

本文档是 Intercept Proxy 内嵌 MCP 服务的完整工具目录。服务监听 `127.0.0.1`，使用 MCP `2026-07-28` 协议，只提供读取能力；任何工具都不会修改应用配置、运行态、规则、抓包、文件或协议包。

## 通用调用契约

- 参数必须是 JSON object。每个工具发布的 input schema 都声明 `additionalProperties: false`；该约束递归应用到 `page`、`package` 等嵌套对象，任意层级的未知字段都会返回 `INVALID_ARGUMENTS`，不会被静默忽略。
- 工具目录同时发布 output schema。成功结果通过 MCP `structuredContent` 返回；运行时会校验 object、array、object/null 根类型与公开 Schema 一致，下表的“成功结果”描述其根类型和语义投影。
- 错误结果同样是结构化对象，至少包含 `code`、`message`，应用错误还可包含 `details`。常见代码包括 `INVALID_ARGUMENTS`、`NOT_FOUND`、`INPUT_BUDGET_EXCEEDED`、`OUTPUT_BUDGET_EXCEEDED`、`OUTPUT_SCHEMA_MISMATCH` 和 `TOOL_DEADLINE_EXCEEDED`。`OUTPUT_SCHEMA_MISMATCH` 表示后端成功值违反公开输出合同，应视为服务端缺陷，而不是客户端重试条件。
- 单次输入逻辑 JSON 上限为 256 KiB，单次输出上限为 8 MiB，执行期限为 8 秒。
- 分页工具只返回当前保留窗口中的数据；日志、诊断、HTTP 抓包和 Exchange 观察都可能因有界保留策略而淘汰旧记录。调用方应保存稳定 ID、游标和 `runtime_epoch`，并显式处理记录已不在保留范围的情况。
- 所有 UUID 参数均以字符串传入。标为“无”的参数应传 `{}` 或省略 `arguments`。

## 通用、诊断与工作区

| 工具 | 参数 | 成功结果 | 说明 |
| --- | --- | --- | --- |
| `application_snapshot` | 无 | object：经有界双读校验的设置、工作区、运行态、协议包、规则和诊断快照 | 状态持续变化时可返回 `SNAPSHOT_UNSTABLE`。 |
| `application_log_query` | 可选 `level`、`target`、`keyword`、`occurred_from`、`occurred_to`、`before_log_id`、`limit` | object：稳定游标分页的 Rust/Tauri 持久化日志与保留元数据 | `limit` 默认 200，最大 500。 |
| `application_log_get` | 必填 `log_id` | object：一条保留的运行日志 | 已淘汰的 ID 返回 `NOT_FOUND`。 |
| `exchange_observation_query` | 必填 `workspace_id`、`page`；可选 `listener_id` | object：连接级 Exchange 页及保留信息 | `page` 含必填的 `page`、`page_size`，页大小最大 200。 |
| `exchange_observation_get` | 必填 `exchange_id` | object：一条完整的连接级 Exchange 观察 | 已淘汰的 ID 返回 `NOT_FOUND`。 |
| `reproduction_report` | 必填 `workspace_id`、`listener_id` | object：`bundle`、有界 `application_logs` 和可复制 `markdown` | 只汇总配置、监听器运行态、结构化诊断、日志与协议包现场；不读取 `ExchangeObservationStore`，也不包含 HTTP 抓包。Exchange 和 HTTP 证据必须分别调用对应查询工具。 |
| `settings_get` | 无 | object：已保存的全局设置 | 读取持久化投影。 |
| `workspace_list` | 无 | array：全部工作区摘要 | 不包含完整子项。 |
| `workspace_get` | 必填 `workspace_id` | object：完整工作区及 entry、Android profile、证书引用和协议规则绑定 | 不返回私钥材料。 |
| `entry_overview` | 必填 `workspace_id` | object：工作区配置 entry 与当前运行态的合并视图 | 用于判断配置与活动监听器是否一致。 |
| `entry_status_list` | 无 | array：全部 entry 的当前运行状态 | 运行时投影。 |
| `diagnostics_query` | 可选 `keyword`、`after_event_id`、`limit` | object：最新优先的结构化诊断与保留信息 | `limit` 默认 300，最大 500。 |
| `diagnose_recent_failures` | 同 `diagnostics_query` | object：证据、确定性排障建议和验证步骤 | 只生成建议，不执行修复。 |

## 运行态、Android 与证书

| 工具 | 参数 | 成功结果 | 说明 |
| --- | --- | --- | --- |
| `external_package_service_status` | 无 | object：外部协议包 WebSocket 地址、固定路径、绑定状态、认证边界和在线数 | 是服务可达性的权威运行态。 |
| `android_adb_get` | 无 | object：当前 ADB 与选中设备状态 | 不会连接或切换设备。 |
| `android_device_list` | 无 | array：已连接 Android 设备 | 只读实时查询。 |
| `android_package_list` | 无 | array：选中设备的缓存包清单 | 依赖当前选中设备。 |
| `android_package_get` | 必填 `package_name` | object：一个 Android 包的详情 | 未找到时返回应用错误。 |
| `android_profile_list` | 无 | array：当前选中工作区的 Android 网络 profile | 依赖应用当前选择。 |
| `android_profile_get` | 必填 `profile_id` | object：一个 Android 网络 profile | 未找到时返回应用错误。 |
| `android_network_status` | 无 | object：当前 Android 网络状态 | 不触发网络变更。 |
| `android_runtime_owner` | 无 | object 或 null：拥有当前 Android 运行态的持久化 profile | null 表示没有 owner。 |
| `android_network_endpoints` | 可选 `profile_id` | object：配置端点与活动运行端点 | 不修改设备；省略时按应用当前上下文解析。 |
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
| `http_rule_list` | 无 | array：按运行顺序排列的 HTTP 规则摘要 | 只读当前应用投影。 |
| `http_rule_get` | 必填 `rule_id` | object：一条完整 HTTP 规则 | 不执行规则。 |
| `protocol_rule_list` | 无 | array：当前选中工作区的四阶段协议 Document 规则 | 依赖应用当前选择。 |
| `workspace_protocol_rule_list` | 必填 `workspace_id` | array：指定已保存工作区的协议 Document 规则 | 不依赖当前选择。 |
| `protocol_package_list` | 无 | array：所有已安装不可变协议包版本及使用数 | 安装源文件不在 Application facade 中暴露。 |
| `protocol_package_catalog` | 无 | object：已启用且已完成能力描述的协议包、方向能力、Schema 和目录校验信息 | 在线不等于能力描述成功。 |
| `protocol_package_detail` | 必填 `package.id`、`package.version` | object：精确版本的 manifest 投影、方向能力、Schema 与 entry 使用情况 | 不返回安装源文件。 |
| `protocol_package_usage` | 必填 `package.id`、`package.version` | array：引用该精确版本的工作区/entry | 版本是不可变身份的一部分。 |

## 证据组合建议

一次可复现排障通常先调用 `reproduction_report` 固定工作区和监听器上下文，再按故障类型补充 `exchange_observation_query` / `exchange_observation_get`、`http_capture_query` / `http_capture_get` 或 `application_log_query` / `application_log_get`。不要把“工具调用成功”误认为业务请求成功；监听器运行、外部包在线、解码成功、上游响应和完整 Exchange 都是独立证据门。
