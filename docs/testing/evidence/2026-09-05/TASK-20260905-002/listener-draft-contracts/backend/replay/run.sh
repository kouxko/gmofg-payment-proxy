#!/bin/sh
set -eu
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application requirements_tests::listener_draft --no-fail-fast
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy commands::listener::tests::generated_bindings_expose_only_the_three_draft_commands --no-fail-fast
deno run -A scripts/check-generated-bindings.mjs
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
