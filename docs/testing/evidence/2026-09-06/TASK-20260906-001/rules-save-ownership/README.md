# rules-save-ownership

- 任务：TASK-20260906-001；执行日期：2026-09-06（Asia/Shanghai）。
- 结果：`COMPONENT_PASS_NATIVE_UI_NOT_RUN`。
- 目的：同次保存仅一次提交，旧物化任务和迟到写回不越过编辑会话。

## 前置条件与复测

使用归档对应的源码；工具、依赖准备、完整快照验证与复测入口见 [整体验证](../integrated-verification/README.md)。
在仓库根目录执行 `python3 <本用例父目录>/integrated-verification/replay/run.py <仓库根目录> rules-save-ownership`。

实际命令：

- `deno task test src/features/rules --reporter=verbose`

## 预期与实际

- 预期：物化和保存请求完成前重复 Save 不重复创建。
- 预期：切换规则后旧保存不覆盖当前编辑器；失败提示仍显示并可重试。
- 预期：New/Cancel/同身份抓包草稿替换会卸载旧会话，旧物化不能调用保存。
- 实际：初始 RED 7 pass / 2 fail，初次修复 41 pass，取消/错误提示补强 43 pass。
- 实际：复审的有效 red-final 同身份替换两项失败；最终 5 文件45项通过。
- 实际：最新全前端 73 文件550项通过。

## 资源、步骤与证据

`runs/` 保留本次各阶段实际测试源码、配置、日志与原执行时间；各阶段 README/命令 JSON 解释对应步骤。
最新状态以整体验证快照及 `outputs/validation/review-fixes/` 为准；此前失败与修复中间态不覆盖、不冒充最终通过。
回归使用的文件型 fixture 已包含在全量源码快照；代码生成的输入以实际测试源码为可重复向量。
动态 UUID/epoch 使用测试内的真实值做逐字段比较，不重建虚假的“实际请求”文件。

- native_app：未启动原生 App 做点击或截图；真实组件测试按可访问名操作并断言状态。
- binary_frames：N/A，UI 与 IPC mocks，不是协议链路测试。

派生自 TASK-20260905-004 / final-verification / `docs/testing/evidence/2026-09-06/TASK-20260905-004/final-verification`；保留父用例原有行为，新增本节反例，父证据不改写。

复审的 `runs/review-session-fix/red/` 中 repeated New 优先级输入初版设置错误，仅记 HARNESS_ERROR；`red-final/` 修正设置后两条真实回归均到达产品行为并失败，才作为 RED。
