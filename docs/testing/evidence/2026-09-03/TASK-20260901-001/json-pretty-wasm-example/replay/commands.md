# 复测命令

从仓库根目录执行：

```bash
deno run -A examples/protocol-packages/json_pretty/build.mjs
cargo fmt --manifest-path examples/protocol-packages/json_pretty/Cargo.toml -- --check
cargo clippy --locked --all-targets \
  --manifest-path examples/protocol-packages/json_pretty/Cargo.toml -- -D warnings
deno check examples/protocol-packages/json_pretty/build.mjs
pnpm exec vitest run \
  src/features/protocol-packages/protocol-package-import-model.test.ts \
  src/features/protocol-packages/protocol-package-model.test.ts \
  src/features/shared/protocol-safe-display.test.tsx \
  src/features/listeners/socket-listener-model.test.ts \
  src/features/listeners/listeners-view.socket-contracts.test.tsx \
  src/features/listeners/http-protocol-processing-card.test.tsx
pnpm typecheck
cargo test --manifest-path src-tauri/Cargo.toml \
  -p intercept-proxy-infrastructure \
  component_manifest_previews_without_instantiating_guest_exports
cd examples/protocol-packages/json_pretty/dist
shasum -a 256 -c json-pretty-1.0.0.wasm.sha256
```

Host runtime 复测程序在当次测试中位于 `/tmp/json-pretty-verify.Wuk2GL`。它不是归档资源；权威被测源码与产物已经保存在本用例的 `resources/` 和 `outputs/`。如临时目录仍存在，可从仓库根目录执行：

```bash
cargo run --locked \
  --manifest-path /tmp/json-pretty-verify.Wuk2GL/Cargo.toml -- \
  examples/protocol-packages/json_pretty/dist/json-pretty-1.0.0.wasm
```

成功输出：

```text
HOST_RUNTIME_PASS 161042 bytes
```

当前 Proxy 应允许导入该无 Schema HTTP 包，并在 HTTP 入口配置目录中显示该包；Socket 包仍必须提供双向 Schema。
