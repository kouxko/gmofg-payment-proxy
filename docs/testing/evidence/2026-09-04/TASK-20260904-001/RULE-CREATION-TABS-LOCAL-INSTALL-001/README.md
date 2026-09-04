# RULE-CREATION-TABS-LOCAL-INSTALL-001

- 目的：验证新建规则三段式 Tab 的最终布局、顶部操作区、基本信息字段对齐与本机 macOS 安装结果。
- 派生自：`TASK-20260904-001 / RULE-CREATION-TABS-001 / docs/testing/evidence/2026-09-04/TASK-20260904-001/RULE-CREATION-TABS-001/`。
- 被测基线：`40d273ce3fd2a85b8b8e8674f041e0fb5804c17f` 加本任务工作区修改。
- 执行时间：`2026-09-04 10:32:45 ～ 10:52:02 +08:00`。
- 环境：macOS arm64；Deno `2.9.6`；Vitest `4.1.11`；Next.js `16.2.12`；Tauri release App `1.0.0`。

## 步骤、预期与实际

1. 打开规则页面并进入新建规则。
   - 预期：默认显示“基本信息 / 匹配条件 / 执行动作”；保存规则位于顶部取消旁边，底部无重复保存按钮。
   - 实际：自动化与安装 App 可访问性树、实时截图观察均 PASS；信息不完整时顶部保存按钮保持禁用。
2. 不选择 Listener 或处理阶段。
   - 预期：“说明”立即显示；条件和动作 Tab 显示明确的前置提示。
   - 实际：说明输入框立即可见，HTTP Listener/阶段选择后先前输入继续保留；Socket 阶段不显示未持久化的 HTTP 说明，PASS。
3. 检查基本信息布局。
   - 预期：启用开关与阶段内优先级输入控件中心线对齐；左右规则列表和创建编辑器顶栏使用相同内边距并水平对齐。
   - 实际：最终安装 App 实时截图确认两组布局均对齐，PASS。
4. 运行定向规则测试、完整 UI 合同、类型检查、Lint、生产构建和差异检查。
   - 实际：定向 15/15、UI 合同 303/303、typecheck、串行 lint、Next 13 个静态路由、`git diff --check` 全部 PASS。一次并行 Lint 因 Vitest 临时配置文件清理竞态失败，串行复跑 PASS。
5. 构建、签名并安装 `/Applications/Intercept Proxy.app`。
   - 实际：bundle id `com.interceptproxy.desktop`，版本 `1.0.0`；严格签名校验 PASS；构建与安装可执行文件 SHA-256 一致，为 `21de419b8a46abe734bdfc73661686224af93401e7b6fd1da6ff494a294dc54d`；最终进程 PID `72494`。

## 判定

`PASS`。三段式 Tab、顶部保存/取消、说明首显、两处视觉对齐、保存禁用合同、正式构建、签名、安装和实际界面均完成验证。

## 安装与恢复

- 当前安装：`/Applications/Intercept Proxy.app`。
- 本任务最终替换前版本：`/Users/codin/.Trash/Intercept Proxy.app.codex-old-20260904-105106`，可恢复。
- 本轮此前中间版本另有可恢复备份：`/Users/codin/.Trash/Intercept Proxy.app.codex-old-20260904-103628`、`/Users/codin/.Trash/Intercept Proxy.app.codex-old-20260904-104457`。

## 不适用项

- 规则 Runtime、Rust 规则模型、持久化、协议报文和真机：`N/A`，本任务只改变前端创建态布局。
- 截图文件：`N/A`，本轮通过 Computer Use 返回的安装 App 实时截图完成视觉验收，未生成独立本地图片文件。
- CI、push、发布：`N/A`，不在用户授权范围。
- 独立整体对抗审查：`N/A`，低优先级局部 UI 变更；主 Agent 完成差异与安装界面复核。

复测命令与结果摘要见 `outputs/validation-summary.txt`。
