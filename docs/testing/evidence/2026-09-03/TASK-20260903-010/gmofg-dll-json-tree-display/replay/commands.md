# 复测命令

从仓库根目录 `/Users/codin/Code/gmofg-payment-proxy` 执行：

```bash
cargo fmt --manifest-path examples/protocol-packages/gmofg_payment_dll/Cargo.toml -- --check
cargo clippy --locked --all-targets \
  --manifest-path examples/protocol-packages/gmofg_payment_dll/Cargo.toml -- -D warnings
cargo test --locked --all-targets \
  --manifest-path examples/protocol-packages/gmofg_payment_dll/Cargo.toml
deno check examples/protocol-packages/gmofg_payment_dll/build.mjs
deno run -A examples/protocol-packages/gmofg_payment_dll/build.mjs

rustfmt --edition 2024 --check \
  src-tauri/crates/package-runtime/tests/gmofg_payment_dll_component.rs
cargo clippy --manifest-path src-tauri/Cargo.toml \
  -p intercept-proxy-package-runtime --test gmofg_payment_dll_component -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml \
  -p intercept-proxy-package-runtime --test gmofg_payment_dll_component

deno run --no-lock -A npm:vitest@4.1.11 run \
  src/features/shared/protocol-safe-display.test.tsx
deno run --no-lock -A npm:eslint@9 \
  src/features/shared/protocol-safe-display.tsx \
  src/features/shared/protocol-safe-display.test.tsx
deno task typecheck
deno task build

shasum -a 256 \
  examples/protocol-packages/gmofg_payment_dll/dist/gmofg-payment-dll-1.0.0.wasm
cmp -s \
  examples/protocol-packages/gmofg_payment_dll/dist/gmofg-payment-dll-1.0.0.wasm \
  docs/testing/evidence/2026-09-03/TASK-20260903-010/gmofg-dll-json-tree-display/outputs/gmofg-payment-dll-1.0.0.wasm
git diff --exit-code -- deno.lock
git diff --check
```
