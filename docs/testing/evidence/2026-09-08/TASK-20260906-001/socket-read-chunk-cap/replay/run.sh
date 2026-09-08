#!/bin/sh
set -eu
cd "$(git rev-parse --show-toplevel)"
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-domain -p intercept-proxy-application -p intercept-proxy-infrastructure --lib socket_read_chunk_cap
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-domain -p intercept-proxy-application -p intercept-proxy-infrastructure --all-targets --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
deno task check:bindings
deno task scan:architecture
deno task scan:source-size
deno task typecheck
