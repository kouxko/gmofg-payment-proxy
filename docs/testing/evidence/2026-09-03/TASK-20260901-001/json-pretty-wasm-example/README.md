# JSON Pretty Wasm 示例包验证

- 任务：`TASK-20260901-001`
- 用例：`json-pretty-wasm-example`
- 执行时间：`2026-09-03 10:46:55 +08:00`
- 结果：`LOCAL_APP_VERIFIED`
- 被测状态：提交 `07ec632581b369d9255a806bd12ab07e918770ed` 上本次未提交的 JSON Pretty、HTTP nullable Schema、HTTP Frame capability 和安全 Display 样式修改。
- 派生自：`TASK-20260901-001` / `wasm-integrated-runtime` / `docs/testing/evidence/2026-09-01/TASK-20260901-001/wasm-integrated-runtime/`

## 目的

交付不声明 Schema 的 `json-pretty@1.0.0` HTTP Wasm Component，并保证 Proxy 的导入、详情和 Listener 目录均接受合法 HTTP `null` Schema。Display 输出缩进 JSON 和编辑器式类型分色，同时只允许经过属性和值白名单过滤的内联视觉 CSS。

## 根因与修复

- Rust 包合同允许 HTTP `document.schema = None`，但前端导入预览/结果、详情和 Listener 目录曾无条件要求 Schema 对象。
- 后端曾把 HTTP/Socket 都投影为 `frame: true`，与前端 HTTP `frame: false` 合同冲突。
- 修复后 HTTP 各方向可独立使用 `null` 或合法 Schema，Socket 仍要求双向合法 Schema；HTTP 为 `frame: false`，Socket 为 `frame: true`。
- Display iframe 继续使用空 sandbox 和 deny-by-default CSP；仅复制白名单内联视觉属性，删除脚本、事件、外链资源、`<style>` 和越界 CSS。

## 资源

- `resources/source/`：实际构建使用的包源码、Manifest、锁文件、构建器和说明快照。
- `outputs/json-pretty-1.0.0.wasm`：最终单文件 Component。
- `outputs/json-pretty-1.0.0.wasm.sha256`：最终产物 SHA-256。
- `outputs/validation-summary.txt`：本次验证摘要。

## 执行与结果

1. `deno run -A examples/protocol-packages/json_pretty/build.mjs`
   - Rust 单元测试 `4/4 PASS`；`wasm32-wasip2 --release` 构建与严格 Manifest section 校验通过。
   - 最终产物 `161042` bytes，SHA-256 `5b7ebda09f3c71c79837e4df6447bafd04a92a57e5421ef283d25f152b897ee3`。
2. 协议包导入/详情/Display 定向 Vitest：`20/20 PASS`。
3. Listener 目录、HTTP 入口与页面合同定向 Vitest：`49/49 PASS`。
4. `pnpm typecheck`：`PASS`。
5. `deno task tauri:dev`：Rust 开发构建完成，前端 `GET /` 返回 `200`。
6. 用户在重新启动的本地 App 中确认无 Schema HTTP 包导入、入口协议包目录读取和使用结果“可以了”。
7. Rust HTTP Manifest 导入投影定向测试：`1/1 PASS`；HTTP fixture 上行无 Schema、下行有 Schema，双向均为 `frame: false`。

## 验收判断

- `PASS`：无 Schema HTTP 包导入、详情与 Listener 目录合同一致，Socket 严格性保持。
- `PASS`：JSON Pretty Component 构建、类型分色和 HTML 转义通过。
- `PASS`：安全内联视觉 CSS 可用，主动内容与非白名单 CSS 仍被删除。
- `PASS`：当前本地开发 App 用户验收。
- `NOT_RUN`：远端 Windows 复测、完整仓库测试与远程 CI 不属于本次本地提交门禁。

## 复测

按 `replay/commands.md` 从仓库根目录执行，然后在协议包页面导入本用例 `outputs/` 中的 Wasm，并在 HTTP 入口配置中选择该包。
