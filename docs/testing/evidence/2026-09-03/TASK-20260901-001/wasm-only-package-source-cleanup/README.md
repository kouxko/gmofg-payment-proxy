# Wasm-only 协议包源码收敛验证

- 任务：`TASK-20260901-001`
- 用例：`wasm-only-package-source-cleanup`
- 执行时间：`2026-09-03 11:06:44 +08:00`
- 结果：`PASS`
- 被测状态：提交 `6a2c7a824214730fa650afe8b0c38aeac634d8f9` 上本次未提交的非 Wasm 重复实现删除、Nuvei fixture 迁移、活动说明和验证清单更新。
- 派生自：`TASK-20260901-001` / `wasm-integrated-runtime` / `docs/testing/evidence/2026-09-01/TASK-20260901-001/wasm-integrated-runtime/`

## 目的

验证仓库中的 AU EFTEX、ISO8583 Deno ASCII、Nuvei Tango JSON 和 Nuvei Tango JSON Rhai 四组包已删除 Python、Deno/TypeScript、Rhai/ZIP 重复实现，同时五个 Rust WebAssembly Component 仍可统一构建并由正式 Host 加载、执行现有回归向量。

## 删除与保留边界

- 删除：三组 Python/Deno 源码、依赖清单和语言级测试；Rhai 源码、TOML Manifest、ZIP 构建器、历史 ZIP 及其旧 Host/oracle 测试；仅服务于 AU Python 实现的 trace verifier。
- 保留：五个 Rust Component、其 Manifest/Cargo 锁文件/测试、通用 `/packages` 外部进程接入实现与测试、历史任务和不可变测试证据。
- 迁移：Nuvei Rhai 合成 request/response JSON 从旧 Rhai Host 测试目录移入 `component/tests/fixtures/`，Component 单测和正式 Host 集成测试改读新路径。
- 本机忽略产物：旧 Python venv/cache、Rhai 旧测试 target 和历史 ZIP 被移出仓库到 `/tmp/gmofg-non-wasm-packages.IGCRBI`，约 `542 MiB`，可在本机临时目录仍存在时恢复。

## 执行与结果

1. `pnpm test:protocol-packages`
   - `PASS`：五个 Component 的局部测试合计 `20/20`；全部重新构建为 `wasm32-wasip2` 单文件产物。
   - 构建索引精确包含 5 项：AU EFTEX、ISO8583 Deno ASCII、Nuvei Tango JSON、Nuvei Tango JSON Rhai、ISO8583 ASCII Standard。
2. `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-package-runtime --test repository_components`
   - `PASS 2/2`：五个仓库 Component 均可由正式 Host 构建、加载和回放；AU EFTEX 公开旧向量通过。
3. `pnpm typecheck` 与 `pnpm lint`
   - `PASS`：删除 Deno 目录后的 TypeScript 配置和全仓 ESLint 均通过。
4. `deno task scan:source-size`
   - `PASS`：手写源码文件行数门禁通过。
5. `cargo fmt ... --check`、`git diff --check`
   - `PASS`：Rust 格式和补丁空白检查通过。
6. 对四个包目录扫描 `.py`、`.ts`、`.rhai`、`.zip`、`deno.json`、`pyproject.toml` 及旧 TOML 包文件。
   - `PASS`：Component 目录之外零匹配；保留目录只含 README 与 Rust Component 内容。

## 验收判断

- `PASS`：用户点名的五个包在活动源码中只保留 Rust WebAssembly Component。
- `PASS`：Nuvei 合成回归资源未丢失，Component 与 Host 两层验证通过。
- `PASS`：活动文档和发布验证矩阵不再要求已删除的 Python/Deno/Rhai 命令。
- `N/A`：本次不改变协议、Schema、运行时、数据库或 Proxy 生产代码，无需网络抓包、数据库快照或 UI 截图。
- `NOT_RUN`：完整 workspace 全量测试、远端 Windows 和远程 CI；本次删除由五包统一构建、正式 Host 集成、lint/typecheck 和静态扫描直接覆盖。

## 复测

从仓库根目录依次运行：

```bash
pnpm test:protocol-packages
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-package-runtime --test repository_components
pnpm typecheck
pnpm lint
deno task scan:source-size
```
