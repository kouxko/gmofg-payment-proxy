# android-active-profile

- 任务：TASK-20260906-001；执行日期：2026-09-06（Asia/Shanghai）。
- 结果：`LOCAL_PASS_DEVICE_NOT_RUN`。
- 目的：保存 Profile 漂移不隐式应用；合法 LAN 更新只改变冻结路由的 proxy_host。

## 前置条件与复测

使用归档对应的源码；工具、依赖准备、完整快照验证与复测入口见 [整体验证](../integrated-verification/README.md)。
在仓库根目录执行 `python3 <本用例父目录>/integrated-verification/replay/run.py <仓库根目录> android-active-profile`。

实际命令：

- `cargo test --offline --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure manual_recovery -- --nocapture`

## 预期与实际

- 预期：目标应用或 100% 丢包保存后 status 不发送 apply_active/apply。
- 预期：纯 LAN 地址变化保留冻结 IP、原始地址/端口、Listener、proxy_port、Profile 指纹、route_count、owner epoch；仅改 proxy_host 并重算 route fingerprint。
- 预期：失活、过期租约、serial/epoch 不匹配仍等待显式操作。
- 实际：RED 两项请求列表非空失败；修复后 manual_recovery 7 项通过。
- 实际：冻结路由补强回归 1 项通过；整体验证中当前测试再次通过。

## 资源、步骤与证据

`runs/` 保留本次各阶段实际测试源码、配置、日志与原执行时间；各阶段 README/命令 JSON 解释对应步骤。
最新状态以整体验证快照及 `outputs/validation/review-fixes/` 为准；此前失败与修复中间态不覆盖、不冒充最终通过。
回归使用的文件型 fixture 已包含在全量源码快照；代码生成的输入以实际测试源码为可重复向量。
动态 UUID/epoch 使用测试内的真实值做逐字段比较，不重建虚假的“实际请求”文件。

- android_device：无真实设备/TUN/跨设备 LAN 切换测试；使用 RecoveryRunner/模拟控制应答。
- ui：不涉及本用例 UI。
- binary_frames：控制请求的字段级逻辑测试，无真实网络帧；输入、expected 与比较见实际 test source。

派生自 TASK-20260905-001 / MANUAL-RECOVERY-001 / `docs/testing/evidence/2026-09-05/TASK-20260905-001/manual-recovery`；保留父用例原有行为，新增本节反例，父证据不改写。

Android 初次 RED 经 tee 管道返回的 shell 退出码不能代表 Cargo 成功；原始输出明确为 FAILED，按 2 项失败记录。
