# DB-SCHEMA-100-001

- 目的：验证数据库版本 `100` 是产品 `1.00` 正式兼容基线；合法单行 `<100` 数据库被原子清理并重建，`=100` 保留，未来版本和异常标记拒绝且不改写。
- 原始现象：默认 Release App 因数据库版本 `21` 与当时程序版本不一致而在 Tauri setup 阶段退出。
- 实际修复：启动状态机在 SQLite 写锁内重新判断 empty/current/pre-1.00；清理统一使用 `BEGIN IMMEDIATE`，事务外关闭并最终恢复外键，失败时回滚全部 DROP；版本已被并发实例更新为 `100` 时不再重复清理。
- 边界覆盖：版本 `19/20/21/99`、版本 `100` 保留、版本 `101` 拒绝、空/错误 singleton/多行/损坏 marker、仅含 View 的无 marker 数据库、已提交 WAL 数据、带 `ON DELETE RESTRICT` 的父子表、清理失败回滚、两个实例的延迟重检。
- 自动化结果：Infrastructure 648 项、归档 24 项、导出 7 项、导入 token 8 项、Host 30 项全部通过；严格 Rust 静态检查、Rust 格式、TypeScript 类型检查、架构边界和源码尺寸门禁通过。
- 真实 App：当前源码的 macOS Release App 构建并启动成功；两秒存活检查通过；数据库标记为 `100`，保留 1 个 Workspace 和当前选择；代理端口 `8765`、MCP IPv4/IPv6 `17653` 正常监听。
- UI：Computer Use 读取到完整 Workspace 管理页，显示默认 Workspace、1 个 Listener、0 个已启用 Listener、版本 1；截图见 `outputs/workspace-ui.jpeg`。
- 对抗审查：依次发现并修复 reset 并发二次清理、View-only 空库误判、空库初始化竞态和外键约束清理四个问题；最终 P0/P1/P2 均为 0。
- 不适用：本任务未定义 `100 -> 101+` 的业务迁移，因此不生成虚构迁移；Windows 构建由随后触发的 CI 单独记录。
- 结果：`PASS`。
