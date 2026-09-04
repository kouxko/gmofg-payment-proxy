# HTTP-ACTION-PARAMETER-FORMS-001

- 任务：`TASK-20260904-003`
- 派生自：`TASK-20260904-002 / HTTP-RULE-EDITOR-SIMPLIFICATION-001 / docs/testing/evidence/2026-09-04/TASK-20260904-002/HTTP-RULE-EDITOR-SIMPLIFICATION-001/`
- 目的：验证规则页 HTTP 动作不再要求手写 JSON，有参动作显示中文字段和单位，无参动作隐藏参数区域，并保持 Rust 动作草稿合同。
- 环境：macOS 27.0（26A5425a）、arm64、Rust 1.98.0、Deno 2.9.6。
- 被测对象：当前工作区规则编辑器源码、生成绑定以及 `/Applications/Intercept Proxy.app`。
- 执行时间：`2026-09-04 14:02:28 +08:00` 至 `2026-09-04 14:26:46 +08:00`。
- 结果：`PASS_WITH_INSTALLED_UI_CONTENT_AUTOMATION_NOT_RUN_AND_KNOWN_UNRELATED_SOURCE_SIZE_FAILURE`。

## 输入与预期

- 固定延迟显示“延迟时间（毫秒）”，限速显示“速率（B/s）”和“分块大小（字节）”。
- 抖动和丢弃方式使用中文下拉；超时、状态码、间歇通断、长度偏移、截断与写入中断均显示各自字段。
- 动作参数区域不再出现“动作参数 JSON”。
- 切换动作后清空旧参数，必填字段未完整时保存按钮禁用。
- `disconnect_before_upstream` 不显示参数区域，并以 `parameters_json: null` 交给 Rust。
- 限速和间歇通断的方向继续取 Rust stage capability，不提供手工方向选择。

## 执行步骤与结果

1. RED：先修改规则编辑器回归，旧实现仍显示“动作参数 JSON”，延迟字段不存在；定向测试得到 2 项预期失败。
2. GREEN：新增动作参数草稿/序列化与表单组件；规则编辑器回归和参数合同回归共 22/22 PASS。
3. 完整前端测试 65 个文件、566/566 PASS；typecheck、lint、Next production build PASS，生成 13 个静态页面。
4. 生成绑定 freshness/determinism、完整架构扫描、Frontend Rust-only 边界和 `git diff --check` PASS。
5. Application 现有动作草稿工厂定向测试 1/1 PASS。
6. 源码尺寸门禁仍被 7 个既有无关文件阻断；本任务新增/修改的生产文件均低于 500 行。
7. `deno task tauri build --bundles app` PASS；bundle 经 ad-hoc deep/strict 签名校验后覆盖安装。
8. 最终 App 位于 `/Applications/Intercept Proxy.app`，bundle id `com.interceptproxy.desktop`、版本 `1.0.0`，构建与安装可执行文件 SHA-256 均为 `9c3af992e43472c095bff14e87a747ca0310483bb6618376d8f54c8341967d23`，PID `62783`。
9. Computer Use 能识别已安装 App 的原生窗口，但当前 macOS Space 未暴露 WebView 内容，故安装版点击式内容检查记为 NOT_RUN；最终产物对应源码的 jsdom 交互回归已覆盖参数字段、切换和无参隐藏。

## 不适用项与剩余边界

- 真实弱网传输与故障数据面：N/A；本次未改变 Rust 动作运行语义，使用现有 Application factory 测试和前端序列化合同验证。
- UI 截图：N/A；当前桌面 Space 无法显示该 App 的 WebView 内容，不保存无关截图作为证据。
- Windows、Android、CI、push、发布：N/A；不在本次本地实现与安装范围。
- 工作区并行修改：`src-tauri/crates/infrastructure/src/adapters/certificates_tests/support_and_failures.rs` 为本任务之外的修改，未编辑、未归档为本任务成果。

复测命令见 `replay/commands.txt`，结果摘要见 `outputs/validation-summary.txt`。
