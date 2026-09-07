# RULE-CREATION-TABS-001

- 目的：验证规则页面的新建规则恢复“基本信息 / 匹配条件 / 执行动作”三段式 Tab，并保持当前单条件、单动作和保存校验。
- 被测基线：`afe3dce9aa56e2c10fc5f9936e3ea827bba7ec9b` 加本任务工作区修改。
- 执行时间：`2026-09-04 10:18:13 ～ 10:21:14 +08:00`
- 环境：macOS arm64；Deno `2.9.6`；Vitest `4.1.11`；Next.js `16.2.12`。

## 步骤、预期与实际

1. 进入规则页面并点击“新建规则”。
   - 预期：显示“基本信息 / 匹配条件 / 执行动作”三个 Tab，默认选择“基本信息”。
   - 实际：前端交互测试逐项 PASS。
2. 未选择 Listener 时切到“匹配条件”。
   - 预期：明确提示先在基本信息选择 Listener，不生成默认能力。
   - 实际：显示“请先在基本信息中选择 Listener。”，PASS。
3. 选择 HTTP Listener 和 `Proxy → Server`，填写名称、优先级和启用状态，依次切换三个 Tab。
   - 预期：条件与动作分别显示在对应 Tab；切回基本信息后元数据保持；保存仍由完整元数据、条件和动作共同控制。
   - 实际：交互测试 PASS；已有规则编辑继续连续显示条件与动作且没有创建态 Tab。
4. 执行定向规则测试、完整前端 UI 合同、TypeScript、ESLint、Next production build 和 diff 检查。
   - 实际：定向 15/15、UI 合同 303/303、typecheck、lint、Next 13 个静态路由、`git diff --check` 全部 PASS。
5. 执行仓库源码尺寸门禁。
   - 实际：本次三个修改源码/测试文件分别为 97、246、209 行，均低于 500 行；全仓门禁被 7 个与本任务无关的既有超限文件阻断。

## 判定

`PASS_WITH_KNOWN_UNRELATED_SOURCE_SIZE_FAILURE_AND_RUNNING_APP_VISUAL_NOT_RUN`。三段式 Tab、未就绪提示、
状态保持、已有编辑态隔离和 production frontend build 均已验证。未构建或安装新的 Tauri 桌面包，因此
运行中桌面 App 的视觉观感未验收。

## 不适用项

- 规则 Runtime、Rust、持久化、协议报文和真机：`N/A`，本任务只改变创建态前端展示结构。
- 截图：`N/A`，本轮未启动当前源码的桌面 App；自动化保存了可访问性 Tab 与关键交互结果。
- CI、push、发布：`N/A`，不在用户授权范围。
- 独立整体对抗审查：`N/A`，低优先级局部 UI 变更；主 Agent 已完成差异复核。

复测命令与原始结果摘要见 `outputs/validation-summary.txt`。
