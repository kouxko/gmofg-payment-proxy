# Deno-only 前端、Tauri 构建与 CI 配置迁移验证

## 目的与结论

- 任务：`TASK-20260902-002`
- 用例：`deno-only-toolchain-migration`
- 执行时间：`2026-09-02 10:40:16 +08:00` 至 `2026-09-02 11:01:59 +08:00`
- 结果：`LOCAL_VERIFIED_CI_NOT_RUN_WITH_KNOWN_BLOCKERS`
- 派生自：`TASK-20260902-001` / `default-deno-development-entry` /
  `docs/testing/evidence/2026-09-02/TASK-20260902-001/default-deno-development-entry/README.md`

本用例验证本地依赖安装、前端 tasks、Next.js production build 和 Tauri 正式构建在系统
`node`、`npm`、`pnpm` 均不可见的 PATH 中运行。GitHub Actions 配置已经迁移到 Deno，但按用户要求
不触发远程 CI，因此不把本地工作流合同检查等同于远程 runner 通过。

## 当次资源

`resources/` 保存实际被测快照：

- `deno.json`、`deno.lock`、`package.json`；
- `tauri.conf.json`、`tauri.dev.conf.json`；
- `ci.yml`、`windows-release.yml`、`windows-quick-build.yml`；
- `deno-toolchain-contract.test.mjs`。

`pnpm-lock.yaml` 和 `src-tauri/tauri.deno.conf.json` 已删除，因此没有为不存在的文件创建占位资源。

## Deno-only 环境

通过 `mktemp -d` 创建只包含 Deno 符号链接的临时工具目录，并使用以下 PATH：

```text
<temporary-deno-bin>:/Users/codin/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin
node=NOT_FOUND
npm=NOT_FOUND
pnpm=NOT_FOUND
deno=2.9.6
```

每次执行后通过精确 `unlink` 和 `rmdir` 清理该临时目录；`/tmp` 下没有
`task-20260902-deno-*` 残留。

## 已验证结果

- `deno ci`：`PASS`，冻结 `deno.lock` 安装 507 个包，并按 `allowScripts` 执行
  `unrs-resolver@1.12.2` postinstall。
- `deno task lint`：`PASS`。
- `deno task typecheck`：`PASS`。
- `deno task test`：`PASS`，63 个测试文件、532 个测试通过。
- `deno task test:coverage-policy`：`PASS`，4/4。
- `deno task test:bindings-check`：`PASS`，6/6。
- `deno task test:deno-toolchain` 与 Windows workflow 合同：`PASS`，5/5。
- `deno task scan:architecture`：`PASS`。
- `deno task scan:source-size`：`PASS`。
- `deno task build`：`PASS`，Next.js 16.2.12 production build、TypeScript 和 13 个静态路由通过。
- `deno task tauri build --bundles app`：`PASS`；日志确认
  `Running beforeBuildCommand deno task build`，并生成
  `src-tauri/target/release/bundle/macos/Intercept Proxy.app`。
- JSON 解析、所选 Deno 格式检查、活动工作流 Node/pnpm 静态扫描和 `git diff --check`：`PASS`。

## 已知失败与边界

- `deno audit --level high --frozen-lockfile`：`FAILED`。当前锁文件包含 5 个既有公告：
  1 个 critical（Vitest 4.0.x）和 4 个 high（`brace-expansion` 三项、`js-yaml` 一项）。本任务没有
  擅自升级依赖或添加 ignore；当前严格 CI audit 会在这些依赖修复前失败。
- `deno task scan:bundle-branding`：`FAILED`。新生成的 macOS 可执行文件命中既有 `/gmofg/i`
  规则；该结果不影响 Deno 构建完成，但完整聚合 `deno task check` 不能记为通过。
- GitHub Actions 远程执行：`NOT_RUN`，用户明确要求先修改 CI、暂不着急验证。
- Windows/Linux runner、代码签名、发布、上传、push：`NOT_RUN`。

## 复测入口

```bash
deno ci
deno task test:deno-toolchain
deno task lint
deno task typecheck
deno task test
deno task build
deno task tauri build --bundles app
deno audit --level high --frozen-lockfile
```

完整摘要见 `outputs/validation-summary.txt`。
