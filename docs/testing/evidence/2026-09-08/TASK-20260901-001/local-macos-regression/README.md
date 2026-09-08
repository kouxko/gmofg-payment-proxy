# 本机 macOS 回归验证

TASK-20260901-001；2026-09-08；本机回归 PASS，独立审查 APPROVE（P0/P1/P2=0）。父任务仍进行中。

用户要求“用本机进行测试验证回归”。本轮把原待补 HTTP 回放的执行主机改为本机 macOS，复用历史 HTTP Method/Header/path/JSON Body 命中、all/miss/非法 JSON，以及内置 ISO8583 Wasm match/miss/非法 Frame。没有修改产品行为或 read_chunk_bytes 上限。

## 当前结果

| 验证 | 结果与依据 |
| --- | --- |
| 静态与自动化 | 10 个 gate 全部退出 0；前端 73 文件 / 560 测试；Rust 45 suites / 1647 测试，0 failed / ignored。见 [质量结果](outputs/quality-results.json)及对应原始日志 |
| 源码稳定 | 测试开始的 1418 个源文件与结束时逐字节一致；归档状态收尾发生在该检查之后。见 [最终检查](outputs/source-stability-final.json) |
| 真实 App | 使用 9 月 7 日 Universal DMG 内 App，以只读挂载与独立 HOME 运行；1247 个构建输入与当前对应源文件一致，见 [产物来源](outputs/app-source-provenance.json) |
| HTTP | 7 例；6 个合法请求均得到 200、准确规则结果和上游实际响应；非法 JSON 在 Decode 失败，无 sent event、无上游请求 |
| Socket / Wasm | 3 例；0200 精确改为 0100，0400 全帧不变；上游仅收到这两帧，客户端响应逐字节相同；非法帧在 Decode 失败且没有发送 |
| 规则 | 5 条规则命中增量依次为 2/2/2/2/1；见 [最终 wire 结果](outputs/wire/summary.json) |
| 阶段与字节 | 逐项连接 Client 输入、Decode、Encode、sent、Server 实收和 Client 响应；HTTP 按实际正文与字段核对，Socket 逐字节核对；[离线验证](outputs/evidence-verification.json) |
| 重启持久化 | 正常退出并重启同一隔离 profile；所选 Workspace、2 个监听、5 条规则完全一致；运行态单独记录，重启后入口为空，再经 UI 启动。见 [比较](outputs/persistence/comparison.json) |
| 清理 | App 正常退出，控制器退出 0；DMG 卸载，MCP/包服务/4 个回放端口关闭；隔离数据库 integrity=ok、application_schema.version=100。见 [清理](outputs/cleanup.json) |

## 环境、资源与来源

- 环境与工具版本：[environment.json](outputs/environment.json)。本机 macOS 27 arm64；Deno 2.9.6、Rust/Cargo 1.98.0、Python 3.14.7。
- 原始当前输入：[current-inputs.tar.gz](resources/current-inputs.tar.gz)，1418 个任务相关 tracked 源码、配置、脚本和 staged APK。未把原有无关 gmofg_cps/http-finalize 未跟踪目录纳入。
- App/DMG 及其 1247 个构建输入：复用 [wasm-universal-build](../../../2026-09-07/TASK-20260829-002/wasm-universal-build/README.md)。本轮没有重新安装 /Applications 或修改日常数据库；签名为 ad-hoc。
- HTTP 规则和 Socket 原始向量父资源：[release-app-replay](../../../2026-09-01/TASK-20260901-001/release-app-replay/README.md)，原 `replay/http_matrix.py`、`replay/socket_matrix.py` 与 `inputs/socket/socket-{match-0200,miss-0400,invalid}.hex`。派生脚本保留原意并适配当前 HTTP topology 合同；为关联 TCP 连接增加 X-Wire-Case 请求头，不参加业务规则。
- 先前覆盖率修复：[frontend-coverage-followup](../../../2026-09-07/TASK-20260901-001/frontend-coverage-followup/README.md)。完整父任务/用例/路径见 metadata.derived_from；旧资源不覆盖。
- 本轮实际候选：[candidate.json](inputs/configuration/candidate.json)。HTTP 127.0.0.1:28080 → 28081；Socket 127.0.0.1:28082 → 28083；均为明文 loopback，认证/TLS 材料 N/A。Socket 使用内置 iso8583-ascii-standard@1.0.0、16 KiB read chunk，不是新增默认值。
- 原始请求、响应、Frame、`.bin` / `.hex`、TCP recv chunks 位于 inputs/wire 与 outputs/wire；完整 MCP 工作区、规则、抓包和双协议 Exchange 位于 outputs/wire/mcp。
- 退出后的真实隔离数据库与 runtime 日志：[isolated-app-data-after.tar.gz](resources/isolated-app-data-after.tar.gz)。实际 profile 路径及两次进程启动记录见 [session.json](outputs/app-session-home-only/session.json)。资源来自本次实际 App，不是生成替代数据。

