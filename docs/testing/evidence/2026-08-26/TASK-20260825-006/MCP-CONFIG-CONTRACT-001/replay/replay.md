# Fresh checkout 复测命令

从包含 G033 变更与本证据的 fresh checkout 仓库根目录依次执行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy 'mcp::tests'
cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application -p intercept-proxy --all-targets --all-features -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
pnpm scan:architecture
pnpm scan:source-size
```

定向复测 RED 后关闭的三个合同缺口：

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application --test environment_configuration_negative_contract
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application --test environment_configuration_document_contract
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy environment_configuration_schema_contract
```

成功标准见 `../steps/success.md`。网络、UI、数据库、设备、CI、Push 和发布不在本用例内。
