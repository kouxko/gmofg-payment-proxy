# HTTP 原始请求路径展示验收

任务：TASK-20260909-001；用例：capture-url；日期：2026-09-09（Asia/Shanghai）。

## 目的与当前被测状态

验证连接表格显示最新 App 原始请求目标，完整保留路径与查询参数，单行超宽省略；不被后续规则改写、
编码、转发或响应替换。被测分支 `feature/capture-http-url`，基线 `1c88aeb8`。
实际文件摘要、工具和系统版本见 [tested-state](outputs/tested-state.json)。冻结测试期间未修改 capture 源码。

## 输入、步骤与结果

- 活动输入来源：`src/features/capture/exchange-observation-test-fixture.ts` 及 model/list 组件测试。
  [当次源码与输入快照](resources/tested-source/src/features/capture/) 保留原始内容；本用例为新需求，无父任务。
- 输入包括 `/pay`、`/`、空 query、重复参数、`%2f`/`%2F`、`+`、2048 字符参数、绝对 request-target、
  `*`、缺失请求行、Socket、后续 encoded/sent/response 和多次请求。按原始 request-target 保留文本，不解析重建 URL。
- `deno task test src/features/capture`：6 文件 30/30 PASS；[终端输出摘录](outputs/capture-tests.log)。
- `deno task typecheck`：PASS；[输出](outputs/typecheck.log)。
- 针对 4 个变更 TS/TSX 文件的 ESLint：PASS，0 输出；[输出](outputs/lint.log)。
- `deno task scan:frontend-boundaries`：最终 PASS；[输出](outputs/frontend-boundaries.log)。初次临时 harness
  使用原生 button 命中控件规则，已改为 HeroUI Button，完成后移除整个临时 route。
- `git diff --check`：PASS。
- 浏览器：Playwright CLI + 已安装 Microsoft Edge，运行实际 ExchangeObservationList、HeroUI 及应用 CSS，
  fixture 注入，使用临时路由绕过桌面 IPC 启动。先显示 `/pay`，点击追加长请求，再点击 URL 验证连接选中。
  1280/960 视口均 PASS：256px 路径宽，15049px 文本滚动宽，16px 单行高，nowrap/ellipsis/hidden；
  1280 视口表格前后均 1232px。完整 DOM/title 文本保留，selected=`exchange-http-1`。
  [检查脚本](replay/browser-check.js)、[实际结果](outputs/browser-result.json)、[原始执行输出](outputs/browser-check.log)、
  [截图](outputs/capture-url.png)。浏览器 console 为 0 errors / 0 warnings（仅 React DevTools/HMR 提示）。
- 初次 CLI chromium 启动缺少其指定版本，改用本机已有 Edge；未安装新浏览器。

## 复测

在对应 feature commit 的干净隔离 worktree 中，安装锁定依赖，然后执行：

```sh
deno task test src/features/capture
deno task typecheck
deno run -A npm:eslint@9 src/features/capture/exchange-observation-model.ts src/features/capture/exchange-observation-model.test.ts src/features/capture/exchange-observation-list.tsx src/features/capture/exchange-observation-list.test.tsx
deno task scan:frontend-boundaries
deno task build --webpack
git diff --check
```

浏览器复测使用 [browser-harness](resources/browser-harness.tsx)。将它复制为临时
`src/app/capture-url-smoke/page.tsx`，暂时在 `src/app/layout.tsx` 移除 AppProviders import 并将
`<AppProviders>{children}</AppProviders>` 换成 `{children}`。保留 ThemeProvider 与 globals.css。
启动 `deno task dev --hostname 127.0.0.1 --port 3187`；使用
`~/.codex/skills/playwright/scripts/playwright_cli.sh -s=capture-url open http://127.0.0.1:3187/capture-url-smoke --browser msedge`，
先 snapshot，再通过 run-code 参数传入 `replay/browser-check.js` 的完整内容。运行前创建 `output/playwright/`。
脚本逐项断言并返回实际 JSON，截图写入 `output/playwright/capture-url.png`。
完成后 close 专用浏览器 session，停止 dev server，删除临时 route 并精确恢复 layout。
本次已完成清理：临时 layout/route 无最终差异，3187 无 LISTEN，专用浏览器已关闭。

## 资源和边界

`resources/tested-source` 为实际源码/fixture/config 快照，`resources/browser-harness.tsx` 为当次可复现注入页面；
构建依赖通过 package.json/deno.lock 版本重建，本体不归档。未更改依赖合同，worktree 复用原工作区 node_modules。
不执行独立整体对抗审查：低风险局部展示，经主 Agent 检查原始请求选择、文本保持、空数据、组件交互与浏览器布局。
真实 Tauri WebView、设备、网络链路、Frame/字节、业务、CI/制品发布为 N/A 或 NOT_RUN：本任务只验收展示组件，
不把 fixture 浏览器验证描述为真实 App 网络抓包验收。

## 构建结果

默认 `deno task build` 因 Turbopack 不接受指向 worktree 根外的 node_modules 符号链接失败，
[原始输出](outputs/build.log)。`deno task build --webpack` PASS，包含 TypeScript 和静态页面导出；
[完整输出](outputs/build-webpack.log)。未修改构建配置或依赖；默认 Turbopack 在独立依赖环境未复验。
