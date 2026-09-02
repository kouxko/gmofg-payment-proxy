# UNIFIED-RULE-CONTRACT-001

- 目的：验证 HTTP 与 Socket 使用单一 `RuleDefinition` 聚合、统一阶段、统一持久化集合与统一公共接口，同时保持各自类型化能力。
- 环境：macOS 本地 Domain、Application、Infrastructure、Host、MCP、前端和正式 App 构建。
- 核心场景：HTTP Header 与 Document 联合匹配；固定阶段顺序；同阶段 priority 升序；one-shot 与命中计数原子完成；编码或 HTTP 动作失败回滚；并发连接隔离；revision 冲突重试；旧或未来数据明确拒绝且数据库相关文件不变化。
- 公共合同：统一 list/get/save/toggle/copy/delete；统一 Environment `workspace.rules`；统一 MCP 工具；一个规则工作区和编辑器；Listener 不可改绑；Socket 不出现 HTTP 能力。
- 实际：Domain 159/159、Application 主测试 461 项及 14/7/5/12 集成组、Infrastructure 642/642、Host 30/30、根包 133/133、MCP 82/82；前端最终全量 61 文件 531 项；严格静态检查、格式、架构、边界、源码大小、类型、生成绑定和正式 App 构建均通过。
- 对抗审查：旧双路径、持久化拒绝、运行时原子性、Document Schema/阶段校验、Socket stage、前端能力与错误边界的发现均已修复并复审关闭。
- 结果：`PASS`。
