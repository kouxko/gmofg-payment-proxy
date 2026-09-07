# 将 Deno 设为默认开发启动工具链

## 任务信息

- 任务 ID：`TASK-20260902-001`
- 状态：`已完成`
- 任务日期：`2026-09-02`
- 创建时间：`2026-09-02 10:27:05 +08:00`
- 开始时间：`2026-09-02 10:27:05 +08:00`
- 最后更新时间：`2026-09-02 10:35:23 +08:00`
- 完成时间：`2026-09-02 10:35:23 +08:00`
- 创建路径：`docs/tasks/pending/2026-09-02/default-deno-development-toolchain.md`
- 归档路径：`docs/tasks/completed/2026-09-02/default-deno-development-toolchain.md`
- 关键词：`Deno default`、`Node.js compatibility`、`beforeDevCommand`、`Tauri config overlay`、`pnpm`
- 任务优先级：`低`
- 优先级理由：只调整本地开发启动默认值和文档顺序，保留 Node.js + pnpm 独立覆盖配置；不修改产品运行时、依赖版本、正式构建、CI、发布或业务合同，变更可逆且影响范围隔离。

## 背景与目标

`TASK-20260831-001` 已验证 Deno 2.9.6 可以在无真实 Node/pnpm 的 PATH 中安装依赖并启动 Next.js 与 Tauri，同时 Node.js + pnpm 回归通过。用户随后明确要求“默认使用 Deno”，并授权删除该任务产生的两个临时测试目录。

目标是让基础 Tauri 开发配置默认执行 `deno task dev`，文档默认推荐 Deno；Node.js + pnpm 继续通过开发 overlay 明确执行 `pnpm dev`，不得形成自动回退或要求两种运行时同时安装。

## 范围

- 将 `src-tauri/tauri.conf.json` 的 `beforeDevCommand` 改为 `deno task dev`。
- 在 `src-tauri/tauri.dev.conf.json` 明确覆盖 `beforeDevCommand: pnpm dev`，保持 `pnpm tauri:dev` 为独立 Node 兼容入口。
- 收窄 `src-tauri/tauri.deno.conf.json` 为 Deno 开发态安全 overlay，默认命令由基础配置拥有。
- 调整根 README 和开发指南，以 Deno 作为默认开发启动方式，Node.js + pnpm 作为兼容与完整质量门禁入口。
- 验证基础配置与两套 overlay 的合并结果，并分别启动 Deno 与 pnpm Tauri 开发入口。
- 删除用户明确授权的 `/tmp/task-20260831-deno-snapshot.d5X0GX` 与 `/tmp/task-20260831-deno-bin.A8bByt`；优先使用可恢复的 macOS 废纸篓。

## 不在范围

- 不删除 `package.json`、`pnpm-lock.yaml`、Node.js/pnpm scripts 或 Node 兼容入口。
- 不把 lint、typecheck、test、完整 `check`、release build、CI 或发布迁移到 Deno。
- 不修改 `beforeBuildCommand`；正式构建继续保持现有 pnpm 合同。
- 不修改最终 Tauri App 运行时、Boa/Wasm 协议包运行时或业务代码。
- 不增加运行时检测、自动回退、静默切换或默认成功。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-09-02 10:27:05 +08:00` | 用户明确要求“默认使用 Deno”，结合前序双工具链语境，默认值限定为本地 Next.js/Tauri 开发启动；Node.js + pnpm 继续兼容。 |
| `2026-09-02 10:27:05 +08:00` | 用户明确授权删除前序任务列出的两个 `/tmp` 临时目录；永久删除命令被安全策略拒绝后，改用 `/usr/bin/trash` 移入 macOS 废纸篓并确认原路径不存在。 |

## 未确认事项

无。用户要求改变默认开发入口；完整质量门禁和正式构建是否迁移到 Deno 不属于当前请求，继续保持既有 Node.js + pnpm 合同。

## 需求就绪检查

- 目标与成功结果：`PASS`，Deno 成为默认开发启动，Node/pnpm 保持兼容。
- 范围与不在范围：`PASS`，只调整开发配置和文档，不迁移构建/质量门禁。
- 输入、输出和状态：`PASS`，基础 Tauri 配置选择 Deno，Node overlay 选择 pnpm。
- 错误行为：`PASS`，各入口失败直接失败，不自动切换运行时。
- 具体示例：`PASS`，基础/`deno task tauri:dev` 输出 `BeforeDevCommand (deno task dev)`；`pnpm tauri:dev` 输出 `BeforeDevCommand (pnpm dev)`。
- 可重复验收：`PASS`，检查 JSON Merge Patch 结果并真实启动两套入口。
- 会改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-09-02 10:27:05 +08:00`

## 最小改动与最优设计比较

| 方案 | 分析 |
| --- | --- |
| 只调整文档顺序 | 用户看到 Deno 为默认，但基础 Tauri 配置仍调用 pnpm，声明与实际不一致，拒绝。 |
| 基础配置默认 Deno，Node 开发 overlay 显式覆盖 pnpm | 一个默认 owner、一个兼容 overlay，命令来源清晰且不破坏双入口，采用。 |
| 删除 Node/pnpm 配置并全面迁移质量门禁 | 超出用户当前要求并破坏已验证兼容入口，拒绝。 |

## 小任务与验收

