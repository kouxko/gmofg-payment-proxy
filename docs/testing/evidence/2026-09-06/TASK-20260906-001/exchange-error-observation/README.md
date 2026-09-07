# exchange-error-observation

- 任务：TASK-20260906-001；执行日期：2026-09-06（Asia/Shanghai）。
- 结果：`LOCAL_UNIT_AND_LOOPBACK_TCP_PASS`。
- 目的：必需应答 EOF 保留 Read/Downstream；Socket external 字段完整且所有失败投影遵守既有 16 MiB。

## 前置条件与复测

使用归档对应的源码；工具、依赖准备、完整快照验证与复测入口见 [整体验证](../integrated-verification/README.md)。
在仓库根目录执行 `python3 <本用例父目录>/integrated-verification/replay/run.py <仓库根目录> exchange-error-observation`。

实际命令：

- `cargo test --offline --manifest-path src-tauri/Cargo.toml -p intercept-proxy-exchange --lib -- --nocapture`
- `cargo test --offline --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime socket_relay -- --nocapture`

## 预期与实际

- 预期：Server 应答前 EOF 返回 READ_FAILED/Read/Downstream；Socket 只有一个 RelayRead/ServerToClient 终态。
- 预期：HTTP/Socket/no-context 的 Context+message+external 文本总 UTF-8 字节恰好16MiB允许，超1字节 observation_lost。
- 预期：6MiB Socket Context +5MiB remote message 拒绝生成超预算投影；业务错误不被改写。
- 实际：初始 Exchange 25 pass/2 fail；真实 TCP EOF 初次终态错误，修复后 Socket63项通过。
- 实际：预算新增两项先RED失败，修复后 Exchange29项通过，6个临界投影断言全部通过。
- 实际：本机TCP实收 55616263，Server EOF，App未收到响应；计数4/4/0/0。

## 资源、步骤与证据

`runs/` 保留本次各阶段实际测试源码、配置、日志与原执行时间；各阶段 README/命令 JSON 解释对应步骤。
最新状态以整体验证快照及 `outputs/validation/review-fixes/` 为准；此前失败与修复中间态不覆盖、不冒充最终通过。
回归使用的文件型 fixture 已包含在全量源码快照；代码生成的输入以实际测试源码为可重复向量。
动态 UUID/epoch 使用测试内的真实值做逐字段比较，不重建虚假的“实际请求”文件。

- external_package_process：测试使用 ScriptedFactory；外部 RPC 实机超大消息路径未重放。
- android_and_business：N/A，本机合成协议，不是Android/远端业务验收。
- screenshots：N/A，无UI交互。

派生自 TASK-20260905-004 / final-verification / `docs/testing/evidence/2026-09-06/TASK-20260905-004/final-verification`；保留父用例原有行为，新增本节反例，父证据不改写。
