# HTTP and Socket rule-program compilation

- Task: `TASK-20260905-004`
- Case: `rule-compilation`
- Executed: `2026-09-05 23:47:39 +08:00` through `2026-09-05 23:53:25 +08:00`
- Environment: Darwin 27.0.0 arm64; rustc 1.98.0; cargo 1.98.0

## Purpose and preserved contracts

Remove the duplicated optional Document Schema validation and `RuleProgramEntry` construction from HTTP and Socket direction-program compilation while preserving their distinct selection contracts:

- HTTP retains `owns_all_http` and per-direction Document condition/action ownership detection.
- Socket retains exact protocol-package binding validation and its existing mismatch code, message, and rule entity.
- Missing Schema remains schema-free for both protocols.
- Rule identity, priority, created order, condition, action, and final `UnifiedRuleProgram` sorting remain unchanged.

No new behavior or dependency was introduced. The existing direction filters, topology parameter, external interfaces, and program finalization stay in their original modules.

## Baseline

Command before refactoring:

```text
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure absent_ -- --nocapture
```

Result: 2 passed, 0 failed. This covered both existing schema-free HTTP and Socket direction compilers. See `outputs/baseline.log`.

An initial Cargo invocation attempted to provide two positional test filters and exited 1 because Cargo accepts one `TESTNAME`; it made no code or repository changes. The corrected `absent_` filter above is the actual baseline.

## Refactor

`rule_program_compilation::compile_rule_entry` now owns the shared sequence:

1. If a Schema is present, validate the Document condition.
2. Validate the unified action against the same Schema.
3. Build the entry from the definition's identity, priority, creation order, condition, and action.

Both callers append the returned entry and keep their existing final `UnifiedRuleProgram::new` call. This extracts one repeated responsibility without creating a generic compiler framework or moving protocol-specific selection into a shared abstraction.

## Verification

```text
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure absent_ -- --nocapture
# 2 passed; 0 failed

cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure adapters::listener_runtime::external_relay::contract::tests::replace_document_rules_installs_new_rules_for_the_running_snapshot -- --exact --nocapture
# 1 passed; 0 failed

cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure adapters::listener_runtime::tests::phase10_http_pipeline_tests::unified_working_exchange::schema_free_http_hot_replace_keeps_root_rule_program -- --exact --nocapture
# 1 passed; 0 failed

cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --lib
# 475 passed; 0 failed

cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --lib -- -D warnings
# PASS
```

`cargo fmt` was applied only to the four owned Rust files. Full result summaries are under `outputs/`.

## Result and N/A

- Result: `PASS`.
- Network packets, TLS, external package processes, UI, screenshots, device, and real payment flow: N/A. The refactor preserves pure compilation behavior and was exercised by deterministic unit/runtime-contract tests.
- External files and fixtures: N/A. Existing typed Rust tests construct all inputs.
- CI, commit, push, and publication: NOT_RUN; outside this assigned slice.