| ID | 任务 | 状态 | 验收 |
| --- | --- | --- | --- |
| DDF-01 | 删除用户授权的临时测试目录 | 已完成 | 两个原路径均不存在，内容可从废纸篓恢复 |
| DDF-02 | 调整基础配置和 Deno/Node overlay | 已完成 | 默认 Deno、Node 显式 pnpm，JSON 有效 |
| DDF-03 | 同步开发文档 | 已完成 | Deno 优先且范围说明准确 |
| DDF-04 | 真实复测并归档证据 | 已完成 | 两套入口显示各自前置命令、Next Ready、Rust App 启动；主窗口受同一既有证书状态阻断并如实记录 |

## 测试计划

- JSON 解析与 `deno fmt --check`。
- 配置合并静态检查：基础 `beforeDevCommand=deno task dev`，Node overlay=`pnpm dev`，Deno overlay不重复拥有默认命令。
- `deno task tauri:dev`：确认 Deno 前端任务、Next.js Ready、Rust Host/主窗口。
- `pnpm tauri:dev`：确认 pnpm 前端任务、Next.js Ready、Rust Host/主窗口。
- `git diff --check`，并确认依赖版本、锁文件和 `beforeBuildCommand` 未改变。
- lint、typecheck、完整测试、release bundle、CI：`NOT_RUN`，超出本任务范围。

## 文档影响

- `README.md`
- `docs/architecture/development-guide.md`
- 任务与测试证据索引

## 实施记录

- `2026-09-02 10:27:05 +08:00`：读取前序任务、当前配置和 Tauri frontend/config skills；确认基础配置仍默认 pnpm，登记本任务。
- `2026-09-02 10:27:05 +08:00`：复核临时目录清单；永久删除命令被安全策略拒绝，随后用 macOS `/usr/bin/trash` 将两个精确目录移入废纸篓并确认原路径不存在。
- `2026-09-02 10:33:48 +08:00`：将基础 `beforeDevCommand` 改为 `deno task dev`，Node 开发 overlay 显式覆盖 `pnpm dev`；Deno overlay 只保留开发态安全配置，正式构建仍为 `pnpm build`。
- `2026-09-02 10:33:48 +08:00`：同步根 README 与开发指南，声明 Deno 默认、Node/pnpm 兼容和完整质量门禁边界。
- `2026-09-02 10:33:48 +08:00`：JSON、格式、合并断言通过；两套入口均启动 Next 与 Rust App，但都被本机既有 `CERTIFICATE_ROOT_REVOKED` 状态阻断主窗口，未清理用户应用数据。

## 修改文件

- `docs/README.md`
- `README.md`
- `docs/architecture/development-guide.md`
- `docs/tasks/completed/2026-09-02/default-deno-development-toolchain.md`
- `docs/testing/evidence/2026-09-02/TASK-20260902-001/default-deno-development-entry/README.md`
- `docs/testing/evidence/2026-09-02/TASK-20260902-001/default-deno-development-entry/metadata.json`
- `docs/testing/evidence/2026-09-02/TASK-20260902-001/default-deno-development-entry/outputs/runtime-summary.txt`
- `docs/testing/evidence/2026-09-02/TASK-20260902-001/default-deno-development-entry/resources/*`
- `src-tauri/tauri.conf.json`
- `src-tauri/tauri.dev.conf.json`
- `src-tauri/tauri.deno.conf.json`

## 验收结果

- `VERIFIED`：基础配置默认 `deno task dev`，Node overlay 明确为 `pnpm dev`，Deno overlay 从基础配置继承默认命令，`beforeBuildCommand` 保持 `pnpm build`。
- `VERIFIED`：Deno 入口 Next.js Ready、Deno 监听 3000、首页 HTTP 200，Rust App 构建并启动。
- `VERIFIED`：pnpm 入口 Next.js Ready，Rust App 构建并启动，证明 Node 兼容 overlay 仍生效。
- `INCONCLUSIVE`：两套入口均在相同 Rust App setup 边界返回既有 `CERTIFICATE_ROOT_REVOKED`，当前主窗口未显示；父任务早先环境中的主窗口证据仍保留但不替代本次结果。
- `VERIFIED`：两个授权临时目录的原路径不存在，内容已移入 macOS 废纸篓，可在清空废纸篓前恢复。

## 测试结果

- 用例：`default-deno-development-entry`
- 结果：`VERIFIED_WITH_APP_STATE_BLOCKER`
- 证据：[默认 Deno 开发入口与 Node 兼容入口验证](../../../testing/evidence/2026-09-02/TASK-20260902-001/default-deno-development-entry/README.md)
- 父证据：[Node.js/pnpm 与 Deno 双开发入口 Smoke](../../../testing/evidence/2026-09-02/TASK-20260831-001/dual-toolchain-development-smoke/README.md)
- `NOT_RUN`：lint、typecheck、Vitest、完整 `pnpm check`、release bundle、CI 与跨平台矩阵，原因见证据。

## CI 情况

- `NOT_RUN`：用户未要求触发远程 CI。

## 完成总结

- Deno 已成为本地开发默认入口，Node.js + pnpm 继续作为显式兼容入口；依赖锁文件、完整质量门禁和正式构建合同未迁移。当前本机旧证书状态是独立于工具链的剩余运行环境阻塞，未在本任务中修改。
