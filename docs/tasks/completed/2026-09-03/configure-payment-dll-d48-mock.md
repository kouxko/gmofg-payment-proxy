# TASK-20260903-001：通过 MCP 验证并配置 Payment DLL 的 D48 Mock

- 任务 ID：TASK-20260903-001
- 状态：已完成
- 任务日期：2026-09-03
- 创建时间：2026-09-03 12:27:22 +08:00
- 开始时间：2026-09-03 12:27:22 +08:00
- 最后更新时间：2026-09-03 12:37:48 +08:00
- 完成时间：2026-09-03 12:37:48 +08:00
- 创建路径：`docs/tasks/pending/2026-09-03/configure-payment-dll-d48-mock.md`
- 归档路径：`docs/tasks/completed/2026-09-03/configure-payment-dll-d48-mock.md`
- 关键词：`MCP`、`Payment DLL`、`HTTP`、`MockResponse`、`D48`
- 任务优先级：高（支付 HTTP 协议、活动设备路由和持久化 Workspace 配置）

## 背景与目标

当前运行 App 的 HTTP 抓包已经取得 `POST /` 的服务器响应。用户要求为 `GMOFG` Workspace 的
`Payment DLL` Listener 创建对应 Mock，并用 MCP 验证配置和运行结果。

## 范围与不在范围

- 保留当前 Listener、固定 Root、上游客户端身份、Android 网络方案和 `D48 -> T02` 规则。
- 新增启用的 request-stage HTTP Mock：`RequestTarget Equals /`，返回日志中的 HTTP 200、可转发
  Header 和完整 D48 JSON 字节。
- 不修改源码，不向真实上游发送交易；本机重放后恢复 Listener 的停止状态。
- 用户已要求加快并跳过对抗审查，本任务不执行额外对抗审查。

## 需求确认与就绪检查

- 2026-09-03 12:25 +08:00：用户要求“配置 mcp Payment DLL，用日志中的信息创建一个 mock”。
- 输入、输出、状态变化、示例和验收标准均已明确；`Content-Length` 由运行时管理，不写入规则。
- 进入实现时间：2026-09-03 12:27:22 +08:00；未确认事项为零。

## 问题与根因分析

- 现象：先前 UI 生成的动作参数只有业务 JSON，缺少 Mock 参数字段，因此提示动作类型不匹配。
- 当前已验证：UI 的动作参数框要求 MockResponse 内层参数，即 `status`、`headers`、`body_bytes`；保存后
  Domain 持久化为完整 `terminal.MockResponse`。
- MCP 完整候选的 schema 层通过，但 domain 层返回 `LISTENER_DOMAIN_INVALID`。源码合同要求 TLS 开启
  时 `server_identity_alias` 非空，而当前合法 UI 配置使用 `server_identity=null` 表示由固定 Root 按 SNI
  动态签发。因此该候选不能无损表达现有 Payment DLL Listener。
- 为避免关闭 TLS、替换固定证书或丢失上游身份引用，没有 apply 失败候选；通过 UI 的窄规则写入完成
  配置，再以 MCP 做权威回读和抓包验证。

## 方案比较与实施记录

- 最小改动：仅新增一条规则，保留其他配置。采用；因 MCP 全量候选无法无损表达现有 TLS 配置，写入
  改走规则编辑入口。
- 最优设计：后续独立修复环境候选对“固定 Root 动态签发”Listener 的表达合同；本任务不扩展源码范围。
- 新规则 ID：`b3996e35-0c57-4971-9a73-74b809d33aee`；Workspace revision `17 -> 18`。
- 测试期间 Listener 短暂启动；完成后恢复为停止。Android runtime owner 未修改。

## 修改文件与附加文件

- 运行配置：`GMOFG` Workspace 新增 `Payment DLL D48 Mock`。
- 证据：[PAYMENT-DLL-D48-MOCK-001](../../../testing/evidence/2026-09-03/TASK-20260903-001/PAYMENT-DLL-D48-MOCK-001/README.md)。
- 项目仅新增任务和测试证据文档；无源码修改。

## 验收结果、测试结果与 CI

- 结果：`PASS_WITH_MCP_FULL_CANDIDATE_INCOMPATIBILITY`。
- MCP 回读：规则启用、priority 100、`proxy_to_upstream`、`RequestTarget Equals /`、完整 200 MockResponse。
- 本机重放：目标指向不可达的 `127.0.0.1:9`，仍在 3 ms 返回 HTTP 200 和 118 字节 D48 正文；规则
  `hit_count=1`，抓包 `matched_rule_ids` 精确包含新规则，证明未连接真实上游。
- 固定 Root 指纹、Listener TLS、Android 路由和上游客户端身份引用均保持不变。
- 真机 Payment App 新交易：NOT_RUN；CI：N/A（无源码变更）。

## 完成总结

Payment DLL 的 D48 Mock 已创建、启用并通过本机真实 Listener 命中验证；MCP 完成配置回读和抓包
证明。直接 MCP 全环境 apply 的 TLS 表达不兼容已保留为明确限制，没有以降级 TLS 的方式绕过。
