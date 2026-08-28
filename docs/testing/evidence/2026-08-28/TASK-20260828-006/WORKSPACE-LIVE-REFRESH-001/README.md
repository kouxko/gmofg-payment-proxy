# WORKSPACE-LIVE-REFRESH-001

- 来源：派生自 `TASK-20260828-003 / RUNNING-APP-FINDINGS-REGRESSION-001`。
- 目的：验证 Workspace 管理页在外部 `snapshot_required` 后刷新列表和详情，保留未保存名称，但不让旧详情遮蔽新 Listener 数量与 revision。
- 自动化：真实 `BootstrapProvider + WorkspacesView + Channel` 组件覆盖成功刷新、未保存名称合并、刷新失败旧快照标识和首次读取失败；聚焦 18/18、前端全量 61 文件 531 项、类型、lint、边界、源码大小和正式 App 构建均通过。
- 运行中 App：重启后最新正式 App 正常渲染；隔离 Workspace 初始 revision 16，外部配置周期后列表与详情无需切页同步显示 revision 18、Listener 2、启用 0；顶部显示全部入口停止、活动 0。
- 派生重放：同一隔离环境在重启前已完成 HTTP 201、Socket 逐字节回显、服务器响应生成未保存禁用 Mock 草稿、诊断日志最新在上；对应截图随本用例保存。
- 本次未执行：重启后的 UI 点击控制通道在任何动作时关闭，因此没有再次启动 Listener；MCP 配置接口按合同不会代替用户启动运行态。本次没有把未运行的数据面报告为成功。
- 对抗审查：草稿遮蔽、刷新失败陈旧标识、首次读取失败文案与空态均已修复；最终 P0/P1/P2 为 0。
- 结果：`PASS_WITH_NOT_RUN`。TASK006 的刷新合同通过；重启后的重复数据面启动保持未执行，父用例的既有数据面结果继续作为派生参考。
