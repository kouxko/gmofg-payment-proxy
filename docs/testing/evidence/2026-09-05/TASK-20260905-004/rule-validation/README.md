# Unified HTTP action validation

- Task: `TASK-20260905-004`
- Case: `rule-validation`
- Executed: `2026-09-05 23:42:34 +08:00` through `2026-09-05 23:45:50 +08:00`
- Environment: Darwin 27.0.0 arm64; rustc 1.98.0; cargo 1.98.0

## Purpose

Prove that an HTTP rule's condition kind does not disable validation of its HTTP or Terminal action. `Condition::Document` and `Condition::DocumentPattern` must use the same stage compatibility, numeric limit, header content, and Terminal compatibility validator already used by `Condition::Http`.

## Input and expected behavior

The Domain regression covers these invalid combinations:

- Document condition + request-stage `CustomHttpStatus`.
- DocumentPattern condition + request-stage downstream `Throttle`.
- Document condition + zero-millisecond `Delay`.
- DocumentPattern condition + managed `Content-Length` header.
- Document condition + response-stage `DisconnectBeforeUpstream` Terminal action.

Each combination must return `RULE_INVALID` with the relevant `action` field error. Valid upstream throttle and downstream custom-status combinations remain accepted. The Application regression saves a Document condition with `Delay { milliseconds: 0 }`; it must return `RULE_INVALID` and leave the Workspace unchanged.

## RED reproduction

Commands:

```text
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-domain --test unified_rule_contract document_conditions_do_not_bypass_http_action_validation -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application requirements_tests::unified_rules::unified_save_rejects_every_invalid_http_runtime_shape_without_persistence -- --exact --nocapture
```

Actual result before the production change: both commands failed. Domain accepted the response-only status on a request-stage Document rule; Application returned a successfully persisted zero-delay Document rule where the test required an error. See `outputs/red-domain.log` and `outputs/red-application.log`.

## Minimal fix

`validate_http_runtime_content` no longer returns early for non-HTTP conditions. HTTP and Terminal actions are converted to the existing `HttpAction` representation and passed to the single existing `validate_http_rule` validator. Document and RecordMatch actions retain their existing behavior. No second action validator or Application production validation was added.

## GREEN verification

Commands and results:

```text
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-domain --test unified_rule_contract -- --nocapture
# 11 passed; 0 failed

cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application --lib requirements_tests::unified_rules:: -- --nocapture
# 5 passed; 0 failed

cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-domain --test unified_rule_contract -- -D warnings
# PASS

cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application --lib -- -D warnings
# PASS

cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-domain
# 102 passed; 0 failed (70 lib + 15 phase5 + 6 phase6 + 11 unified contract); 0 doc tests

cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application --lib
# 435 passed; 0 failed
```

Full stdout is stored under `outputs/`. Formatting was applied only to the three owned Rust files with `cargo fmt --manifest-path src-tauri/Cargo.toml -- <paths>` and completed successfully.

## Result and N/A

- Result: `PASS`.
- Network, TCP frames, TLS, runtime Listener, UI, screenshots, external service, and real payment flow: N/A. This case changes deterministic Domain save validation and verifies the Application persistence boundary without network I/O.
- CI, commit, push, and publication: NOT_RUN; outside this assigned slice.
- File resources: N/A. The inputs are typed Rust test values declared in the referenced regression tests; no external fixture is used.
