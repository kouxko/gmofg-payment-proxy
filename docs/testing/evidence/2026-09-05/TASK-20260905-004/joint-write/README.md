# Unified HTTP write preparation and completion

- Task: `TASK-20260905-004`
- Case: `joint-write`
- Executed: `2026-09-06 01:50:27 +08:00`
- Result: `PASS`

## Purpose

Verify that Rules and protocol Encode form one write-preparation transaction, HTTP writers consume the prepared message once, and write completion distinguishes a flushed request from an early readable response. A failed joint Encode, empty Socket output, or oversized Socket output must preserve the typed error location, original bytes, and uncommitted rule lifecycle.

## Commands and results

1. `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime http::exchange_runtime::endpoints::tests -- --nocapture`
   - PASS: 6 passed, 0 failed. Covers effective request/response ownership, Local echo, flush-before-response, legal early response, and typed external failure conversion. See `outputs/proxy-endpoint-tests.txt`.
2. `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure production_snapshot_uses_shared_provider_for_both_directions_and_joint_encode -- --nocapture`
   - PASS: 1 passed, 0 failed. The failure branch asserts code `EXTERNAL_PACKAGE_CALL_FAILED`, stage `Encode`, direction `Upstream`, package stable code `BODY_ENCODE_FAILED`, unchanged message, zero commit attempts, and unchanged rule snapshot. See `outputs/joint-encode-rollback.txt`.
3. `cargo check --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --tests`
   - PASS. See `outputs/infrastructure-check-tests.txt`.
4. `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime http::exchange_runtime:: -- --nocapture`
   - PASS: 9 passed, 0 failed after the final typed producer codes were added. See `outputs/proxy-exchange-runtime-tests.txt`.
5. `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure socket_encode_failure_rolls_back_lifecycle_before_successful_commit -- --nocapture`
   - PASS: 1 passed, 0 failed. The transaction covers an explicit typed Encode failure, `EMPTY_OUTPUT`, `OUTPUT_LIMIT_EXCEEDED`, and a valid output. Each invalid result preserves the original `original-frame` bytes and leaves commit attempts and rule hit count at zero; only the valid output commits one hit. See `outputs/socket-bounded-transaction.txt`.
6. `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure socket_frame_waits_for_listener_gate_before_reading_rule_generation -- --nocapture`
   - PASS: 1 passed, 0 failed. The joint Socket preparation still holds the listener transaction while taking the rule generation snapshot after receiving the new limits dependency. See `outputs/socket-joint-prepare-bounded.txt`.
7. `cargo check --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --tests`
   - PASS after the bounded Socket change. See `outputs/infrastructure-check-tests-final.txt`.

## RED/GREEN record

- Typed error preservation was first RED because the Socket joint Encode path returned no `exchange_code`; see `outputs/socket-typed-error-red.txt`. The common typed conversion then made the same regression GREEN; see `outputs/socket-typed-error-green.txt`.
- The empty and oversized output cases were added after independent review exposed that the outer `BoundedPrepare` ran after lifecycle commit. Their first recorded execution was GREEN after moving the shared validation into joint Encode. No behavioral RED execution exists for these two cases, so this evidence does not claim one.

## Source snapshots

- `resources/socket-output-validation.rs`: the single output validator shared by joint Encode and the outer capability boundary.
- `resources/joint-socket-prepare.rs`: the limits-aware joint Socket preparation used by the external package path.
- `resources/socket-joint-transaction-test.rs`: the exact typed error, empty output, oversized output, rollback, and success assertions used for this run.

## N/A / not run

- UI, screenshot, device, external payment service and TLS: N/A for this contract and localhost transport slice.
- Full workspace test and strict clippy: delegated to the root agent after this stable integration point.
- CI, commit, push and publication: outside this assigned slice.
