# 支持 Node.js 与 Deno 双开发工具链

## 任务信息

- 任务 ID：`TASK-20260831-001`
- 状态：`已完成`
- 任务日期：`2026-08-31`
- 创建时间：`2026-08-31 11:15:38 +08:00`
- 开始时间：`2026-09-02 09:58:32 +08:00`
- 最后更新时间：`2026-09-02 10:22:00 +08:00`
- 完成时间：`2026-09-02 10:22:00 +08:00`
- 创建路径：`docs/tasks/pending/2026-08-31/support-node-and-deno-development-toolchains.md`
- 归档路径：`docs/tasks/completed/2026-09-02/support-node-and-deno-development-toolchains.md`
- 关键词：`Node.js`、`Deno`、`Next.js`、`pnpm`、`Tauri CLI`、`deno task`、`node shim`、`dual toolchain`
- 任务优先级：`低`
- 优先级理由：用户已把范围收窄为本机开发启动兼容；本任务只增加可逆的 Deno 开发入口并回归现有 Node.js + pnpm 启动，不改变产品运行时、业务合同、正式打包、CI 或发布。若实施发现必须修改这些高风险边界，立即停止并重新确认优先级。

## 背景与目标

当前项目以 `package.json`、pnpm 和 Node.js 为主要前端工具链：Next.js、Vitest、ESLint、TypeScript、Tauri npm CLI 以及大量项目检查脚本均由该入口编排。当前 `src-tauri/tauri.conf.json` 的 `beforeDevCommand` 和 `beforeBuildCommand` 也直接调用 pnpm。

Deno 2.9 已支持 npm、`package.json`、npm CLI、Node API 兼容和缺少真实 Node 时的 best-effort `node` shim。Deno 官方提供了通过兼容开关、`deno install --allow-scripts` 和 `deno task` 运行 Next.js 的示例，但当前项目尚无 `deno.json`、Deno 任务入口或无 Node 环境验收证据。

目标是在不改变 Tauri 产品运行时和业务行为的前提下，保留现有 Node.js + pnpm 开发入口，并按照 Deno 官方 Next.js 方案增加 Deno 开发入口。最终在没有真实 Node.js 和 pnpm 的隔离 PATH 中，使用 Deno 安装依赖并启动 Next.js 前端及 Tauri 开发应用；Deno 路径不得暗中调用本机真实 Node.js 或 pnpm。

## 范围

- 保留当前 `package.json`、`pnpm-lock.yaml`、Node.js + pnpm 命令和现有依赖版本，不迁移或删除 Node 工具链。
- 按 Deno 官方 Next.js 教程增加 `deno.json`、所需 unstable 兼容开关、npm/node 兼容配置及 Deno tasks。
- Deno 依赖安装入口使用官方方式 `deno install --allow-scripts`；npm 依赖版本继续来自当前 `package.json`。
- 提供并验证 `deno task dev`，启动当前 Next.js 开发服务器。
- 提供并验证 `deno task tauri:dev`，通过 Deno 调用 Tauri CLI、启动当前 Tauri 开发应用并显示主窗口。
- 允许使用 Deno 官方提供的 best-effort node shim；验收时隔离真实 Node.js 和 pnpm，证明实际没有依赖本机安装。
- 回归现有 `pnpm dev` 和 `pnpm tauri:dev`，证明增加 Deno 入口没有破坏 Node.js 路径。
- 更新开发文档，记录 Node.js + pnpm 与 Deno 两种启动方法、Deno 兼容开关和已验证版本。

## 不在范围

