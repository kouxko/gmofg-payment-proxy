# 将完整前端与 Tauri 工具链迁移到 Deno

## 任务信息

- 任务 ID：`TASK-20260902-002`
- 状态：`已完成`
- 任务日期：`2026-09-02`
- 创建时间：`2026-09-02 10:40:16 +08:00`
- 开始时间：`2026-09-02 10:40:16 +08:00`
- 最后更新时间：`2026-09-02 11:03:39 +08:00`
- 完成时间：`2026-09-02 11:03:39 +08:00`
- 创建路径：`docs/tasks/pending/2026-09-02/migrate-entire-frontend-toolchain-to-deno.md`
- 归档路径：`docs/tasks/completed/2026-09-02/migrate-entire-frontend-toolchain-to-deno.md`
- 关键词：`Deno`、`Next.js build`、`Tauri build`、`deno ci`、`GitHub Actions`、`pnpm removal`
- 任务优先级：`高`
- 优先级理由：迁移覆盖正式构建、完整质量门禁、跨平台 CI/发布工作流和唯一前端锁文件，会改变仓库级开发与交付合同。

## 背景与目标

`TASK-20260831-001` 已建立 Deno 开发入口，`TASK-20260902-001` 已把 Deno 设为默认开发入口。用户进一步明确
要求正式构建也使用 Deno，并希望“完全切换为 Deno”。本次已先执行 `deno task build`，Next.js 16.2.12
production build、TypeScript 和 13 个静态页面全部成功。

目标是让本地开发、依赖安装、正式前端构建、Tauri 构建、质量门禁和 GitHub Actions 都不再要求系统安装
Node.js 或 pnpm。依据 Deno 官方既有 Node 项目迁移与 Next.js 指南，`package.json` 继续作为 npm 依赖清单，
`deno.lock` 成为唯一前端锁文件，所有执行入口由 `deno task` / `deno ci` 拥有。

## 范围

- 把所有前端、脚本、质量门禁和 Tauri 命令迁移为 Deno 可直接读取的 tasks；通用 npm scripts 保留在
  `package.json`，需要显式 Deno flags 的入口保留在 `deno.json`。
- 把显式 `node` 执行改为 Deno Node 兼容执行，把 task 间调用从 pnpm 改为 `deno task`。
- 将 `beforeBuildCommand` 改为 `deno task build`。
- 删除 Node 专用 Tauri overlay，保留单一 Deno 开发 overlay。
- 让 `deno task tauri:build` 先构建 Android Companion，再执行 Tauri 正式构建。
- GitHub Actions 使用 `denoland/setup-deno`、`deno ci` 和 `deno task`；移除 Node/pnpm setup 与 pnpm audit。
- 删除 `pnpm-lock.yaml` 和 `package.json.packageManager`，保留 `package.json` npm 依赖声明。
- 同步 README、开发指南、工作流测试与硬编码命令提示。

## 不在范围

