# RULE-SELECT-OVERFLOW-001

- 目的：验证规则新建/编辑区域所有 Select 在窄双栏布局中不因长已选文本换行或越出固定高度边框。
- 派生自：`TASK-20260904-001 / RULE-CREATION-TABS-LOCAL-INSTALL-001 / docs/testing/evidence/2026-09-04/TASK-20260904-001/RULE-CREATION-TABS-LOCAL-INSTALL-001/`。
- 被测基线：`40d273ce3fd2a85b8b8e8674f041e0fb5804c17f` 加本任务工作区修改。
- 执行时间：`2026-09-04 10:59:09 ～ 11:04:55 +08:00`。
- 环境：macOS arm64；Deno `2.9.6`；Vitest `4.1.11`；Next.js `16.2.12`；Tauri release App `1.0.0`。

## 步骤、预期与实际

1. 静态检查规则目录全部 `Select.Trigger` 与 `Select.Value`。
   - 预期：触发器固定 `h-10/min-h-10`、`min-w-0/overflow-hidden`；值使用 `min-w-0 flex-1 truncate whitespace-nowrap`；Indicator 使用 `shrink-0`。
   - 实际：基本信息 Listener/阶段、匹配条件来源/HTTP 字段/操作符/Schema 路径/类型/谓词、执行动作来源/HTTP 类型/Schema 路径/类型/动作/通用动作全部统一，PASS。
2. 自动化选择 `/KCCI_01/*/kid` 长 Schema 条件和动作路径。
   - 预期：按钮保留完整可访问名称，视觉值单行裁切，保存所用 path 不变。
   - 实际：条件与动作两个触发器均满足裁切类合同，定向规则测试 15/15 PASS。
3. 在正式安装 App 中选择 Payment DLL、`Proxy → App` 和 Document 来源，分别选择超长条件路径 `/GICC_01/*/tables/*/communication_id` 与动作路径 `/GICC_01/*/tables/*/merchant_type_code`。
   - 预期：已选文本只显示一行省略号，不越出边框；手动路径保留完整值。
   - 实际：实时可访问性树保留完整值，实时截图确认两个 Schema Select 均为单行省略且边框无溢出；其它可见 Select 高度稳定，PASS。
4. 执行完整 UI 合同、类型检查、Lint、生产构建、签名与安装检查。
   - 实际：UI 合同 303/303、typecheck、lint、`git diff --check`、Tauri macOS App 构建和严格签名全部 PASS。

## 判定

`PASS`。规则页所有 Select 已统一单行裁切合同，长 Schema 条件/动作路径在最终安装 App 中均未溢出。

## 安装与恢复

- 当前安装：`/Applications/Intercept Proxy.app`。
- bundle id：`com.interceptproxy.desktop`；版本：`1.0.0`。
- 可执行文件 SHA-256：`190b7c16f7f875c7e32240684b9bf2df021be99527576bd7c6b0cdadc2f3c7aa`；最终 PID：`87008`。
- 最终替换前版本：`/Users/codin/.Trash/Intercept Proxy.app.codex-old-20260904-110203`，可恢复。

## 不适用项

- 规则 Runtime、Rust 模型、持久化、协议报文和真机：`N/A`，只改变 Select 文本布局。
- 截图文件：`N/A`，通过 Computer Use 返回的安装 App 实时截图验收，未生成独立本地图片文件。
- CI、push、发布：`N/A`，不在用户授权范围。

复测摘要见 `outputs/validation-summary.txt`。
