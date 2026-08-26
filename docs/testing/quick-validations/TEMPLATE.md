# 快速配置验证模板

本模板用于 `docs/testing/quick-validations/<执行日期>/<验证ID>/README.md`。删除不适用的空目录，
但在 README 或 `metadata.json` 中保留 `N/A` 和原因。

## 基本信息

- 验证 ID：`QV-YYYYMMDD-NNN`
- 生命周期：`RESERVED | RUNNING | FINAL`
- 最终结果：`null | VERIFIED | FAILED | INCONCLUSIVE | NOT_RUN`
- 执行时间：`YYYY-MM-DD HH:mm:ss +08:00`
- 执行者：
- 执行者 Session ID：
- 执行者独立 Process ID：`null` 或真实进程 ID
- 开始时间：
- 最后更新时间：
- 用户原始问题：
- 本次最高验证层级：
- 请求路径 ID：
- 是否存在关联正式任务：`无 | TASK-YYYYMMDD-NNN`

## 环境与外部依赖

- 操作系统和架构：
- 时区：
- Proxy 构建版本、来源和启动方式：
- App 构建版本、来源和启动方式：
- 测试工具、运行时和版本：
- 网络接口和地址：
- DNS 配置和当次解析上下文：
- 系统代理、VPN 或透明路由状态：

| 外部服务或硬件 | 版本或环境标识 | 来源 | 启动或连接方式 | 必要配置 | 当时可用性 |
| --- | --- | --- | --- | --- | --- |
|  |  |  |  |  |  |

不涉及项记录 `N/A` 和原因。

## 验证合同

- 目标 URL：`N/A` 或精确 URL
- Host：
- Port：
- 当次解析地址：
- 路径：`资源解析 | DNS | TCP | 直接 TLS | Proxy 上游测试 | 下游 Listener | 完整 Proxy 链路 | 应用协议`
- SNI：
- 主机名校验：`开启 | 关闭 | N/A`
- 信任来源：
- TLS 版本要求：
- mTLS：`是 | 否 | 未知`
- 客户端身份：
- 是否允许应用请求：
- 精确请求：`N/A` 或指向 `inputs/` 中的文件
- 预期响应：
- 成功标准：
- 停止条件：

## 测试资源

| 原始文件名 | 来源 | 实际角色 | 是否必需 | 使用位置 | 归档相对路径 |
| --- | --- | --- | --- | --- | --- |
|  |  |  |  |  |  |

没有文件型资源时记录 `N/A` 和原因。不得只记录测试时的临时绝对路径。

## 测试前状态

- 当前 Workspace：
- 已启用 Listener：
- 相关端口监听：
- 相关进程：
- Proxy 草稿或运行配置快照：`N/A` 或指向 `inputs/` 中的完整字段文件
- 临时文件或对象：
- 不适用项及原因：

## 执行步骤

1. 保存实际资源与精确配置。
2. 按层执行到用户要求的最高层级。
3. 保存每条命令、输入、stdout、stderr 和实际响应。
4. 执行清理并与测试前状态逐项比较。

具体命令和可复测入口保存在 `steps/` 与 `replay/`；实际输入和输出保存在 `inputs/` 与 `outputs/`。

## 路径与因果证据

| path_id | 客户端输入或连接 | Listener 会话/连接标识 | 对应上游连接与目标 | 返回方向证据 | 关联判定 |
| --- | --- | --- | --- | --- | --- |
|  |  |  |  |  |  |

完整 Proxy 链路没有同一 `path_id` 的连接映射证据时不得为 `VERIFIED`。

## 分层结果

| path_id | 层级 | 状态 | 精确输入或配置 | 实际结果 | 证据 | 不能证明的内容 |
| --- | --- | --- | --- | --- | --- | --- |
|  | 资源解析 | NOT_RUN |  |  |  |  |
|  | DNS | NOT_RUN |  |  |  |  |
|  | TCP | NOT_RUN |  |  |  |  |
|  | 直接 TLS | NOT_RUN |  |  |  |  |
|  | Proxy 上游测试 | NOT_RUN |  |  |  |  |
|  | 下游 Listener | NOT_RUN |  |  |  |  |
|  | 完整 Proxy 链路 | NOT_RUN |  |  |  |  |
|  | 应用协议 | NOT_RUN |  |  |  |  |

## 诊断变体

没有执行诊断变体时记录 `N/A`。执行时逐项记录与原配置的差异、目的、结果，并明确其结果不能替代
原配置结论。

## 清理结果

- 临时 Workspace：
- 临时 Listener：
- 临时端口和进程：
- 临时文件：
- 测试后状态与测试前是否一致：
- 清理结论：`NOT_RUN | VERIFIED | FAILED | INCONCLUSIVE | N/A`
- 保留对象及原因：`N/A` 或精确说明

## 最终结论

- 整体状态：`VERIFIED | FAILED | INCONCLUSIVE | NOT_RUN`
- 已验证：
- 未验证：
- 失败层级：
- 剩余风险：
- 是否需要升级为正式任务：
- 升级原因：

## 复测方式

- 复测入口：
- 所需资源：
- 前置条件：
- 预期结果：

## metadata.json 最低字段

```json
{
  "validation_id": "QV-YYYYMMDD-NNN",
  "lifecycle": "RUNNING",
  "result": null,
  "executed_at": "YYYY-MM-DD HH:mm:ss +08:00",
  "runner": {
    "session_id": "agent-session-id",
    "process_id": null,
    "started_at": "YYYY-MM-DD HH:mm:ss +08:00",
    "last_updated_at": "YYYY-MM-DD HH:mm:ss +08:00"
  },
  "environment": {
    "os_arch": "Darwin arm64",
    "timezone": "Asia/Shanghai",
    "proxy_build": null,
    "app_build": null,
    "tools": [],
    "network_context": {
      "interfaces": [],
      "dns": null,
      "system_proxy": null,
      "vpn_or_transparent_routing": null
    },
    "external_dependencies": []
  },
  "requested_level": "direct_tls",
  "requested_path_id": "path-001",
  "target": {
    "url": null,
    "host": "example.test",
    "port": 443,
    "resolved_addresses": []
  },
  "tls": {
    "sni": "example.test",
    "hostname_verification": true,
    "mtls": false
  },
  "layer_results": {
    "path-001": {
      "resource_parse": "NOT_RUN",
      "dns": "NOT_RUN",
      "tcp": "NOT_RUN",
      "direct_tls": "NOT_RUN",
      "proxy_upstream": "NOT_RUN",
      "downstream_listener": "NOT_RUN",
      "full_proxy_path": "NOT_RUN",
      "application_protocol": "NOT_RUN"
    }
  },
  "path_evidence": {
    "path-001": {
      "client_connection": null,
      "listener_session": null,
      "upstream_connection": null,
      "return_path": null,
      "causal_link_status": "NOT_RUN"
    }
  },
  "temporary_state": {
    "baseline": null,
    "declared_objects": []
  },
  "cleanup": {
    "required": false,
    "status": "NOT_RUN",
    "baseline_match": null,
    "retained_objects": []
  },
  "replay": null,
  "derived_from": null,
  "promoted_task_id": null,
  "not_applicable": []
}
```