- 不把 npm 生态依赖改写成 JSR 或自行替换 Next.js、React、Tauri、Vitest、ESLint、TypeScript。
- 不改变最终 Tauri App 的 Rust/WebView 运行时，也不改变 Boa/Wasm 协议包运行时。
- 不改变业务功能、数据库、证书状态、协议合同或 Android Companion 构建逻辑。
- 不触发远程 CI、发布、上传或推送。
- 不加入 Node/pnpm 自动回退或双锁文件。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-09-02 10:40:16 +08:00` | 用户明确要求正式构建也使用 Deno，并要求“完全切换为 Deno”；该结论覆盖前序保留 Node/pnpm 兼容入口的决定。 |
| `2026-09-02 10:40:16 +08:00` | 按 Deno 官方既有 Node 项目迁移方式保留 `package.json` 作为 npm 依赖清单；保留文件不代表要求 Node.js 运行时。 |
| `2026-09-02 10:40:16 +08:00` | `deno.lock` 成为唯一前端锁文件，CI 使用 `deno ci` 验证冻结依赖。 |
| `2026-09-02 10:40:16 +08:00` | 用户先要求本地测试、CI 后续处理，随后明确更新为“CI 改但是不着急验证”；因此本次修改 CI 配置并做本地合同测试，远程 CI 明确 `NOT_RUN`。 |

## 未确认事项

无。用户要求仓库工具链完全迁移；`package.json` 的保留方式由 Deno 官方 Next.js/npm 兼容合同确定。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`，无 Node/pnpm 的本地与 CI 工具链可以完成开发、正式构建和质量门禁。
- 范围与不在范围：`PASS`，迁移构建工具链，不改变产品运行时和业务代码。
- 输入、输出和状态：`PASS`，输入为 `package.json` + `deno.json` + `deno.lock`，输出保持 Next `out/` 和 Tauri artifacts。
- 错误行为：`PASS`，Deno install/task/build/check 失败必须直接失败，不自动回退 Node/pnpm。
- 具体示例：`PASS`，`deno ci`、`deno task build`、`deno task tauri:build`、`deno task check`。
- 可重复验收：`PASS`，使用无 Node/pnpm 的 PATH、静态扫描、真实构建和工作流合同测试。
- 会改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-09-02 10:40:16 +08:00`

## 最小改动与最优设计比较

| 方案 | 分析 |
| --- | --- |
| 只改 `beforeBuildCommand` | 可以让 Tauri 前端 build 使用 Deno，但 lint/test/check/CI 仍要求 pnpm，不满足完全切换。 |
| 保留 package scripts，只把最外层命令改为 Deno | 内部仍有大量显式 `node`/`pnpm`，无 Node PATH 会失败并形成隐性双工具链，拒绝。 |
| Deno 拥有执行入口，`package.json` 保留 Deno 可读取的依赖与通用 scripts | 不重复维护两套 task；显式 Deno flags 放在 `deno.json`，其余 scripts 由 `deno task` 读取，单一 `deno.lock`，采用。 |

## 小任务与验收

| ID | 任务 | 状态 | 验收 |
| --- | --- | --- | --- |
| FDM-01 | 迁移 tasks、Next 与 Tauri dev/build | 已完成 | Deno-only PATH 下 build/Tauri 正式构建不查找 Node/pnpm |
| FDM-02 | 迁移脚本、测试与质量门禁 | 已完成 | lint、typecheck、532 个前端测试、架构和源码大小门禁通过；完整聚合门禁受既有品牌扫描失败限制 |
| FDM-03 | 迁移 CI 与发布工作流 | 已完成 | 无 setup-node/pnpm action/pnpm 命令，工作流合同 5/5；远程 CI `NOT_RUN` |
| FDM-04 | 删除双工具链合同并同步文档 | 已完成 | 单一 `deno.lock`，活动文档不再要求 Node/pnpm |
| FDM-05 | Deno-only 正式构建与整体验收 | 已完成 | 冻结安装、Next build、Tauri App build 有证据；audit 与品牌扫描失败如实归档 |
| FDM-06 | 对抗审查、修复与归档 | 已完成 | 复核 action inputs、回归合同、证据和索引一致性 |

## 测试计划

- `deno ci`：冻结 `deno.lock` 安装依赖和生命周期脚本。
- Deno-only PATH：确认 `node`、`npm`、`pnpm` 均不可见，执行核心 tasks。
- `deno task build`：Next.js production/static export。
- `deno task tauri:build -- --bundles app`：正式 Tauri App 构建；若外部签名/平台环境阻塞则分层记录。
- `deno task check` 的受影响子门禁逐项执行；聚合命令若被既有门禁失败阻断则保留失败阶段，不改写期望。
- 工作流与 task 静态扫描：禁止活动配置继续调用 Node/pnpm，证据快照中的历史文本除外。
- GitHub Actions YAML/合同测试、JSON 解析、`deno fmt --check`、`git diff --check`。
- 远程 CI、发布、推送：`NOT_RUN`，不在授权范围。

## 文档影响

- `README.md`
- `docs/architecture/development-guide.md`
- `docs/architecture/data-flow.md`
- `docs/architecture/README.md`
- `docs/onboarding-guide.md`
- `docs/testing/release-validation-matrix.md`
- 任务索引与测试证据索引

## 官方依据

- Deno Next.js 教程：`https://docs.deno.com/examples/next_tutorial/`
- Deno 既有 Node 项目迁移：`https://docs.deno.com/examples/migrate_node_project_tutorial/`
- Deno tasks：`https://docs.deno.com/runtime/reference/cli/task/`
- Deno GitHub Actions：`https://docs.deno.com/examples/deno_github_actions_tutorial/`

## 实施记录