- 不把 Deno 引入最终 Tauri App 运行时；已打包应用继续是 Rust + 系统 WebView。
- 不用 Deno 替换 Boa 协议包 Sidecar，也不改变协议包 JavaScript、WebSocket JSON-RPC 或 Host API 合同。
- 不因工具链兼容顺带升级 Next.js、React、Tauri、Rust crate 或其他业务依赖；确有必要时必须先记录原因和兼容影响并重新确认。
- 不增加工具链失败后的自动回退、默认成功或静默忽略。
- 未经用户明确要求，不触发远程 CI、不 push、不发布、不部署。
- 不要求开发机同时安装 Node.js 和 Deno；任务目标是分别验证两种独立环境，而不是把双安装作为成功条件。
- 不要求本任务让全部 lint、typecheck、test、coverage、bindings、架构扫描或聚合 `check` 命令支持 Deno。
- 不要求执行 `deno task tauri:build`、正式 macOS bundle、Windows/Linux/Android 构建或跨平台矩阵。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-08-31 11:15:38 +08:00` | 用户要求建立任务，评估并实现项目同时兼容 Node.js 与 Deno。当前只登记任务，不开始实现。 |
| `2026-08-31 11:15:38 +08:00` | 当前已验证 Deno 官方 Next.js 示例使用 Deno npm 兼容、unstable 兼容开关、`deno install --allow-scripts` 和 `deno task`；这证明路线可行，但不证明本仓库全部命令已经兼容。 |
| `2026-08-31 11:20:07 +08:00` | 用户明确不需要扩展确认：按 Deno 官方方案让本项目能够用 Deno 跑起来即可。任务据此收窄为 Next.js 与 Tauri 开发启动，保留现有 Node.js + pnpm；完整测试矩阵、正式构建、CI、发布和 Boa 运行时不在范围。 |
| `2026-09-02 09:58:32 +08:00` | 用户明确要求实施，并补充询问 Deno 与 Node.js 的优劣；实施继续按既定双入口范围执行，比较结论作为工具链边界说明，不扩大到替换 Node.js 主兼容目标。 |

## 未确认事项

无会改变实现方向的未确认事项。当前进行中的 `TASK-20260829-002` 正在修改共享 `package.json`；这是实施顺序约束，不是需求未知。本任务必须等待该共享文件稳定，或在实施前重新读取并基于届时内容适配，不得覆盖其修改。

## 需求就绪检查

- 问题、用户目标和成功结果：`PASS`，保留 Node.js + pnpm，并让项目按 Deno 官方方案完成开发启动。
- 范围与不在范围：`PASS`，只覆盖 Next.js dev 与 Tauri dev；完整门禁、正式构建、CI、发布和协议运行时明确排除。
- 输入、输出和状态变化：`PASS`，输入为当前 `package.json` 依赖和 Deno 官方兼容配置；输出为可重复的 Deno tasks 及开发应用启动结果。
- 错误行为：`PASS`，Deno 启动失败必须返回失败；不得静默调用真实 Node/pnpm、自动回退或报告成功。
- 具体示例：`PASS`，仅 Deno PATH 下依次执行 `deno install --allow-scripts` 与 `deno task tauri:dev`，Next dev server 启动且 Tauri 主窗口显示。
- 可重复 PASS/FAIL 验收：`PASS`，隔离 PATH 分别验证 Deno-only 与现有 Node.js + pnpm 启动。
- 会改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-09-02 09:58:32 +08:00`。共享 `package.json` 与 Tauri 配置本次读取无未提交差异，开始实施。

## 问题与根因分析

本任务是新增工具链兼容能力，不是已确认缺陷修复。

- 当前已验证：项目通过 pnpm 调用 Next.js、测试和检查脚本；大量脚本显式调用 `node`，Tauri 前置命令显式调用 `pnpm`；本机 pnpm 启动文件依赖 `/usr/bin/env node`。
- 当前已验证：Deno 2.9.6 已安装；Deno 官方文档和示例展示了 Next.js/npm/Node 兼容路径，但要求额外兼容配置和安装权限。
- 历史已验证：此前对 Deno 的评估集中在协议包 Sidecar 能力、隔离和资源边界，不等于前端开发工具链验收。
- 推断：Deno 官方兼容配置可以覆盖当前 Next.js 开发启动；Tauri npm CLI 是否能在仅 Deno 环境直接运行仍需实际验证。
- 未知：仅保留 Deno 的隔离 PATH 中，当前 `@tauri-apps/cli` npm 包及 Next.js `--webpack` 开发服务器的实际启动结果。
- 正确处理边界：只实现并验证 Deno 开发启动入口；不得把启动 Smoke 扩写为完整测试、正式构建或发布兼容。

