# Node.js/pnpm 与 Deno 双开发入口 Smoke

## 目的与结果

- 任务：`TASK-20260831-001`
- 用例：`dual-toolchain-development-smoke`
- 执行时间：`2026-09-02 09:58:32 +08:00` 至 `2026-09-02 10:20:08 +08:00`
- 结果：`VERIFIED`

验证新增 Deno 入口在 PATH 中没有真实 `node`、`npm`、`pnpm` 时可以安装依赖、启动 Next.js 和
Tauri 开发 App，同时回归现有 Node.js + pnpm 入口。测试只证明开发启动，不证明完整门禁、release
bundle、CI 或跨平台兼容。

## 被测配置

任务相关配置的不可变快照保存在 `resources/`：

- `deno.json`、`deno.lock`；
- `package.json`、`pnpm-lock.yaml`；
- `tauri.conf.json`、`tauri.dev.conf.json`、`tauri.deno.conf.json`。

哈希及运行结果摘要见 `outputs/runtime-summary.txt`。Tauri 验收在 `/tmp` 中建立不含 `.git`、
`node_modules`、`.next`、`out`、Rust `target` 的双次 `rsync` 稳定快照；任务相关文件哈希与仓库文件
逐项一致。这样主工作区的其他并行修改不会触发 Tauri watcher，也不会改变当次被测状态。

## Deno-only 环境

使用只包含 Deno、Rust 和 macOS 系统工具的 PATH：

```text
/tmp/task-20260831-deno-bin.A8bByt:/Users/codin/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin
```

进入测试前：

```text
deno=/tmp/task-20260831-deno-bin.A8bByt/deno
node=NOT_FOUND
pnpm=NOT_FOUND
deno 2.9.6 (stable, release, aarch64-apple-darwin)
```

执行：

```bash
deno install --allow-scripts
deno task dev
deno task tauri:dev
```

实际结果：

- 全新稳定快照安装 507 个 npm 包，运行 `unrs-resolver` 和 `esbuild` 的 postinstall，退出码为 0；
- `deno task dev` 启动 Next.js 16.2.12 webpack server，监听者为 `deno`，首页 HTTP 200，标题为
  `网络代理工具`；
- `deno task tauri:dev` 的 `BeforeDevCommand` 为 `deno task dev`，Rust Host 启动；
- macOS CoreGraphics 窗口列表确认 `owner=intercept-proxy`、`name=Intercept Proxy`、layer 0、
  1440 x 900；
- 调试链接阶段出现既有 `__eh_frame section too large` warning，但构建完成且 App 正常运行，不是本用例
  失败。

系统中其他应用仍可能自带或运行 Node 进程；本用例的隔离标准是测试 PATH 找不到真实 Node/pnpm，且
3000 端口监听进程及任务进程链为 Deno。未删除系统或其他应用的 Node。

## Node.js + pnpm 回归

环境：Node.js `v26.8.1`、pnpm `11.13.1`。执行：

```bash
pnpm install --frozen-lockfile
pnpm dev
pnpm tauri:dev
```

实际结果：

- pnpm 按现有 `pnpm-lock.yaml` 安装成功；
- `pnpm dev` 由 `node` 监听 3000，首页 HTTP 200，标题为 `网络代理工具`；
- `pnpm tauri:dev` 继续使用 `src-tauri/tauri.dev.conf.json`，`BeforeDevCommand` 为 `pnpm dev`；
- Tauri CLI、Next.js 分别由 Node 运行，Rust Host 启动，CoreGraphics 再次确认同一主窗口为
  1440 x 900。

## 静态检查

```bash
python3 -m json.tool deno.json
python3 -m json.tool src-tauri/tauri.deno.conf.json
deno fmt --check deno.json src-tauri/tauri.deno.conf.json
git diff --check -- README.md docs/architecture/development-guide.md \
  docs/tasks/pending/2026-08-31/support-node-and-deno-development-toolchains.md
git diff --exit-code -- package.json pnpm-lock.yaml \
  src-tauri/tauri.conf.json src-tauri/tauri.dev.conf.json
```

全部退出码为 0。最后一项证明本任务未修改原 Node/pnpm 依赖合同与原 Tauri 配置。

## N/A / NOT_RUN

- lint、typecheck、Vitest、完整 `pnpm check`：`NOT_RUN`，超出本任务的开发启动范围。
- release bundle、CI、Windows/Linux/Android：`NOT_RUN`，超出本任务范围。
- WebDriver UI 自动化：`N/A`，macOS Tauri 无官方 WebDriver 驱动；使用真实进程、HTTP 与
  CoreGraphics on-screen window 共同验证。

## 复测入口

Node.js + pnpm：

```bash
pnpm install --frozen-lockfile
pnpm dev
pnpm tauri:dev
```

Deno-only：把 PATH 收窄到 Deno、Rust 和系统工具后执行：

```bash
deno install --allow-scripts
deno task dev
deno task tauri:dev
```
