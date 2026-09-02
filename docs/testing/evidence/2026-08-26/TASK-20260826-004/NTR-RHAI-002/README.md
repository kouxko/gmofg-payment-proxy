# NTR-RHAI-002：真实 Listener 双向 Exchange

- 任务：`TASK-20260826-004`
- 派生自：`TASK-20260826-003 / NUVEI-PKG-003`
- 检查时间：2026-08-26 16:30:00 至 16:33:09 +08:00
- 结果：NOT_RUN

## 目标

把最终 Rhai ZIP 导入并启用到真实 Proxy，为 Nuvei Socket Listener 绑定该包，再由授权测试 App 产生
1602/647、1602/914、1322/896 B 三组双向 Exchange，证明 Listener 数据面上的 Frame、Decode、
Display、Encode 均成功且输出字节数不变。

## 当前检查

- `10.0.28.85:8765` 的 external-package WebSocket TCP 入口可连接，但该入口只能注册 external package，
  不能导入或启用内置 Rhai ZIP。
- 当前执行主机地址为 `10.0.34.61`，本机没有 8765 或 9081 相关 Listener。
- `adb devices -l` 没有连接设备，当前会话也没有授权测试 App 可发起交易。
- 当前 MCP 是只读合同，不能远程导入包、变更 Listener 或启动交易。

因此本次没有执行任何真实交易，没有生成同一次 Exchange 的因果日志，也没有把
`TASK-20260826-003` 的历史成功记录改写成本任务的当前 PASS。包级对同字节数的合成双向验证已在
`NTR-RHAI-001` PASS，但不能替代本用例。

## 阻塞解除条件

需要能够操作 `10.0.28.85` 上的 Proxy UI，或提供等价的已授权写入控制面，并由授权测试 App 在包绑定
后产生三组交易。本用例只保存方向、阶段、字节数、结果和连接关联，不保存真实 JSON 原文。
