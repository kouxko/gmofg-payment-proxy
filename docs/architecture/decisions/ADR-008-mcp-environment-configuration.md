# ADR-008: MCP 环境配置合同与分阶段启用

- Status: Accepted; implementation staged
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
wire、Schema、catalog 与 transport 适配。Infrastructure 后续通过受保护材料准备端口与唯一的
`EnvironmentCommitPort` 完成持久化。保护材料在 SQLite 事务前准备，Workspace 与最终材料引用在
单个 SQLite `IMMEDIATE` 事务中提交；Application 不通过旧的 save/import/store 路径拼装部分成功。

## 分阶段实施边界

本 ADR 的接受不等于写工具已经对外启用。当前阶段仅交付：

- Application 严格候选 DTO、解析器和显式 tagged terminal-result 合同；
- 五个计划工具的独立 contract registry、封闭 input/output Schema、注解和公共 literal registry；
- create 参数通过 Application 权威解析器校验。

独立 contract registry 不合并到现有 `catalog::tools`、dispatch、server 或 endpoint。当前 MCP
运行时继续遵守 ADR-004 的 loopback-only、read-only 实现事实，直到 G036 完成运行时接入及对应
验证。文档和代码不得在此前宣称远程写工具已经可调用。

## 理由

- 显式模板把客户端可写字段与服务端生成/持久化字段分开，避免把 revision、运行态或最终引用引入
  公共写合同。
- Application 统一持有候选与 apply 权限，MCP 适配器不直接依赖数据库、保护器或运行时实现。
- apply-time lease 描述 Application 可观察状态在事务线性化点的一致性，不虚假承诺物理 Socket、
  外部服务或系统密钥库参与分布式原子提交。
- 分阶段启用使合同能够先被回归测试锁定，同时保持现有生产 MCP 的只读边界不变。

## 后果

- 公共合同包含五个计划工具：capabilities、create、status、cancel、apply；它们具有混合的
  read-only/destructive/idempotent 注解。
- `committed` terminal result 是唯一携带持久化 Workspace ID/revision 的变体，且
  `status_code` 必须为 null；其他 terminal result 必须携带一个已注册的非空状态码且不能伪造
  持久化标识。
- 新目标禁止 `existing_rule_id` 和持久化 Listener ID；同一候选不能重复使用 HTTP 或 Protocol
  Document 的现有规则选择器。
- 弱网可空字段必须显式出现并以 JSON null 表示缺值；省略与 null 不等价。
- 后续运行时接入必须保留封闭 literal registry、无静默默认/降级、秘密不进入公开输出，以及
  ADR-004 到本 ADR 的事实性迁移说明。

## Alternatives

- 直接公开当前 `ProxyWorkspace`：拒绝，因为会混合客户端输入与服务端身份、revision 和最终引用。
- 让 MCP 直接调用现有多条 save/import/store API：拒绝，因为会拆分提交权限并产生部分成功窗口。
- 在合同阶段立即把五个工具加入生产 catalog/dispatch：拒绝，因为候选生命周期、验证、租约和原子
  commit 尚未实现，提前暴露会制造不可履约公共接口。
- 为远程写入增加认证、TLS 或 CIDR：不采用，因为与已确认产品边界冲突；未来安全远程管理需要独立
  任务与 ADR。
