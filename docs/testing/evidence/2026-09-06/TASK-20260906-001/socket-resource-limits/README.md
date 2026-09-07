# socket-resource-limits

- 任务：TASK-20260906-001；执行日期：2026-09-06（Asia/Shanghai）。
- 结果：`OBSERVER_PASS_READ_CHUNK_CAP_PENDING`。
- 目的：诊断队列按需增长，容量不得直接触发巨额预分配。

## 前置条件与复测

使用归档对应的源码；工具、依赖准备、完整快照验证与复测入口见 [整体验证](../integrated-verification/README.md)。
在仓库根目录执行 `python3 <本用例父目录>/integrated-verification/replay/run.py <仓库根目录> socket-resource-limits`。

实际命令：

- `cargo test --offline --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime bounded_retention -- --nocapture`

## 预期与实际

- 预期：usize::MAX 保留数量配512字节预算，构造不panic；记录两事件后仅保留第二个并记录一次淘汰。
- 预期：现有非零数量/字节预算和淘汰语义保持。
- 实际：RED 构造 capacity overflow；GREEN bounded_retention 三项通过。
- 实际：read_chunk_bytes 的绝对上限未实施，仍需用户选择数值；不把该P1子项记为PASS。

## 资源、步骤与证据

`runs/` 保留本次各阶段实际测试源码、配置、日志与原执行时间；各阶段 README/命令 JSON 解释对应步骤。
最新状态以整体验证快照及 `outputs/validation/review-fixes/` 为准；此前失败与修复中间态不覆盖、不冒充最终通过。
回归使用的文件型 fixture 已包含在全量源码快照；代码生成的输入以实际测试源码为可重复向量。
动态 UUID/epoch 使用测试内的真实值做逐字段比较，不重建虚假的“实际请求”文件。

- huge_read_allocation：未执行危险巨额 vec 分配；现有无上限校验只做静态/非分配探针。
- binary_and_ui：N/A，内部队列逻辑，无网络/界面。

派生自 TASK-20260905-004 / final-verification / `docs/testing/evidence/2026-09-06/TASK-20260905-004/final-verification`；保留父用例原有行为，新增本节反例，父证据不改写。
