# Intercept Proxy 文档

本文是项目文档总入口。当前源码是最终事实来源；架构文档描述已落地行为，ADR 保存重要决策历史，
测试矩阵定义可重复验收范围。

## 新人接手

- [新人接手与项目全景指南](onboarding-guide.md)：按“能运行、能观察、能定位、能修改、能验证、能发布”
  逐步理解产品、术语、代码组织、运行流程、测试、CI、安全边界和第一月学习路线。

## 产品与操作

- [需求与验收基线](requirements.md)：产品边界、功能要求和不支持能力。
- [用户操作说明](user-operation-guide.md)：Workspace、Listener、规则、协议包、抓包、日志和证书操作。

## 待实现任务

- [Android 目标设备后台刷新抖动修复](tasks/pending/2026-08-27/android-device-list-refresh-jitter.md)：
  修复每秒后台设备查询导致目标设备下拉框反复切换禁用状态的问题，并确认自动发现策略。
- [规则创建入口与协议入口绑定修复](tasks/pending/2026-08-27/rule-creation-entry-requirements.md)：
  修复无兼容 Listener 时仍可选择 Body/Socket 但不显示编辑器的问题，并确认空白规则及普通 HTTP 规则的绑定语义。
- [最终归档场景复跑与 MCP 验证经验指南](tasks/pending/2026-08-27/final-archive-replay-and-mcp-validation-playbook.md)：
  最终提交前重跑归档测试与运行中 Proxy 场景，并把稳定诊断方法发布为 MCP 只读验证指南。

## 架构

- [架构文档入口](architecture/README.md)
- [模块与代码组织](architecture/modules.md)
- [Exchange 与 Pipeline](architecture/exchange-pipeline.md)
- [真实数据流、错误与验证](architecture/data-flow.md)
- [规则、Document 与协议包](architecture/rules-and-protocol-packages.md)
- [运行时观测与诊断](architecture/runtime-observability.md)
- [安全、TLS 与持久化](architecture/security-and-persistence.md)
- [Android VPN 透明路由](architecture/android-vpn-transparent-routing.md)
- [开发与维护指南](architecture/development-guide.md)

## 架构决策

- [ADR-001：HTTP 与 Socket 边界](architecture/decisions/ADR-001-http-socket-boundary.md)
- [ADR-002：HTTP 协议包](architecture/decisions/ADR-002-protocol-packages-http.md)
- [ADR-003：应用 ZIP 所有权](architecture/decisions/ADR-003-application-zip-ownership.md)
- [ADR-004：内嵌只读 MCP](architecture/decisions/ADR-004-embedded-read-only-mcp.md)
- [ADR-005：运行证据与复现报告](architecture/decisions/ADR-005-runtime-evidence-and-reproduction-report.md)
- [ADR-006：统一 Exchange 观测（已被替代）](architecture/decisions/ADR-006-unified-exchange-observation.md)
- [ADR-007：Exchange/Pipeline 运行边界](architecture/decisions/ADR-007-exchange-pipeline-runtime-boundary.md)
- [ADR-008：MCP 环境配置合同与分阶段启用](architecture/decisions/ADR-008-mcp-environment-configuration.md)

ADR-006 仅作为决策历史保留；当前实现以 ADR-007 和源码为准。讨论期的 Exchange
概念代码保存在 [Exchange Pipeline Template](architecture/exchange-pipeline-template/README.md)，它用于解释
抽象来源，不替代生产源码。

## MCP 与外部包

- [MCP 只读工具参考](mcp/tool-reference.md)
- [应用接入 MCP 指南](mcp/app-integration-guide.md)
- [诊断架构](mcp/diagnostic-architecture.md)
- [外部协议包接入](mcp/external-package-integration-guide.md)
- [证书概念](mcp/certificate-concepts.md)

## 测试与发布

- [快速配置验证索引与模板](testing/quick-validations/README.md)：用于证书、URL、Host、Port、TLS/mTLS
  和临时 Proxy 配置的分层观察、清理记录与复测，不替代正式任务验收。
- [可复用测试证据索引](testing/evidence/README.md)：按任务和用例查询测试资源、步骤、结果与派生关系。
- [发布级验证矩阵](testing/release-validation-matrix.md)：固定层级、端口、场景、命令和判定标准。
- [2026-08-25 App 测试结果](testing/release-validation-results-20260825.md)：最新 release App 测试用例与结果。
- [2026-08-24 发布验证结果](testing/release-validation-results-20260824.md)：完整自动化、真实 App 和外部包证据。

验证结果只能证明当次源码与环境。修复任何功能后，应从受影响层开始重跑，并在最终 App 构建后
重新进行真实 UI/网络冒烟。
