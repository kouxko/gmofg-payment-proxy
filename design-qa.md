# GMO-FG Payment Proxy UI QA

## 验收环境

- 日期：2026-07-28
- 浏览器：Microsoft Edge 150.0.4078.83
- 视口：1440×1024、1024×900
- 构建：Next.js 静态导出目录 `out/`，由本地静态服务加载
- 数据来源：注入 Rust ViewModel/IPC 模拟响应，仅用于浏览器视觉与交互验收
- 应用集成：Tauri/Rust 集成构建单独验证，不以浏览器模拟替代

## 页面覆盖

以下 8 个页面均在两个视口下完成真实 Edge 截图、页面标题检查和横向溢出检查：

1. 代理控制台
2. 实时抓包
3. 会话记录
4. 断点实验台
5. 拦截规则
6. 故障模拟
7. 证书管理
8. 系统设置

最终截图位于 `output/playwright/ui-audit-2026-07-28/production-final/`：

- `<viewport>-<route>.png`
- `contact-1440.png`
- `contact-1024.png`
- `19-pkcs12-before-after.png`
- `22-settings-reset-pending.png`
- `23-breakpoint-rust-actions-1024.png`
- `24-breakpoint-rust-action-list-1024.png`
- `v106-1024-{rules,faults,settings,breakpoints}.png`

## 交互覆盖

- 1024 宽度主导航 Drawer 可打开，并显示全部 8 个页面入口。
- 实时抓包可选择记录、加载 Rust 详情并切换请求详情 Tab。
- 会话记录仅在用户点击“查看完整报文”后调用 `session_get`；仅选择行不会加载 Payload。
- 断点实验台可切换有效报文 Tab，并调用 Rust 校验。
- 拦截规则可切换基本信息、匹配条件、执行动作并提交保存意图。
- 系统设置可切换分类、修改容量值并提交保存意图。
- 代理控制台可提交重启代理意图。
- AlertDialog 提交期间“取消”和确认按钮均禁用；按下 Escape 后弹窗仍保持，Rust
  响应成功后才关闭。
- PKCS12 Modal、设置 AlertDialog、会话 Drawer 的取消/关闭操作均位于 Footer
  安全边距内。
- 侧栏普通项、选中项及“关于”的图标、文字、背景共用同一水平中心轴。
- `src/test/overlay-contracts.test.tsx` 使用真实 HeroUI v3 组件自动验证 Modal、
  Drawer 的 Footer close slot，以及 AlertDialog 等待态、禁用态和 Escape 防误关。
- 生产 `RulesView` 自动验证 Header 解析待定时“保存规则”不可执行、Rust 最新解析
  返回后才保存新 Draft；动作类型草稿同样进入保存门禁，两次选择逆序完成时仅应用
  最新 Rust 结果。Header、Body 与状态码并发编辑使用最新动作合并，互不回滚；条件
  类型请求期间删除当前行或前置行后，迟到结果不会写入移位后的条件。
- 生产 `SettingsView` 自动验证真实恢复默认 AlertDialog 的等待态，以及 SAN 原始
  文本不经 TypeScript 拆分而直接提交 Rust。
- `BootstrapProvider` 使用请求代次隔离，旧快照不得覆盖更新的 Rust Channel 事件。
- 断点处理方式、标签和默认参数来自 Rust `available_actions`，请求与响应阶段动作集
  由 Rust 测试分别覆盖。

## 验收结果

- 16 个页面/视口组合均无横向溢出。
- v1.0.7 变更涉及的规则、故障、设置和断点页在 1024×900 重新验证，4/4
  页面标题存在且无横向溢出。
- 最终稳定静态服务回归：0 个 Console Error、0 个 Console Warning、0 个 Page Error。
- 1024 宽度使用紧凑顶栏、Drawer 导航及内部滚动；固定操作区保持可见。
- HeroUI Tabs、Drawer、Table、Form 与 Dialog 交互均可用。
- 页面底部不再展示无实际控制功能的内存占用状态条。
- 统一门禁当前通过：12 个 UI contract 文件 / 30 个测试、12 个前端测试文件 /
  30 个测试，以及 Rust workspace 118 个测试。

## 证据边界

- 本轮 Edge 验收证明静态导出 UI 的视觉、响应式布局、DOM 语义和模拟 IPC
  交互，不替代真实 Tauri 窗口、系统文件选择器、Keychain/DPAPI、证书链和网络代理验证。
- 键盘操作覆盖了导航、Dialog 和 Escape 等主路径，不等同于完整 WCAG
  辅助技术审计。

final result: passed
