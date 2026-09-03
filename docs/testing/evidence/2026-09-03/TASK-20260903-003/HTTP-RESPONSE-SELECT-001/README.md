# HTTP-RESPONSE-SELECT-001

- 目的：验证 HTTP Listener 使用单一下拉框表达动态转发、固定 Server 和本机应答三种互斥路由。
- 被测基线：`f58e181`
- 执行时间：2026-09-03 15:26:55 ～ 15:31:46 +08:00
- 环境：macOS arm64；Deno；Vitest；Tauri 2；本机 `/Applications/Intercept Proxy.app`。

## 步骤与结果

1. 执行 `deno task test src/features/listeners/listeners-view.test.tsx src/features/listeners/listeners-view.certificates.test.tsx`。
   - 实际：2 个测试文件、32 个测试全部通过。
   - 覆盖：三选项切换、Local topology 保存、固定 Server URL 展示、证书草稿清理与持久化证书保留。
2. 执行 `deno task typecheck`、`deno task lint` 和 `git diff --check`。
   - 实际：全部退出码 0。
3. 执行 `deno task tauri build --bundles app`。
   - 实际：Next.js 13 个静态页面生成成功；Rust release 和 macOS `.app` 生成成功。
4. 将旧应用移动到废纸篓备份，安装新包并执行 ad-hoc 签名，然后运行严格签名校验。
   - 安装路径：`/Applications/Intercept Proxy.app`
   - 旧包备份：`/Users/codin/.Trash/Intercept Proxy.app.codex-old-20260903-153100`
   - 可执行文件 SHA-256：`6dc4a5b5dc00c8274ac5f106041691c48585e0d603b2bdf0cee252c476fb0c23`
   - 运行 PID：`35652`
   - 实际：`codesign --verify --deep --strict` 通过。
5. 打开已安装应用的“代理入口配置”，展开“HTTP 响应方式”。
   - 实际：可访问性树显示三个选项：“按原请求目标转发”“转发到固定 Server”“本机应答”。
   - 实际：页面不存在“转发到固定 Server”独立 switch。

## 判定

PASS。三个选项与既有 topology/fixed_server 状态映射通过自动化测试，正式包已安装并在实际界面显示。

## 不适用项

- 协议报文、远端 Server、A920MAX：N/A，本任务只改变本地配置 UI，不改变运行时或网络合同。
- 截图文件：N/A，本次使用已安装应用的实时可访问性树确认选项与控件类型。
- CI、push、发布：N/A，用户要求本地测试、安装与提交。
- 对抗审查：N/A，低风险局部 UI 调整，且用户明确要求跳过。

复测命令见 `outputs/test-summary.txt`。
