# Shared Socket runtime plan configuration

- Task: `TASK-20260905-004`
- Case: `socket-plan`
- Verified at: `2026-09-05 23:47:17 +08:00`
- Environment: Darwin arm64; rustc 1.98.0; cargo 1.98.0

## Purpose

Prove that Direct and Scripted Socket listeners use one topology-aware configuration builder for Relay and LocalResponder endpoints. The shared builder must preserve the selected TLS mode, maximum connections, read chunk size, connect/handshake timeout, read timeout, write timeout, upstream endpoint, and existing error mapping. Scripted assembly adds only its protocol capability factory and pipeline limits after the common endpoint configuration is prepared.

The upstream probe remains on its existing separate path. This preserves the contract that probing a Relay does not load downstream TLS identity and that LocalResponder has no applicable upstream probe.

## Input and expected behavior

The mapping regression uses a listener with connect/read/write timeouts of `101/202/303 ms`, maximum connections `17`, read chunk size `4097`, bind address `127.0.0.1:19090`, and Relay upstream `upstream.example:443`. It constructs Relay and LocalResponder runtime configs from one `SocketCommonConfig` and requires all shared fields to be identical. The Relay must retain Transparent security and the LocalResponder must retain TCP security.

Existing runtime cases cover:

- Direct Relay actual TCP byte forwarding, metrics, stop, and port release.
- Direct Relay TLS reference selection and fail-closed unmanaged trust.
- LocalResponder exact scripted package requirement and no-upstream probe error.
- Probe behavior that does not resolve downstream TLS identity.
- Scripted LocalResponder and Relay exchanges through both protocol directions.
- External package frame size failure and listener recovery behavior.

## Baseline

Before the refactor, the existing Listener Runtime suite passed `54/54`. Direct and Scripted plan files each independently constructed Relay and LocalResponder runtime configs, including TLS translation, limits, endpoint, and timeout fields.

## Implementation

`plan/socket_config.rs` owns the immutable common values and the only Direct/Scripted topology-to-runtime mapping. `plan/socket.rs` selects raw Relay or raw LocalResponder service construction. `plan/scripted.rs` selects scripted Relay or scripted LocalResponder construction and supplies only protocol capabilities and limits. No dependency, fallback, default value, probe path, runtime service API, or error code was changed.

## GREEN verification

Commands and results:

```text
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure common_socket_settings_map_identically_to_relay_and_local_configs --lib
# 1 passed; 0 failed

cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure adapters::listener_runtime --lib
# 84 passed; 0 failed; 391 filtered out

cargo check --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure
# PASS

cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --lib --tests -- -D warnings
# PASS

rustfmt --edition 2024 <owned Rust files>
git diff --check -- <owned Rust files>
# PASS
```

Concise command output is stored under `outputs/`. The final source search found `SocketRelayConfig` and `SocketLocalResponderConfig` construction only in `plan/socket_config.rs`; the platform-range error text for read chunk conversion also has one implementation in that module.

## Result and N/A

- Result: `PASS`.
- Actual TCP traffic is covered by the existing localhost Direct and Scripted runtime tests. No real external server, public network, or payment transaction was used.
- TLS material files: N/A. Existing fail-closed reference-selection tests use in-memory/unmanaged references; no private key material was created or archived.
- UI and screenshots: N/A; no UI behavior changed.
- CI, commit, push, and publication: NOT_RUN; outside this assigned slice.
- File resources: N/A; inputs are typed Rust configuration values and existing in-repository tests.

## 2026-09-06 source-size follow-up

`socket_observer` and its `SocketDiagnosticObserver` import were moved from `plan.rs` into the existing `plan/socket_config.rs` implementation. The method body, conversion checks, error codes, messages, observer inputs, and `Arc` ownership are unchanged. This places Socket diagnostic configuration beside the shared Socket endpoint and limit configuration.

Fresh results at `2026-09-06 00:56:45 +08:00`:

```text
cargo check --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --lib
# PASS

wc -l src-tauri/crates/infrastructure/src/adapters/listener_runtime/plan.rs src-tauri/crates/infrastructure/src/adapters/listener_runtime/plan/socket_config.rs
# 473 plan.rs; 210 socket_config.rs

git diff --check -- src-tauri/crates/infrastructure/src/adapters/listener_runtime/plan.rs src-tauri/crates/infrastructure/src/adapters/listener_runtime/plan/socket_config.rs
# PASS
```

The focused Listener Runtime test command was attempted again, but the test target did not compile while another assigned agent was actively splitting unrelated HTTP phase-10 tests. The compiler failures are confined to `tests/phase10_http_pipeline/unified_working_exchange.rs` and `live_plain_hot_replacement.rs`; neither file belongs to this source-size follow-up. The exact output is archived in `outputs/source-size-listener-test.log`. Stable Listener integration testing remains an integration-owner follow-up after the concurrent test edits reach a terminal state.
