# Intercept Proxy 文档

本文是项目文档总入口。当前源码是最终事实来源；架构文档描述已落地行为，ADR 保存重要决策历史，
测试矩阵定义可重复验收范围。

## 产品与操作

- [需求与验收基线](requirements.md)：产品边界、功能要求和不支持能力。
- [用户操作说明](user-operation-guide.md)：Workspace、Listener、规则、协议包、抓包、日志和证书操作。

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

ADR-006 仅作为决策历史保留；当前实现以 ADR-007 和源码为准。讨论期的 Exchange
概念代码保存在 [Exchange Pipeline Template](architecture/exchange-pipeline-template/README.md)，它用于解释
抽象来源，不替代生产源码。

## MCP 与外部包

- [应用接入 MCP 指南](mcp/app-integration-guide.md)
- [诊断架构](mcp/diagnostic-architecture.md)
- [外部协议包接入](mcp/external-package-integration-guide.md)
- [证书概念](mcp/certificate-concepts.md)

## 测试与发布

- [发布级验证矩阵](testing/release-validation-matrix.md)：固定层级、端口、场景、命令和判定标准。
- [2026-08-24 验证结果](testing/release-validation-results-20260824.md)：最近一次自动化、真实 App 和外部包证据。

验证结果只能证明当次源码与环境。修复任何功能后，应从受影响层开始重跑，并在最终 App 构建后
重新进行真实 UI/网络冒烟。
