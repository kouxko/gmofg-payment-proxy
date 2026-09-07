# ADR-011：HTTP 写准备中的独立 Finalize 能力

- Status: Accepted
- 日期：2026-09-07
- Extends: [ADR-010](ADR-010-in-process-webassembly-protocol-packages.md) 的本地 HTTP Wasm 能力

## Context

HTTP 包需要在规则、条件性 Encode 和本轮正文动作完成后，读取最终 HTTP 信息并修改允许字段。
该能力必须覆盖无规则命中、Document 未变、Encode 跳过和 Mock 响应，同时兼容没有新导出的旧包。
仅修改 Encode 的参数或只在 Encode 实际运行时处理，会遗漏这些路径。

现有 HTTP actor 协调 Document、HTTP 动作和生命周期提交。若将新变换简单放到外层 Writer 前面，
规则可能已经提交，变换失败后不能保证本轮命中次数回滚。仅把方法附在 Prepare 上也不能明确表达
独立的 Finalize 能力，不符合既有能力装配风格。

## Decision

1. 在 Proxy HTTP 层定义独立 `Finalize` trait，使用原生 `HttpRequestMetadata` 与 `Message`，
   由 `HttpDirectionCapabilities` 明确组合装配。共享 Exchange 保留已有 PrepareWrite 边界。
2. 正常路径由 actor 在 Encode 和本轮正文动作后调用 Finalize，成功后才提交当前方向生命周期。
   joint transaction 和 Listener guard 保持到提交结束。
3. 旧 MockResponse 使用独立装配的 Finalize，保持跳过 downstream Rules/Encode 的既有行为。
   普通与 Mock 路径复用同一 `ExternalHttpFinalizeCall`，不复制 JSON、codec 和写回逻辑。
4. 本地 HTTP Wasm 增加两个独立可选导出；旧 world 必需 ABI、Manifest API 1、Schema100 不变。
   远程 RPC 与 Socket 不增加方法。
5. 输入输出为相同五字段 JSON：url、method、code、header、body。method 固定；upstream 只可改
   path/query；downstream 保留 App 原请求关联；Header 同名字段采用最后值。
6. 校验与写回在候选 Message 上完成。缺少可选导出可以跳过；执行故障、非法返回和合同违规必须
   传播，不回退发送进入 Finalize 前的旧消息。

## Why

- 独立 trait 明确表达最终变换能力；HTTP 原生类型保留 Header、状态行与正文原始字节，避免为
  泛型外观进行有损 Context 转换。
- 复用既有 actor 的 checkpoint 和提交模型，避免引入第二个事务协调器或“先提交再补救”。
- 可选导出发现区分“确实没有”和“存在但错误”，兼顾旧包兼容与可观察失败。
- 请求关联属于当前 transaction；固定连接仍由拓扑决定，包不获得隐式重路由能力。

## Consequences

- 包能在每次写准备的最终正文上处理派生字段，通过允许字段影响真实 HTTP 消息。
- Finalize 成功不等于发送成功；其后 Writer 失败仍遵循既有合同，不被称为 Finalize 回滚。
- 单值 Header 投影不能创建多个新同名值；未变化头的原始重复项通过差量写回保留。
- 数据库兼容范围为 Schema100 和既有持久化结构；历史版本迁移策略不由本决策改写。
- ABI、未修改/Mock、非法输出、取消与实际 wire 分别验证；低层测试不能代替 Writer 收发证据。

## Alternatives

- Rejected：由 Encode 同时承担 Finalize；Document 不变会跳过 Encode，无法覆盖每次写准备。
- Rejected：仅在外层 Writer 前调用；规则已经提交，失败会导致生命周期提前消费。
- Rejected：仅给 Prepare 增加 finalize_only；缺少独立能力装配，普通与 Mock 的责任不清楚。
- Rejected：把 HTTP 关联与 Header 写回塞入共享 Exchange Context；增加 Socket 无需的合同，
  Message/Context 往返还可能损失原始字节。
- Rejected：更改旧 world 必需 ABI或同时扩展远程 RPC；前者破坏旧包，后者超出当前范围。

## References

- [详细架构、接口、时序与验证矩阵](../http-finalize.md)
- [Exchange 与 Pipeline](../exchange-pipeline.md)
- [规则和协议包事务](../rules-and-protocol-packages.md)
