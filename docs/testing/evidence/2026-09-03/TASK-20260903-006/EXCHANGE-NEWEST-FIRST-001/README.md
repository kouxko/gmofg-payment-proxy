# EXCHANGE-NEWEST-FIRST-001

- 目的：验证抓包运行记录在分页前按连接建立顺序从新到旧返回，第一页顶部始终是最新连接。
- 被测基线：`d2346ca`
- 执行时间：2026-09-03 15:40:30 ～ 15:45:54 +08:00
- 环境：macOS arm64；Rust/Cargo；Deno/Vitest；Tauri 2；本机 `/Applications/Intercept Proxy.app`。

## 输入、步骤与结果

1. 在 `ExchangeObservationStore` 中依次写入 `exchange-a`、`exchange-b`、`exchange-c`，以 page_size=2 查询两页。
   - 预期：第一页 `exchange-c, exchange-b`；第二页 `exchange-a`。
   - 实际：逐项完全一致，定向 Rust 测试 1/1 PASS。
2. 执行 ExchangeObservationStore 模块全部测试。
   - 实际：9/9 PASS；事件追加、容量淘汰、计数和 Workspace 隔离回归均通过。
3. 执行抓包 model/list/view 三个前端测试文件。
   - 实际：3 个文件、10/10 PASS；表头显示“建立时间（最新优先）”。
4. 执行 `deno task typecheck`、`deno task lint`、`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` 和 `git diff --check`。
   - 实际：全部退出码 0。
5. 执行 `deno task tauri build --bundles app`，安装、ad-hoc 签名并启动正式包。
   - 实际：Next.js 13 个静态页面、Rust release 和 macOS `.app` 均成功；严格签名校验通过。
   - 安装路径：`/Applications/Intercept Proxy.app`
   - 旧包备份：`/Users/codin/.Trash/Intercept Proxy.app.codex-old-20260903-154545`
   - 可执行文件 SHA-256：`622d5015822d9c689d5fcdaa379435361861b5e7aea2fdbbd6c65f98405f4f44`
   - 运行 PID：`61305`
6. 打开已安装应用的“实时抓包”。
   - 实际：可访问性树显示列标题“建立时间（最新优先）”。

## 判定

PASS。全局倒序在 Rust 分页前完成；前端未进行局部重排，单个 Exchange 的事件顺序保持不变。

## 不适用项

- 实际多行抓包截图：N/A；当前 Workspace 为 0 条记录。全局分页顺序由确定性 Rust 测试验证，安装界面验证表头。
- 网络报文、远端 Server、A920MAX：N/A；本任务只改变观测查询投影顺序。
- CI、push、发布：N/A；用户要求本地修改、安装与提交。
- 对抗审查：N/A；按用户在当前连续交付中的明确要求跳过。

复测命令见 `outputs/test-summary.txt`。