- `2026-09-02 10:40:16 +08:00`：读取前序任务、项目历史、Tauri frontend/config skills 与 Deno/Tauri 官方文档；扫描当前 package scripts、Tauri 配置和工作流。
- `2026-09-02 10:40:16 +08:00`：真实执行 `deno task build`，Next.js 16.2.12 production build、TypeScript 和 13 个静态页面全部通过。
- `2026-09-02 11:01:59 +08:00`：迁移 package scripts、显式 Node/pnpm 子进程、Tauri dev/build 配置、macOS/Windows 打包脚本和 CI/release workflows；删除 `pnpm-lock.yaml`、Node overlay 与 `packageManager`。
- `2026-09-02 11:01:59 +08:00`：`deno ci` 在允许 `unrs-resolver` lifecycle script 后冻结安装 507 个包；Deno-only PATH 中 lint、typecheck、63 文件/532 测试、Next build 和 Tauri macOS App build 通过。
- `2026-09-02 11:01:59 +08:00`：新增 Deno 工具链回归合同；本地 5/5 工具链/workflow 合同通过，并从 `setup-deno` 官方 action 定义确认 `cache: true` 是有效输入。
- `2026-09-02 11:01:59 +08:00`：对抗复核保留两个真实失败：严格依赖 audit 发现 1 critical + 4 high，bundle branding 命中新二进制中的 `gmofg`；未增加 ignore、未升级依赖、未把失败记为成功。

## 修改文件

- `.github/workflows/ci.yml`
- `.github/workflows/windows-release.yml`
- `.github/workflows/windows-quick-build.yml`
- `deno.json`、`deno.lock`、`package.json`；删除 `pnpm-lock.yaml`
- `src-tauri/tauri.conf.json`、`src-tauri/tauri.dev.conf.json`；删除 `src-tauri/tauri.deno.conf.json`
- `scripts/deno-toolchain-contract.test.mjs`
- `scripts/build-macos-universal.mjs`、`scripts/package-portable.ps1`
- `scripts/check-*.mjs` 与 `scripts/check-*.test.mjs` 中受影响的显式运行时命令和提示
- `README.md`、`docs/architecture/README.md`、`docs/architecture/data-flow.md`、
  `docs/architecture/development-guide.md`、`docs/onboarding-guide.md`、
  `docs/testing/release-validation-matrix.md`
- `docs/README.md`、任务档案与测试证据索引

## 附加文件

- `docs/testing/evidence/2026-09-02/TASK-20260902-002/deno-only-toolchain-migration/`

## 验收结果

- `LOCAL_VERIFIED_CI_NOT_RUN_WITH_KNOWN_BLOCKERS`：本地前端与 Tauri 工具链已完全由 Deno 驱动；CI 配置已迁移但远程 runner 未执行。严格 audit 和品牌门禁的真实失败未隐藏。

## 测试结果

- `deno ci`：`PASS`，507 个包，所需 postinstall 已显式允许并执行。
- Deno-only PATH：`node`、`npm`、`pnpm` 均 `NOT_FOUND`。
- lint、typecheck、架构、源码大小、coverage policy、bindings check：`PASS`。
- 前端：63 文件、532 测试 `PASS`；Next 13 个静态路由 build `PASS`。
- Tauri：`deno task tauri build --bundles app` `PASS`，生成 macOS `.app`。
- 工具链与 Windows workflow 合同：5/5 `PASS`。
- `deno audit --level high --frozen-lockfile`：`FAILED`，1 critical + 4 high 既有依赖公告。
- `deno task scan:bundle-branding`：`FAILED`，生成的 macOS 可执行文件命中 `/gmofg/i`。
- 完整 `deno task check`：未记为通过；上述品牌门禁失败会阻断聚合结果。

## CI 情况

- 配置已改为 `denoland/setup-deno`、`deno ci`、`deno task`、`deno audit` 和 `deno.lock` cache key。
- `NOT_RUN`：按用户要求不触发远程 CI；只在本地验证 workflow 合同。
- 已知阻塞：严格 audit 会因当前 5 个依赖公告失败，需另行升级依赖后再做远程 CI 验证。

## 完成总结

- Node.js/pnpm 已从活动前端、Tauri 正式构建和 CI 工具链中移除；`package.json` 仅作为 Deno 官方支持的 npm 依赖与 scripts 清单保留。当前本地迁移和正式构建已验证，远程 CI、跨平台 runner 和发布继续 `NOT_RUN`，依赖 audit 与品牌扫描失败已作为后续风险保留。
