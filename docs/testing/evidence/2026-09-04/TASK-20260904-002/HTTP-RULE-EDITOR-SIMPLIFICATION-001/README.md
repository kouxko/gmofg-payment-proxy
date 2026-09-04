# HTTP-RULE-EDITOR-SIMPLIFICATION-001

- 任务：`TASK-20260904-002`
- 目的：验证 HTTP 规则编辑器移除重复/无效能力、Method 使用有限候选、Document 条件候选按谓词能力过滤，以及抓包响应只生成 LocalHttpServer 下行 Body 替换规则。
- 环境：macOS 27.0（26A5425a）、arm64、Rust 1.98.0、Deno 2.9.6。
- 被测对象：当前工作区源码、生成绑定以及 `/Applications/Intercept Proxy.app`。
- 执行时间：`2026-09-04 11:53:10 +08:00` 至 `2026-09-04 12:51:07 +08:00`。
- 结果：`PASS_WITH_INSTALLED_UI_AUTOMATION_NOT_RUN_AND_KNOWN_UNRELATED_SOURCE_SIZE_FAILURE`。

## 输入与预期

- HTTP 条件只提供 Method 和 Path；Method 值限定为 GET、POST、PUT、PATCH、DELETE。
- 动作来源不提供“通用”；HTTP 动作不提供 SetJsonField、SetHeader、MockResponse，保留动作以中文显示。
- Document Schema 条件路径和值类型只展示 `predicates` 非空的能力；Object/Array 容器仍可按 action capability 出现在动作路径。
- 抓包响应规则固定为 Proxy → App `ReplaceBodyText`，只复制配对 request-target 和 UTF-8 Body，不复制 status/Header，需配合 LocalHttpServer。
- 底层旧 Header/RecordMatch/SetJsonField/SetHeader/MockResponse 类型继续读取和运行，避免破坏既有持久化数据。

## 执行步骤与结果

1. RED：新增规则 UI 测试首先发现 `/GBRD_01/*` Object 节点仍出现在条件路径下拉；修复后条件路径/类型过滤、动作路径保留容器的测试 PASS。
2. RED：新增抓包草稿测试首先得到 `ProxyToUpstream`，而预期为 `ProxyToApp`；前端按钮测试也首先只找到旧“创建 Mock 草稿”文案。修复后 Rust 1/1、抓包与规则定向前端 17/17 PASS。
3. Application 全量 417/417 PASS，包含阶段能力、草稿 factory、旧类型兼容和新抓包 Body 替换回归。
4. 完整前端 64 文件 554/554 PASS；typecheck、lint、Next production build PASS，生成 13 个静态页面。
5. Rust fmt、workspace all-targets/all-features strict Clippy、架构扫描、生成绑定 freshness/determinism 与 `git diff --check` PASS。
6. 源码尺寸门禁仍只被 7 个既有无关文件阻断；本次触及的规则编辑器、抓包草稿与测试文件均低于 500 行。
7. `deno task tauri build --bundles app` PASS；bundle ad-hoc 签名并通过 deep/strict 校验。
8. 最终 App 安装到 `/Applications/Intercept Proxy.app`；bundle id `com.interceptproxy.desktop`、版本 `1.0.0`，构建与安装可执行文件 SHA-256 均为 `ff21f80b323c901241db65459d705894b13b825d9b2e129e1b2c2d04dd56d824`，最终 PID `87690`。
9. Computer Use 状态确认 App 正在运行；对 `/Applications/Intercept Proxy.app` 的可访问性连接连续超时，故最终安装版点击式条件下拉检查记为 NOT_RUN。相同行为已由最终产物对应源码的 jsdom 交互回归覆盖。

## 不适用项与剩余边界

- 真实业务请求与 LocalHttpServer 数据面重放：N/A；本次没有发送真实交易，草稿 factory、Application 全量和 UI 交互已覆盖规则形状。
- Windows、Android、CI、push、发布：N/A；不在本次本地实现与安装范围。
- UI 截图：N/A；Computer Use 无法连接该 App 的可访问性界面，未伪造截图证据。
- 旧 MockResponse 规则：继续兼容读取和运行，但新建/编辑与抓包生成入口不再创建该动作。

复测命令和结果摘要见 `outputs/validation-summary.txt`。
