#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --lib manual_recovery -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure -p intercept-proxy-application --lib
cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure -p intercept-proxy-application --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure -p intercept-proxy-application -- --check
deno run -A scripts/check-runtime-architecture.mjs
deno run -A scripts/check-architecture-docs.mjs
