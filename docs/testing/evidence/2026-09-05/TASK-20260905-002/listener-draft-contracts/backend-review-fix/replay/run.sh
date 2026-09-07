#!/bin/sh
set -eu
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application requirements_tests::listener_draft --no-fail-fast
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application listener_validation --no-fail-fast
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy commands::listener::tests::generated_bindings_expose_only_the_three_draft_commands --no-fail-fast
cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application --tests -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