## 最小改动与最优设计比较

| 方案 | 分析 |
| --- | --- |
| 保留现有 Node.js 配置，只增加官方风格 `deno.json` 和 `dev`/`tauri:dev` tasks | 修改最少、可逆，直接满足“用 Deno 跑起来”；允许 Deno 官方 node shim，但必须在隔离 PATH 实测，采用。 |
| 抽取全部 Node/Deno 共享命令并统一所有测试、构建和发布入口 | 长期一致性更强，但明显扩大到用户未要求的完整工具链迁移，暂不采用。 |
| 全面迁移为 Deno 主工具链，仅为 Node.js 保留包装 | 会改变现有 Node.js + pnpm 权威入口，不符合“同时兼容”，拒绝。 |

### Node.js 与 Deno 取舍

- Node.js 的优势是 Next.js、Tauri npm CLI、npm lifecycle 和原生 npm 包以其作为主要兼容目标，生态成熟且边缘兼容风险较低；代价是开发工具通常需要分别组合包管理器、TypeScript、格式化、Lint 和测试工具，默认权限边界较宽。
- Deno 的优势是原生 TypeScript、统一的安装/任务/格式化/Lint/测试工具、显式权限模型和内建 npm/Node 兼容层；代价是当前 Next.js/Tauri 路径仍建立在 npm/Node 兼容之上，需要 unstable 兼容开关、允许 npm lifecycle scripts，并可能依赖 best-effort `node` shim，边缘包兼容风险高于真实 Node.js。
- 本项目采用双入口：Node.js + pnpm 保持主要兼容与回归基线，Deno 提供独立开发启动入口；二者共享 `package.json` 的依赖版本，不建立自动回退或混用运行时。

## 小任务与依赖

| ID | 任务 | 依赖 | 可并行 | 状态 | 验收 |
| --- | --- | --- | --- | --- | --- |
| NDC-01 | 在共享 `package.json` 稳定后回归现有 `pnpm dev` 和 `pnpm tauri:dev` 启动基线 | TASK-20260829-002 共享文件稳定 | 否 | 已完成 | Next dev server 与 Tauri 主窗口可启动 |
| NDC-02 | 按 Deno 官方方案增加 `deno.json`、兼容开关和 `dev`/`tauri:dev` tasks | NDC-01 | 否 | 已完成 | 配置不修改现有 Node.js + pnpm 入口或依赖版本 |
| NDC-03 | 在无真实 Node/pnpm 的隔离 PATH 中执行 Deno 安装和启动 Smoke | NDC-02 | 否 | 已完成 | `deno install --allow-scripts`、`deno task dev` 和 `deno task tauri:dev` 成功，主窗口显示 |
| NDC-04 | 回归 Node.js 启动、更新开发文档并保存证据 | NDC-03 | 否 | 已完成 | 两种启动入口均通过，文档与实际命令一致 |

在共享 `package.json` 和 Tauri 配置稳定前不得并行实施。任务为低优先级；完成前由主 Agent 检查是否存在真实 Node/pnpm 泄漏、现有 Node 启动回归和文档漂移，不强制独立整体对抗审查。

## 测试计划

