# Phase 20 Windows CI 与本地 Schema 运行时验证

## 目的

验证 Windows `build-only/windows` 打包修复，以及本地 Release App 的无 Schema HTTP JSON Body 与带 Schema Socket 规则生产链路。

## 结果

- Windows CI：PASS。Windows sidecar 在 Tauri 打包前完成 staging，MSI、NSIS 与 portable artifact 均生成。
- Plain HTTP：PASS。`/customer/age` number equals `18` 命中；`17` 不命中；非法 JSON 在 Decode 阶段 fail-closed 且不发送到 Server。
- Socket Schema：PASS。`iso8583-ascii-standard@1.0.0` 的 `/message_type` 条件命中 `0200`，动作写为 `0220`；`0400` 保持不变；非法 Frame Decode fail-closed。

## 资源与输出

- `inputs/`：实际发送的 HTTP Body 与 Socket Frame。
- `resources/`：MCP apply 使用的候选配置及状态。
- `outputs/`：客户端结果、Server 实收、规则/Workspace/Exchange 读回、日志和当次隔离数据库。
- `steps/replay.md`：本地复测步骤。

`outputs/attempt-1-invalid-environment/` 是受控 Socket Server 提前退出的无效环境轮次，不计产品结论；保留用于区分环境失败与有效重放。

## 不适用项

- Developer ID、发布签名与 GitHub Release：N/A；本轮只验证 unsigned Windows 构建和本地 macOS 规则行为。
- 人工系统权限：本轮未出现权限弹窗。
