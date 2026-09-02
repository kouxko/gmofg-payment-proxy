# ADR-008: MCP 环境配置合同与运行时启用

- Status: Accepted; implementation staged
- Current runtime: Enabled by G036
- 日期：2026-08-26
- Supersedes: [ADR-004](ADR-004-embedded-read-only-mcp.md)

## 决策

接受 MCP 对话式环境配置作为 ADR-004 只读、本机边界的后继方向。最终能力使用明文 Streamable
HTTP 监听所有本机网络接口，不增加认证、授权、TLS、Host allowlist、Origin gate 或来源 IP
过滤。任何网络可达方都可能读取已公开数据、提交私有材料并发起配置写入；候选确认令牌仅用于
候选新鲜度和一次性应用，不证明身份、权限或用户在场。

写入合同采用严格的 `environment_configuration_candidate.v1`，不直接复用可持久化
`ProxyWorkspace`。客户端只能提交 Workspace 模板、现有目标/规则选择器和材料别名，不能提交最终
证书/秘密引用、新 Workspace ID、revision、运行态或 `created_order`。Protocol Document 值继续
使用当前 `{type,value}` wire，终止动作继续使用当前 Domain serde wire，所有 owned DTO 边界拒绝
未知字段。

Application 是候选解析、生命周期、验证、租约、确认令牌和 apply task 的所有者；MCP 只负责
wire、Schema、catalog 与 transport 适配。Infrastructure 通过受保护材料准备端口与唯一的
`EnvironmentCommitPort` 完成持久化。保护材料在 SQLite 事务前准备，Workspace 与最终材料引用在
单个 SQLite `IMMEDIATE` 事务中提交；Application 不通过旧的 save/import/store 路径拼装部分成功。

## 运行时实施边界

G036 已完成运行时接入：

- 生产 catalog 现在包含 34 个只读工具和五个环境配置工具；这些工具的名称、只读注解和预算保持
  不变。
- `mcp_environment_capabilities`、`environment_candidate_create`、`environment_candidate_status`、
  `environment_candidate_cancel`、`environment_candidate_apply` 已接入类型化 dispatch 和
  Application 候选生命周期。
- MCP 通过 `0.0.0.0:17653` 提供必需的 IPv4 服务，并尝试通过 `[::]:17653` 提供 IPv6 服务；
  IPv4 绑定失败是启动致命错误，IPv4 成功后的 IPv6 不支持或绑定失败只形成准确的 capability warning。
- Streamable HTTP 保持明文且无认证。任意语法有效的 `Host`、缺失或任意语法有效的 `Origin`，以及
  缺失或任意 `Authorization`、API key、Cookie 都不参与授权判断。
- MCP 只适配 Application API，不直接访问 SQLite、受保护材料实现或任意本机文件。

该实施明确替代 ADR-004 的 loopback-only、read-only 运行时事实。ADR-004 继续保留为历史决策记录，
不得作为当前监听或工具能力说明。

## 理由

- 显式模板把客户端可写字段与服务端生成/持久化字段分开，避免把 revision、运行态或最终引用引入
  公共写合同。
- Application 统一持有候选与 apply 权限，MCP 适配器不直接依赖数据库、保护器或运行时实现。
- apply-time lease 描述 Application 可观察状态在事务线性化点的一致性，不虚假承诺物理 Socket、
  外部服务或系统密钥库参与分布式原子提交。
- 分阶段实施先锁定公共合同，再接入生产 transport/catalog/dispatch，避免在候选生命周期、租约和
  原子提交尚未完成时暴露不可履约写接口。

## 后果

- 公共合同包含五个活动工具：capabilities、create、status、cancel、apply。capabilities/status 的
  注解为 read-only、非 destructive、idempotent；create 为非 read-only、非 destructive、非
  idempotent；cancel 为非 read-only、destructive、idempotent；apply 为非 read-only、destructive、
  非 idempotent。
- capabilities 及原有只读工具使用 256 KiB 输入、8 MiB 输出和 8 秒期限；create 使用 1 MiB
  输入/输出和 30 秒验证期限；status/cancel/apply 使用 16 KiB 输入、1 MiB 输出和 8 秒 ack 期限。
- create 在返回候选前仍归请求所有；请求取消或断开会取消创建并清理私有材料。apply 成功 ack 后由
  Application owned task 持有生命周期；调用方断开不会取消已经排队或进行中的 apply。
- `committed` terminal result 是唯一携带持久化 Workspace ID/revision 的变体，且
  `status_code` 必须为 null；其他 terminal result 必须携带一个已注册的非空状态码且不能伪造
  持久化标识。
- 新目标禁止 `existing_rule_id` 和持久化 Listener ID；同一候选不能重复使用 HTTP 或 Protocol
  Document 的现有规则选择器。
- 弱网可空字段必须显式出现并以 JSON null 表示缺值；省略与 null 不等价。
- 封闭 literal registry、无静默默认/降级和秘密不进入公开输出仍是运行时门禁。私钥、密码、
  confirmation token、保护后字节和原始请求体不得进入预览、状态、终态、日志或诊断；明文传输本身
  仍会让网络观察者看到客户端提交的输入。

## Alternatives

- 直接公开当前 `ProxyWorkspace`：拒绝，因为会混合客户端输入与服务端身份、revision 和最终引用。
- 让 MCP 直接调用现有多条 save/import/store API：拒绝，因为会拆分提交权限并产生部分成功窗口。
- 在合同阶段立即把五个工具加入生产 catalog/dispatch：拒绝，因为候选生命周期、验证、租约和原子
  commit 尚未实现，提前暴露会制造不可履约公共接口。
- 为远程写入增加认证、TLS 或 CIDR：不采用，因为与已确认产品边界冲突；未来安全远程管理需要独立
  任务与 ADR。
