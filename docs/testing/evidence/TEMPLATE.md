# 正式任务测试证据模板

复制到 `docs/testing/evidence/<执行日期>/<任务ID>/<用例ID>/README.md`。只创建实际有内容的
`resources/`、`inputs/`、`outputs/`、`steps/`、`replay/`；不适用项在 README 或 `metadata.json`
写 `N/A` 和原因，不创建空占位文件或伪造报文、截图、日志。

## 基本信息

- 任务 ID：`TASK-YYYYMMDD-NNN`
- 用例 ID：
- 执行时间：`YYYY-MM-DD HH:mm:ss +08:00`
- 执行者与稳定 Session ID：
- 被测 commit / worktree 状态：
- 目的：
- 最高验证层级：`L1 | L2 | L3 | L4`
- 最终结果：`PASS | FAILED | BLOCKED | NOT_RUN`

状态语义：

- `PASS`：本用例要求的全部层级有当次证据。
- `FAILED`：实现或实际结果违反合同；保存首个失败层和原始输出。
- `BLOCKED`：必测层被明确外部条件阻塞；保存阻塞条件、解除方式和已经完成的证据。
- `NOT_RUN`：该层完全没有执行；保存原因、必要前置条件和复测入口。不得由较低层 PASS 替代。

## 来源与派生关系

- 活动 fixture 来源、版本和实际使用方式：`N/A` 或仓库相对路径
- `derived_from`：`N/A`，或同时填写父任务 ID、父用例 ID、父证据相对路径
- 本次相对父用例的变化：
- 必须保持不变的合同：

## 环境与被测状态

- 操作系统、架构、时区：
- Proxy/App 构建来源与启动方式：
- 工具、runtime 与版本：
- Workspace、Listener、runtime epoch：
- 精确协议包 `id + version`、source、online/enabled：`N/A` 或实际值
- `/packages` URL、connection ID、registration fingerprint：`N/A` 或实际值
- Schema 版本与启动分支：`N/A | Schema 100 preserve | pre-100 development recreate`
- 外部服务、网络、证书、硬件和人工环境：
- 测试期间被测对象是否变化：`否`；若为是，本次结果无效并重新执行

## 输入、预期与步骤

- 前置条件：
- 实际输入与配置：指向 `inputs/` 或 `resources/`
- 预期输出、状态转换和稳定错误：
- 执行命令：
- 成功判定：
- 停止条件：
- 清理与恢复步骤：

精确可复测命令放入 `replay/`；准备、成功路径和清理说明放入 `steps/`。用户提供或测试实际依赖的
文件必须完整保存到 `resources/`，记录原始文件名、来源、用途、是否必需和加载位置。

## 实际结果

| 层级/步骤 | 状态 | 实际结果 | 原始证据 | 不能证明的内容 |
| --- | --- | --- | --- | --- |
|  | NOT_RUN |  |  |  |

协议或线路测试按适用保存 TCP chunks、完整 Frame、Decode Document、Rules、Encode、Server 实际收到、
响应和 App 最终收到。规则处理证据记录：

- `received.document`
- typed `processed.changes`：rule ID、matched、`record_match | set | clear | insert | append`、RFC 6901 path
- `changes_truncated`
- `processed.final_document`
- `encoded.context`、`sent.context` 与对端实际接收
- `failed.external_package_call`：stage、method、request ID、remote code、stable code 和有界 data 摘要

`changes_truncated=true` 表示过程操作摘要触及容量限制；不能证明未列出的动作没有执行。应继续用
`final_document`、Encode、实际写出和对端接收判断结果。Display 失败只能证明观测回退，不能替代业务
Frame/Decode/Rules/Encode 结论。

## 比较与结论

- 逐字段/逐字节 expected：
- actual：
- comparison：
- 首个失败或阻塞层：
- 稳定错误码：
- 已验证：
- 未验证与 `NOT_RUN`：
- 剩余风险：
- 清理后是否恢复测试前状态：

## metadata.json 最低字段

```json
{
  "task_id": "TASK-YYYYMMDD-NNN",
  "case_id": "case-id",
  "executed_at": "YYYY-MM-DD HH:mm:ss +08:00",
  "result": "NOT_RUN",
  "runner": {
    "session_id": "agent-session-id",
    "process_id": null
  },
  "tested_state": {
    "commit": null,
    "worktree_sha256": null,
    "changed_during_run": false
  },
  "environment": {
    "os_arch": null,
    "timezone": "Asia/Shanghai",
    "tools": [],
    "external_dependencies": []
  },
  "package": null,
  "schema": null,
  "inputs": [],
  "outputs": [],
  "layer_results": {},
  "process_evidence": {
    "changes_truncated": null,
    "final_document": null,
    "encoded_context": null,
    "sent_context": null
  },
  "stable_errors": [],
  "cleanup": {
    "required": false,
    "status": "NOT_RUN",
    "baseline_match": null
  },
  "replay": null,
  "derived_from": null,
  "not_applicable": [],
  "not_run": []
}
```
