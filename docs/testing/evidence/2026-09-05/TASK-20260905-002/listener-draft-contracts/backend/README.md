# TASK-20260905-002 B1 Listener Draft backend contracts

## Purpose

Verify the application-owned Listener editing DTO, pure Rust transitions, incomplete-field validation, three-command Tauri surface, and generated TypeScript contract without changing the strict domain model or adding a second save path.

## Environment and tested state

- Repository: `/Users/codin/Code/gmofg-payment-proxy`
- Git source baseline: `c7a70d3ef63719ae3bb7f9b3e6dca8db995c6525`
- Rust: `rustc 1.98.0`, `cargo 1.98.0`
- Deno: `2.9.6`, Apple Silicon macOS
- Final execution window: 2026-09-05 15:27:59 through 15:54:12 +08:00
- The shared worktree was dirty before B1 because TASK-20260905-001 and other assigned lanes were active. B1 verification commands ran after this lane stopped editing its tested Rust and generated-binding files.

## Inputs and expected behavior

- `inputs/contract-scenarios.json` is the task-level scenario inventory copied as actually consulted by B1.
- `inputs/backend-contract-input.json` records the three APIs, six transition intents, incomplete fields, and validation order.
- `inputs/expected.json` records the directly asserted results.
- The active automated test resources used for GREEN are snapshotted under `resources/after/`; `SHA256SUMS.txt` identifies their exact content.
- The RED command ran against the first version of `requirements_tests/listener_draft.rs`. That file was then extended and split under the 500-line gate; the final archived tests are a strict superset. The contemporaneous compiler output is retained in `outputs/red-application-tests.log`; no reconstructed file is presented as an original snapshot.

## Steps and commands

1. Add the Listener Draft requirement tests before production types or methods existed.
2. Run:
   `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application requirements_tests::listener_draft --no-fail-fast`
   Result: expected RED, exit 101, unresolved ListenerDraft/transition/API symbols.
3. Implement application DTO normalization, pure transitions, CAS-first validation, three thin Tauri commands, and regenerate bindings using:
   `cargo run --manifest-path src-tauri/Cargo.toml --bin export-bindings --features export-bindings`
4. Re-run the application contract command. Final result: 12 passed, 0 failed.
5. Run:
   `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy commands::listener::tests::generated_bindings_expose_only_the_three_draft_commands --no-fail-fast`
   Result: 1 passed, 0 failed.
6. Re-run the existing strict listener-validation regressions with:
   `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application listener_validation --no-fail-fast`
   Result: 2 passed, 0 failed.
7. Run:
   `deno run -A scripts/check-generated-bindings.mjs`
   Result: generated bindings fresh and deterministic across two release generations.
8. Run application and Tauri clippy with `-D warnings`, Rust formatting check, scoped `git diff --check`, and the source 500-line gate.

The exact repeatable commands are in `replay/run.sh`.

## Actual result

PASS. `outputs/actual.json` and `outputs/comparison.json` summarize the machine-checked outcome. The tests prove:

- complete strict HTTP and Socket listeners round-trip exactly through Draft;
- copy changes only ID, trimmed-suffix name, and enabled state;
- Rust defaults drive data-plane, HTTP response, Socket topology, processing, and TLS transitions;
- App and Server TLS remain independent, LocalResponder removes Server state, and an existing exact package is not replaced by a recommendation;
- wrong-data-plane transitions return `LISTENER_DRAFT_TRANSITION_NOT_APPLICABLE`;
- null is accepted only for the approved incomplete fields while malformed enums, UUIDs, and nonempty invalid package identities fail deserialization;
- workspace lookup/revision CAS precede Draft field errors;
- incomplete Draft and strict-domain-invalid candidates both return `normalized: null`; valid candidates return `normalized: ProxyWorkspace`;
- existing `listener_save` remains the only save transaction, with no draft-new or draft-save IPC.

## Evidence map

- RED: `outputs/red-application-tests.log`
- Final application GREEN: `outputs/green-application-tests-final-v2.log`
- Command/binding GREEN: `outputs/green-command-bindings-test.log`
- Existing strict listener-validation regression: `outputs/existing-listener-validation-regression.log`
- Deterministic binding check: `outputs/generated-bindings-check.log`
- Static analysis: `outputs/clippy-application-final.log`, `outputs/clippy-tauri.log`, `outputs/static-checks.log`
- Intermediate failed clippy diagnostics and corrected rerun: `outputs/clippy-application.log`, `outputs/clippy-application-final.log`
- Final source/generated snapshots: `resources/after/`

## N/A

Protocol frames, TCP chunks, network server input/output, screenshots, certificate bytes, external services, real devices, CI, deployment, and cleanup are N/A because B1 is a local application/IPC type and pure-transition change. No CI, push, commit, runtime listener, certificate store, or lease operation was executed.
