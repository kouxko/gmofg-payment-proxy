# Listener Draft 前端合同验证

- 任务：TASK-20260905-002 / F1
- 最终证据整理时间：2026-09-05 16:33:13 +08:00
- 被测 Git 基线：`c7a70d3ef63719ae3bb7f9b3e6dca8db995c6525`，加共享工作区当前未提交改动
- 范围：ListenerDraft 本地编辑、Rust copy/transition/validate 三接口、转换队列竞态、提交门禁、证书引用、TASK001 外部冲突与局部放弃回归。

## RED 与输入

`outputs/red.log` 保存先写的 transition hook 回归在实现前因模块不存在而失败的实际观察。`inputs/transition-cases.json` 保存竞态输入和逐项预期；`inputs/source-before/` 是实施前源文件快照。

## 执行窗口与结果

- 16:23:31 开始的正式全前端回归：`deno task test`，68 个文件、536 项全部通过，见 `outputs/full-frontend-test.log`。
- 16:25:06 开始的 Listener 回归：`deno task test src/features/listeners`，18 个文件、168 项全部通过，见 `outputs/listener-tests.log`。
- 16:26:40 和 16:27:03 因一次未引用 heredoc 中的 Markdown 反引号被 shell 错误解释，意外在本机重复执行 Listener 与全前端测试；两轮同样分别为 168/168 和 536/536 PASS，没有外部操作。被污染的 README 已整体重写，未把插值输出当作独立日志。
- 最终生命周期保护和测试拆分后，16:30:50 开始的受影响回归：6 个文件、72 项全部通过，见 `outputs/affected-tests-v2.log`。
- 16:33:13 增加“拓扑在途只禁用旧 Server 控件”回归后：3 个文件、39 项全部通过，见 `outputs/pending-controls-v4.log`。
- 最终 `deno task typecheck` 与 `deno task lint`：通过且 lint 为 0 error、0 warning，见 `outputs/typecheck-v4-final.log`、`outputs/lint-v4-final.log`。
- `deno run -A scripts/check-frontend-boundaries.mjs`：通过，见 `outputs/frontend-boundaries-v4-final.log`。
- 最终尺寸检查见 `outputs/source-file-sizes-v3.log`：只剩仓库原有 6 项债务；`listeners-view.tsx` 为 491 个 `wc` 行，`listeners-view.test.tsx` 从父任务约 525 行拆到 499 个 `wc` 行，新文件均低于 500 行。
- `git diff --check -- src/features/listeners .../frontend`：通过，无空白错误。

## 行为结论

结构意图只由生成的 Rust transition 接口执行。队列按 Rust 返回的中间 Draft 串行组合，仅发布当前 epoch 的最终 Draft；叶字段、选择、外部 authority、工作区、卸载和现有操作锁会同步失效旧批次与尾部。数据平面或 LocalResponder 拓扑切换期间，旧平面/旧 Server 控件不可提交不适用意图。转换在途时保存、启动、校验和 TLS 探测均被阻止，公共叶字段与快速结构选择保持可用。失败进入现有可见错误通道，队列停止且没有本地默认或回退转换。

证书收集仅携带非空且可达引用；apply/prune/commit/discard 与 TASK001 的逐 Listener 草稿冲突、局部放弃和 lease 清理回归全部通过。

## 资源与复测

- `resources/source-after/`：16:25 左右状态的快照，作为上一执行状态保留。
- `resources/source-after-v2/`：生命周期保护和测试拆分后的中间快照；校验值见 `outputs/source-snapshots-v2.sha256`。
- `resources/source-after-v3/`：尺寸拆分后仅删除一个测试空行的补充快照；校验值见 `outputs/source-snapshots-v3.sha256`。
- `resources/source-after-v4-final/`：最终冻结的 Listener 源码和测试快照；校验值见 `outputs/source-snapshots-v4-final.sha256`。
- `outputs/source-after.diff`：较早 tracked diff，保留为历史执行材料；最终新增/修改文件以 v4-final 快照为准。
- `replay/run.sh`：从仓库根目录复测完整 Listener、类型、lint、边界和尺寸。

## N/A

真实设备、网络报文、截图：本次是前端 DTO/并发状态合同，使用 jsdom 组件状态与 IPC mock 输入输出断言，不涉及真实网络或设备 UI。
