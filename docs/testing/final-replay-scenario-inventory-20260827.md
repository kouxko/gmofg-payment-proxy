# 2026-08-27 最终归档场景复跑清单

本清单属于 `TASK-20260827-003`。它只记录本次重新执行的事实；历史结果仅用于发现资源和复测入口，
不自动继承为当前结论。

状态只使用：

- `PASS`：本次已按适用层级重新执行并满足预期。
- `FAIL`：本次结果违反当前合同。
- `NOT_RUN`：缺少真实设备、远端资源或人工交互条件，列出缺失条件和复测入口。
- `PENDING`：已进入本次计划但尚未执行完。

## 场景矩阵

| 场景 | 来源 | 本次层级与入口 | 当前结果 | 说明 |
| --- | --- | --- | --- | --- |
| 治理规则与文档结构 | `DOC-GOV-001..006` | 文档链接、结构、任务索引、锁协议 replay | `PASS` | 锁协议 replay 与文档结构门禁通过 |
| 架构与源码规模 | `FINAL-ARCHITECTURE-VALIDATION` | `pnpm scan:architecture`、`pnpm scan:source-size` | `PASS` | 当前源码重新执行通过 |
| 日志与 Exchange 观测 | `G029-OBSERVABILITY` | Application/Infrastructure/MCP 聚焦、诊断与抓包 UI | `PASS` | 日志稳定倒序与 Exchange 实时刷新通过 |
| 外部协议包故障隔离 | `G030-EXTERNAL-PACKAGE-FAULT-ISOLATION` | Infrastructure 外部包与真实本地 WebSocket/Socket 回归 | `PASS` | 外部包完整回归与本地服务测试通过 |
| 上游双 CA Bundle | `TLS-CA-BUNDLE-001` | 归档 PEM 解析、链验证、Rust TLS Trust Store、远端分层探测 | `PASS` | TCP、链验证和 TLS 1.3 零业务字节握手通过 |
| Nuvei Tango Python 包 | `NUVEI-PKG-001..003` | Python 单元、本机 WebSocket、编译检查 | `PASS` | 14/14，通过；未发送生产交易 |
| Nuvei Tango Rhai 包 | `NTR-RHAI-001` | Rhai Host 编译、Frame/Decode/Display/Encode、Python parity | `PASS` | 6/6 与编译门禁通过 |
| Nuvei Tango 真实链路 | `NTR-RHAI-002` | 真实 Listener 与授权后端 | `NOT_RUN` | 当前没有已授权的在线 Tango 交易端点和测试交易窗口 |
| MCP 环境配置合同 | `MCP-CONFIG-CONTRACT-001` | MCP schema、typed dispatch、生命周期、错误和 resource | `PASS` | 本次 MCP 定向 81/81 通过 |
| 打包 App 与 MCP 重启恢复 | `MCP-CONFIG-APP-001` | 新构建隔离 App，create/apply/status、退出重启、资源读取 | `PASS` | `preview_ready`→`apply_queued`→`committed`，重启后 MCP 可用 |
| Android 多设备所有权 | `TASK-20260827-001` | Application/Infrastructure/UI 多设备与 epoch 回归 | `PASS` | 自动化合同通过；真机部分单独为 `NOT_RUN` |
| Android 真机 A/B 并行 | `TASK-20260827-001` | 两台真实设备、逐 serial apply/stop/emergency | `NOT_RUN` | 当前未连接 Android 设备，缺少两台设备和 Companion 运行环境 |
| 日志最新在前 | `TASK-20260827-002` | Application 稳定倒序与 UI 返回顺序 | `PASS` | Application 与前端全量通过 |
| HTTP 响应生成 Mock 草稿 | `TASK-20260827-002` | Application 合同、Tauri 参数、UI 跳转、真实 HTTP Proxy 事件 | `PASS` | 草稿 3/3、前端相关 14/14、Runtime 真实 HTTP 观测回归通过 |
| HTTP/Socket/TLS/mTLS 数据面 | 发布验证矩阵与 Proxy tests | 启动真实 Listener，发送实际 loopback 请求并检查 wire/result | `PASS` | 本次 Runtime 225/225 通过，覆盖 HTTP、Socket、TLS、mTLS 与真实端口释放 |
| 外部包真实数据面 | `scripts/e2e_external_packages.py` | Deno/AU EFTEX 本地服务与真实 Socket/协议回归 | `PASS` | Deno 14/14、AU EFTEX 72/72，Infrastructure/Runtime 数据面通过 |
| MCP 生产非回环 IPv4 实调 | `MCP-CONFIG-IPV6-001` | wildcard Listener + 当前 LAN IPv4 源/目标 + 五个环境工具 | `NOT_RUN` | 本机 Proton VPN 透明代理截流同机 LAN 流量，严格 10 秒期限超时；未把 TCP 表面连接当成功，交 Windows CI 复验 |
| 最终本地门禁 | 发布验证矩阵 8.1/8.3 | 前端、Rust、Windows 静态检查、打包 App | `PASS` | 除单一本机网络环境用例外全部适用门禁通过；远程 Windows CI 在最终交付后执行 |

## 已确认的当前环境

- 操作系统：macOS arm64。
- Android：ADB 可用，但当前无已连接设备。
- Deno：可用。
- AU EFTEX Python 环境与可执行入口：可用。
- 打包 App：已有旧隔离构建；最终结论必须使用本次重新构建产物。
- 固定业务端口：执行真实场景前逐一确认空闲，结束后确认释放。

## 最终停止条件

- 任一自动化或适用真实链路为 `FAIL`：停止归档与交付，先修复并重新执行。
- 外部服务或真机不可用：保留 `NOT_RUN` 和准确缺失条件，不用低层测试替代。
- 只有本地全部适用门禁通过、真实 Proxy 场景完成、MCP 指南可读取、整体审查无发现后，才进入最终远程 Windows CI。
