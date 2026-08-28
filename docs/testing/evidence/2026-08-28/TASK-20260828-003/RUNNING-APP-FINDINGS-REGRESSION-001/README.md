# RUNNING-APP-FINDINGS-REGRESSION-001

- 来源：派生自 `TASK-20260828-001 / RUNNING-APP-REPLAY-001`。
- 目的：验证真实服务器响应可生成安全 Mock 草稿，以及外部 Workspace 提交会刷新当前入口、顶栏和规则能力。
- Mock 草稿：真实 201 JSON 响应 fixture 含 `Content-Length`、连接类 Header 和普通 Header；草稿保留状态、正文与普通 Header，但不保存运行管线托管的长度 Header。
- Workspace 刷新：使用生产 `snapshot_required` 事件验证当前 Workspace 0→2→0，另用 `workspace_changed` 覆盖旧事件路径；先失效请求代次，再按集合与当前 Workspace 顺序刷新，迟到响应不能回写。
- Application 事件：仅在 Environment commit 成功后发布一次；事务前失败和回滚不发布。
- 实际：Mock focused 4/4、前端全量 681/681、Application 484 项、MCP 生命周期 6/6、严格 Clippy、格式、类型、架构和源码大小门禁均通过。
- 运行中 App：HTTP 与 Socket 模拟 Server 数据平面、抓包、连接状态和 0→2→0 顶栏刷新完成重放；额外的 Mock 按钮人工探索按用户结论不再作为阻断项。
- 结果：`PASS_WITH_NOT_RUN`，未继续执行该额外人工按钮探索。
