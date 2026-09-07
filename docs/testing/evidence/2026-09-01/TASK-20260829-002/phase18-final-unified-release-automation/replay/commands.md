# Phase18 复测命令

从仓库根目录按依赖顺序执行：

```bash
pnpm check:bindings
pnpm typecheck
pnpm lint
pnpm test
pnpm scan:architecture
pnpm scan:source-size
cargo check --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features
node --test scripts/check-task-20260829-002-phase18-packaging.test.mjs
pnpm build:macos:universal
git diff --check
```

人工验收不在本次复测命令中：GUI/computer-use、系统/网络权限、Windows、Android、Developer ID签名/公证、CI、push和release均保持`NOT_RUN`。
