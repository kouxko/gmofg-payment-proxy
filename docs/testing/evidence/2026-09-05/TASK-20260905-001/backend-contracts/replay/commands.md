# Replay commands

Run from the repository root.

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application bootstrap_cursor_replays_events_published_while_snapshot_is_read -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime forward_normalization_removes_connection_declared_and_proxy_hop_headers -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime --test upstream_http upstream_is_http11_single_use_host_rewritten_and_redirect_not_followed -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime --test fault_semantics -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime --test raw_http_proxy chunked -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application --lib requirements_tests::events:: -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime --lib message::tests:: -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime --test upstream_http -- --nocapture
cargo check --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application -p intercept-proxy-runtime
cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application -p intercept-proxy-runtime --all-targets -- -D warnings
```