## 操作与复测

离线检查当前归档（不启动 App、不联网）：

```bash
python3 docs/testing/evidence/2026-09-08/TASK-20260901-001/local-macos-regression/replay/verify_evidence.py
```

完整复测使用独立工作区与新用例目录，保持相同目录深度，只复制 replay/resources，不复制旧 outputs/inputs；需要复现精确源码时在该独立工作区恢复 current-inputs.tar.gz。把上述父证据/DMG置于其原相对路径。不得在本次已完成目录覆盖历史结果。

1. `python3 <新用例>/replay/quality.py`：依次运行全部 gate，保存输出并检查输入稳定。工具和系统依赖按 environment.json 及父构建用例准备。
2. `python3 <新用例>/replay/app_session.py`：只读挂载 DMG，以新临时 HOME 启动，等待 APP_READY；接受会话未设置 CFFIXED_USER_HOME。先确认 MCP 17653 与 28080–28083 空闲。
3. `python3 <新用例>/replay/preflight.py` 提供最长 180 秒的仅 TCP 预检端点；运行 `python3 <新用例>/replay/configure.py`，确认 preview_ready → committed 和工作区回读。预检端点只 accept/close，不处理业务。
4. 通过 App「代理入口配置」分别选择 HTTP 与 Socket，点击「启动监听」，确认「全部入口运行中 · 活动 2」。CUA 实際观察的标签与操作见 [UI 观察](outputs/ui-observations.json)；截图在操作时检查，归档用 AX 标签与 MCP 状态支撑功能结论，不作像素布局验收。
5. `python3 <新用例>/replay/persistence.py before`；创建 App 控制器 outputs/app-session-home-only/restart 标记，再通过 App Cmd+Q 正常退出；控制器自动用同一 profile 重启。运行 `persistence.py after` 比较，并通过 UI 再次启动两个监听。
6. 确认预检端点已退出。运行 `python3 <新用例>/replay/wire_matrix.py --mcp-port 17653`，看到 READY 后创建 outputs/wire/start-clients。必须以 summary PASS、8 条成功完整 wire chain、2 条明确 Decode 失败与原始字节为通过标准；超时不能当非法输入通过。
7. 完成后创建 outputs/app-session-home-only/stop 标记，再经 App Cmd+Q 退出；控制器卸载 DMG。确认 17653、8765、28080–28083 均关闭，保存真实 profile 与日志。控制器有 30 分钟测试监督期限，本轮在等待脚本时仅暂停/恢复自有监督进程，App及其协议超时未修改，见 controller-pause.json。

## 保留的尝试与限制

- 初次采用 HOME+CFFIXED_USER_HOME 的会话出现空白窗口，主 Agent 正常 Cmd+Q 结束；初版监督脚本把退出 0 误标为 unexpected，详见 outputs/app-session/controlled-close.json。接受会话改为仅 HOME 隔离；不能据此声称 CFFIXED_USER_HOME 是根因。
- 旧 HTTP settings.fixed_server 直接提交被当前 schema 拒绝；按当前源码移入 topology.remote_server.settings.fixed_server。下一次预检因上游未启动失败；最终预检/提交通过。见 outputs/configuration-attempts.json。
- [wire attempt 1](outputs/wire-attempt1-http-half-close/diagnosis.json)：派生 HTTP 客户端额外半关闭写方向，7 请求在 Read 失败；恢复历史 http.client 的读取响应期间保持写侧打开后重跑。Socket 原始结果保留。未修改 App 或宣称修复 HTTP 半关闭能力。
- [wire attempt 2](outputs/wire-attempt2-observation-error-format/diagnosis.json)：真实规则/字节已匹配，附加断言仍要求历史 error 文本带 JSON_INVALID。当前 Exchange 保存域错误 message（Invalid Document JSON），本轮按当前源码核对该消息、Decode、failed close 和无 sent 后重跑；没有放宽业务预期或忽略失败。
- 清理时初次把 SQLite PRAGMA user_version 当应用版本；当前权威表为 application_schema，实际 version=100、integrity=ok。错误检查已保留，数据库未修改。
- 启动空白等待、CUA 捕捉/AX 间歇错误及一次 MCP 查询超时见 [自动化观察](outputs/automation-observations.md)。没有连续启动计时或已确认产品根因，不能宣称性能/稳定性全面验收。
- 本轮 Windows、Android 真机 VPN、系统代理/证书权限、Developer ID/公证、远端 CI/Release 为 NOT_RUN。此次本机合同回归不替代上述平台与发布门禁。生产实现、旧归档、日常 App 数据和设备配置均未改动。

独立整体审查：[independent-review.md](outputs/independent-review.md)。本机子项完成时间：2026-09-08 10:53:56 +08:00。