- Node.js 回归：当前 Node.js + pnpm 环境依次启动 `pnpm dev` 和 `pnpm tauri:dev`，确认 Next dev server 与 Tauri 主窗口。
- Deno 安装：在不暴露真实 `node`、`npm`、`pnpm` 的隔离 PATH 中执行 `deno install --allow-scripts`，记录依赖安装和 lifecycle script 结果。
- Deno 前端：执行 `deno task dev`，确认当前 Next.js `--webpack` 开发服务器成功监听并能返回首页。
- Deno Tauri：执行 `deno task tauri:dev`，确认 Tauri CLI 启动、Rust Host 初始化和主窗口显示。
- 运行时证明：记录 PATH、`command -v node`/`pnpm` 结果、Deno 版本以及子进程实际执行证据，避免误用本机 Node.js。
- 失败路径：Deno 配置、npm lifecycle 或 Tauri CLI 失败必须产生非零退出并保留 stderr，不自动转用 pnpm。
- `lint/typecheck/test/check`、正式 bundle、跨平台和 CI 全部记录为 `N/A` 或 `NOT_RUN`，原因是超出本次开发启动范围。

正式测试证据保存到 `docs/testing/evidence/<执行日期>/TASK-20260831-001/<用例ID>/`，记录运行时版本、PATH、依赖状态、命令、stdout/stderr、退出码、产物和复测入口。

## 对抗审查计划

- 检查 Deno-only Smoke 是否真的隐藏真实 Node.js/pnpm，是否只是假阳性。
- 检查 Deno unstable 兼容开关、`--allow-scripts`、权限和 node shim 是否与官方方案一致并被准确记录。
- 检查现有 Node.js + pnpm 入口、依赖版本和锁文件是否保持不变。
- 检查是否引入自动回退、默认成功或超出开发启动范围的工具链重构。

## 对抗审查结果

- `PASS`：Deno-only PATH 中 `command -v node` 与 `command -v pnpm` 均为 `NOT_FOUND`，3000 端口监听者为 `deno`；系统中其他应用自带的 Node 进程不属于测试进程链。
- `PASS`：当前 Deno 官方示例所需的 `unsafe-proto`、`sloppy-imports` 和 `--unstable-detect-cjs` 已准确落入配置；依赖安装明确执行 `deno install --allow-scripts`。
- `PASS`：`package.json`、`pnpm-lock.yaml`、`src-tauri/tauri.conf.json` 和 `src-tauri/tauri.dev.conf.json` 的 Git diff 为空，Node.js + pnpm 路径实际回归通过。
- `PASS`：没有自动回退、默认成功、Deno 产品运行时、完整测试迁移或发布/CI 扩展。任务为低优先级，本次由主 Agent 完成范围审查，不要求独立整体审查。

## 文档影响

- `README.md`：增加 Node.js + pnpm 与 Deno 两种开发启动入口。
- `docs/README.md`：本任务 pending 入口；完成时按规则移除。
- `docs/onboarding-guide.md`：本次不修改；该文件已有其他任务未提交修改，且根 README 与开发指南已提供完整 Deno 入口，避免覆盖无关工作。
- `docs/architecture/development-guide.md`：记录依赖安装、兼容开关、启动命令和故障排查。
- `docs/testing/release-validation-matrix.md`：无需更新，本任务不建立发布级 Deno 合同。
- CI/workflow 文档：无需更新，本任务不修改或触发 CI。
- 协议包与 Boa 文档：无需更新，本任务不改变产品脚本运行时。

## 实施记录

