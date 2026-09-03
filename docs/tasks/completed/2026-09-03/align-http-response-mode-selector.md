# TASK-20260903-003：统一 HTTP 与 Socket 响应方式选择器

- 任务 ID：TASK-20260903-003
- 状态：已完成
- 任务日期：2026-09-03
- 创建时间：2026-09-03 15:15:49 +08:00
- 开始时间：2026-09-03 15:15:49 +08:00
- 最后更新时间：2026-09-03 15:32:32 +08:00
- 完成时间：2026-09-03 15:32:32 +08:00
- 创建路径：`docs/tasks/pending/2026-09-03/align-http-response-mode-selector.md`
- 归档路径：`docs/tasks/completed/2026-09-03/align-http-response-mode-selector.md`
- 关键词：`HTTP`、`Socket`、`响应方式`、`Select`、`LocalServer`、`UI`
- 任务优先级：低（仅调整已落地 topology 的 UI 选择方式，不改变公共合同或运行时）

## 背景与目标

HTTP 已支持 RemoteServer/LocalServer，但当前界面使用单独的“使用 Local HTTP Server”开关；Socket
使用“1. 响应方式”下拉选择“转发到上游/本机应答”。用户指出两种数据平面的配置方式不一致，HTTP
开关不易理解。

目标：HTTP 使用单一“响应方式”下拉表达三种互斥路由；选择结果继续映射现有
`HttpTopology` 与 `fixed_server`，不修改持久化、MCP、Runtime 或规则合同。

## 范围、不在范围与确认记录

- 将 HTTP 卡片标题改为“1. 响应方式”，说明与 Socket 对齐。
- 使用 Select 提供“按原请求目标转发”“转发到固定 Server”和“本机应答”。
- 移除单独的“转发到固定 Server”开关；选择固定 Server 时显示现有 Server URL 配置。
- 更新组件测试、用户操作文档、测试证据并重新构建安装本地 App。
- 不修改 topology 数据结构、旧配置迁移、LocalServer 运行时、Mock 合同或 Socket UI。
- 2026-09-03：用户以 HTTP 当前界面和 Socket 截图确认，要求 HTTP 像 Socket 一样添加下拉，当前配置不好理解。
- 2026-09-03：首版实现为“转发到上游/本机应答”下拉，并保留“转发到固定 Server”开关。
- 2026-09-03：用户进一步明确要求把“按原请求目标转发”也改成下拉选项；原验收项 2、4 失效，改为三个互斥选项且不保留独立开关。
- 未确认事项：零。具体选项沿用 Socket 当前权威文案，需求就绪并进入实现。

## 方案与验收

- 最小改动：用单个 Select 直接映射 `remote_server + fixed_server null`、`remote_server + fixed_server`、`local_server` 三种既有状态。
- 最优设计：抽取跨协议响应方式组件。当前两个面板还包含不同能力和布局，抽取会扩大范围且无实际复用收益。
- 采用最小改动；不增加新抽象或依赖。

验收标准：

1. HTTP 显示“1. 响应方式”和下拉框，不再显示 LocalServer Switch。
2. 下拉包含“按原请求目标转发”“转发到固定 Server”和“本机应答”，不再显示独立固定 Server 开关。
3. 三个选项分别保存动态 `remote_server`、固定 `remote_server` 和 `local_server`。
4. 选择“转发到固定 Server”后显示现有固定 Server URL 配置。
5. 相关 UI 测试、typecheck、lint、正式 macOS App 构建与本机安装验证通过。

## 小任务、文档、测试与审查

| ID | 内容 | 状态 | 验收 |
| --- | --- | --- | --- |
| T01 | HTTP 响应方式 Select 与 topology 映射 | 已完成 | 三个选项双向切换 |
| T02 | UI 回归与文档 | 已完成 | 相关测试与静态检查通过 |
| T03 | 构建、安装、证据与归档 | 已完成 | `/Applications` 新包运行 |

- 测试：ListenersView 断言三个下拉选项、三种 topology 映射和固定 Server UI。
- 文档：用户操作说明同步下拉文案。
- 对抗审查：低优先级局部 UI 一致性修改，且用户要求加快并跳过额外审查；不执行。

## 实施、测试与完成总结

- `RequestRoutingCard` 使用单一 Select 映射动态目标、固定 Server、本机应答三种既有状态，移除独立固定 Server Switch。
- 选择固定 Server 时继续展示既有 URL 与上游 TLS/mTLS 配置；切回动态目标或本机应答时清除固定目标。
- 更新 Listener UI 与证书生命周期测试，确认切换不误删持久化证书并正确清理未保存证书引用。
- 更新用户操作说明和 Listener 列表中的本机应答文案。
- 测试：ListenersView 与证书测试 2 文件 32 项 PASS；typecheck、lint、`git diff --check` PASS。
- 构建：正式 macOS App 构建 PASS；安装到 `/Applications/Intercept Proxy.app`，严格签名校验 PASS，PID `35652`。
- 实际 UI：下拉显示三个选项，旧固定 Server 开关不存在。
- 证据：[HTTP-RESPONSE-SELECT-001](../../../testing/evidence/2026-09-03/TASK-20260903-003/HTTP-RESPONSE-SELECT-001/README.md)。
- CI、push、发布：NOT_RUN；不在用户授权范围。
- 对抗审查：SKIPPED；低风险局部 UI 修改，按用户明确要求跳过。
- 完成结论：PASS。
