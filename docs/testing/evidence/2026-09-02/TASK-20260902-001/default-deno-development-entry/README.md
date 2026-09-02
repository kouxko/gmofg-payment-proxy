# 默认 Deno 开发入口与 Node 兼容入口验证

## 目的与结果

- 任务：`TASK-20260902-001`
- 用例：`default-deno-development-entry`
- 执行时间：`2026-09-02 10:27:05 +08:00` 至 `2026-09-02 10:33:48 +08:00`
- 结果：`VERIFIED_WITH_APP_STATE_BLOCKER`
- 派生自：`TASK-20260831-001` / `dual-toolchain-development-smoke` /
  `docs/testing/evidence/2026-09-02/TASK-20260831-001/dual-toolchain-development-smoke/README.md`

验证基础 Tauri 配置已经把 Deno 设为默认开发前置命令，同时 Node.js + pnpm overlay 继续明确使用
`pnpm dev`。两套入口均实际启动 Next.js、返回首页 HTTP 200 并进入 Rust App setup；当前本机已有的
吊销测试 Root CA 状态让两套入口都在同一 setup 边界以 `CERTIFICATE_ROOT_REVOKED` 停止，因此本次
不把主窗口显示记为通过，也不擅自清除用户应用数据。

## 被测配置

当次实际文件快照保存在 `resources/`：

- `deno.json`、`deno.lock`；
- `package.json`、`pnpm-lock.yaml`；
- `tauri.conf.json`、`tauri.dev.conf.json`、`tauri.deno.conf.json`。

静态合并检查确认：

```text
base.beforeDevCommand=deno task dev
node_overlay.beforeDevCommand=pnpm dev
deno_overlay.beforeDevCommand=INHERITED_FROM_BASE
base.beforeBuildCommand=pnpm build
```

## Deno 默认入口

执行：

```bash
deno task tauri:dev
```

实际观察：

- Tauri 输出 `Running BeforeDevCommand (deno task dev)`；
- Next.js 16.2.12 webpack server Ready，Deno 监听 3000 端口；
- 首页返回 HTTP 200、31678 bytes，标题为 `网络代理工具`；
- Rust debug build 完成并运行 `intercept-proxy`；
- App setup 读取到当前本机旧测试 Root CA 已吊销，返回 `CERTIFICATE_ROOT_REVOKED`，未显示主窗口。

## Node.js + pnpm 兼容入口

执行：

```bash
pnpm tauri:dev
```

实际观察：

- Tauri 输出 `Running BeforeDevCommand (pnpm dev)`；
- Next.js 16.2.12 webpack server Ready；
- Rust debug build 完成并运行同一 `intercept-proxy`；
- App setup 返回与 Deno 入口相同的 `CERTIFICATE_ROOT_REVOKED`。

相同业务 setup 失败说明当前主窗口阻塞来自本机已有应用/证书状态，而不是默认工具链选择。本用例没有
删除配置、数据库、钥匙串证书或其他应用数据。父用例在较早且未触发该状态的环境中已经分别验证两套
工具链显示 1440 x 900 主窗口，但该历史结果不替代本次环境结论。

## 静态与清理检查

以下检查通过：

```bash
python3 -m json.tool src-tauri/tauri.conf.json
python3 -m json.tool src-tauri/tauri.dev.conf.json
python3 -m json.tool src-tauri/tauri.deno.conf.json
deno fmt --check deno.json src-tauri/tauri.conf.json \
  src-tauri/tauri.dev.conf.json src-tauri/tauri.deno.conf.json
git diff --check
cmp package.json <父证据>/resources/package.json
cmp pnpm-lock.yaml <父证据>/resources/pnpm-lock.yaml
cmp deno.json <父证据>/resources/deno.json
cmp deno.lock <父证据>/resources/deno.lock
```

用户授权的两个临时测试路径已通过 macOS `/usr/bin/trash` 移入废纸篓，原路径不存在：

```text
/tmp/task-20260831-deno-snapshot.d5X0GX
/tmp/task-20260831-deno-bin.A8bByt
```

## N/A / NOT_RUN

- 主窗口：`INCONCLUSIVE`，两套入口均被当前本机 `CERTIFICATE_ROOT_REVOKED` 状态阻断。
- lint、typecheck、Vitest、完整 `pnpm check`：`NOT_RUN`，未修改业务代码且超出本任务范围。
- release bundle、CI、Windows/Linux/Android：`NOT_RUN`，超出本任务范围。
- 证书/应用数据清理：`NOT_RUN`，不属于本任务且会改变用户现有应用状态。

## 复测入口

```bash
deno install --allow-scripts
deno task tauri:dev

pnpm install --frozen-lockfile
pnpm tauri:dev
```