- `2026-08-31 11:15:38 +08:00`：读取当前项目脚本、Tauri 配置、Deno 官方 Next.js/Node 兼容资料和既有 Deno Sidecar 历史；登记任务。未修改源码、工具链配置或测试。
- `2026-08-31 11:20:07 +08:00`：用户将目标收窄为按 Deno 官方方案让本项目用 Deno 跑起来；关闭其余确认项，将范围固定为 Next.js/Tauri 开发启动及 Node.js + pnpm 回归，任务调整为低优先级 `待实现`。
- `2026-09-02 09:58:32 +08:00`：用户明确要求实施。重新读取共享文件与工作区状态，确认 `package.json`、`src-tauri/tauri.conf.json` 和 `src-tauri/tauri.dev.conf.json` 当前无未提交差异，任务进入 `进行中`。
- `2026-09-02 10:09:00 +08:00`：增加官方风格 `deno.json` 与 `deno.lock`；通过独立 Tauri overlay 将 Deno 入口的 `beforeDevCommand` 改为 `deno task dev`，保留原开发态 `freezePrototype: false`，未修改原 pnpm 配置。
- `2026-09-02 10:12:00 +08:00`：在 PATH 找不到 Node/pnpm 的环境执行 `deno install --allow-scripts` 与 `deno task dev`；全新稳定快照安装 507 个 npm 包，Next.js 由 Deno 监听并返回首页。
- `2026-09-02 10:17:00 +08:00`：在同一 Deno-only 稳定快照运行 `deno task tauri:dev`；Tauri CLI 调用 Deno 前端任务，Rust Host 启动，CoreGraphics 确认 `Intercept Proxy` 1440 x 900 主窗口显示。
- `2026-09-02 10:20:08 +08:00`：回归 `pnpm install --frozen-lockfile`、`pnpm dev` 和 `pnpm tauri:dev`；Node 监听、首页和 Tauri 主窗口通过。JSON、Deno format、diff whitespace 与原 Node/pnpm/Tauri 配置不变检查通过，保存可复测证据。
- `2026-09-02 10:22:00 +08:00`：完成主 Agent 对抗检查、任务文档、证据索引和归档一致性事务，任务关闭。

## 修改文件

- `README.md`
- `deno.json`
- `deno.lock`
- `src-tauri/tauri.deno.conf.json`
- `docs/architecture/development-guide.md`
- `docs/README.md`
- `docs/tasks/README.md`
- `docs/tasks/completed/2026-09-02/support-node-and-deno-development-toolchains.md`
- `docs/testing/evidence/README.md`
- `docs/testing/evidence/2026-09-02/TASK-20260831-001/dual-toolchain-development-smoke/`

## 附加文件

- Deno 官方 Next.js 教程：<https://docs.deno.com/examples/next_tutorial/>
- Deno 官方 Node/npm 兼容说明：<https://docs.deno.com/runtime/fundamentals/node/>
- Deno 官方示例仓库：<https://github.com/denoland/tutorial-with-next>
- 当前相关任务：`TASK-20260829-002`，仅用于共享文件协调；本任务不改变其协议包运行时合同。
- 验收证据：[dual-toolchain-development-smoke](../../../testing/evidence/2026-09-02/TASK-20260831-001/dual-toolchain-development-smoke/README.md)

## 验收结果

- `VERIFIED`：Deno-only 依赖安装、Next.js、Tauri CLI、Rust Host 与真实主窗口通过；Node.js + pnpm 安装、Next.js 与 Tauri 主窗口回归通过。
- `VERIFIED`：Deno 路径未暴露真实 Node/pnpm，现有 `package.json`、`pnpm-lock.yaml` 和原 Tauri 配置未修改。

## 测试结果

- `VERIFIED`：`deno install --allow-scripts`、`deno task dev`、`deno task tauri:dev`。
- `VERIFIED`：`pnpm install --frozen-lockfile`、`pnpm dev`、`pnpm tauri:dev`。
- `VERIFIED`：JSON 解析、`deno fmt --check`、`git diff --check` 和原 Node/pnpm/Tauri 配置无 diff。
- `NOT_RUN`：lint、typecheck、Vitest、完整 `pnpm check`、release bundle 和跨平台矩阵；均超出本任务开发启动范围。

## CI 情况

- `NOT_RUN`：用户未要求触发远程 CI。

## 完成总结

- 已保留 Node.js + pnpm 权威入口，并增加可独立安装和启动 Next.js/Tauri 的 Deno 入口。
- Deno 2.9.6 在没有真实 Node/pnpm 的 PATH 中完成真实开发启动；最终 Tauri App 运行时仍为 Rust + 系统 WebView。
- 任务按低优先级既定范围完成，没有扩展到完整质量门禁、正式构建、CI、发布或 Boa/Wasm 协议包运行时。
