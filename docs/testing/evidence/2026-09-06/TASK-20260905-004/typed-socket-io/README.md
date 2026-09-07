# Typed Socket I/O verification

## Purpose

Verify that Socket transport failures carry `code`, `stage`, and `direction` as structured fields, and that Raw and protocol exchanges share the same cancellation-first read/write/flush/shutdown implementation without changing Raw half-close behavior or losing partial byte progress.

## Environment

- Executed: `2026-09-06 00:14:33 +08:00`
- OS: `Darwin 27.0.0 arm64`
- Rust: `rustc 1.98.0 (88d9e12ae 2026-08-18)`
- Cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05)`
- Manifest: `src-tauri/Cargo.toml`

## Inputs and expected results

- A message containing misleading `Upstream|READ_TIMEOUT` text must not override structured `ENCODE_FAILED` and Downstream metadata.
- A Raw write that writes seven bytes and then times out must report `WRITE_TIMEOUT`, stage `write`, direction Upstream, and seven progressed bytes.
- A flush failure after five written bytes must report `WRITE_FAILED`, stage `flush`, direction Downstream, and five progressed bytes.
- Existing Socket relay, TLS, LocalResponder, scripted relay, binary traffic, cancellation, timeout, and asymmetric half-close tests must remain green.

## Steps and results

1. Baseline before refactoring: Exchange `25 passed`; Socket relay `58 passed`.
2. Added structured error and partial write/flush regressions first. RED failed to compile because `ErrorStage`, `Error::coded`, and the `code`/`stage`/`direction` fields did not exist. The observed compiler excerpt is saved in `outputs/red.txt`.
3. Replaced message parsing with structured fields and routed Raw/protocol transport operations through `socket_relay/io.rs`.
4. Ran focused structured error and Raw I/O tests; results are saved in `outputs/focused-tests.txt`.
5. Ran the Socket relay suite; `61 passed`, including the existing transparent asymmetric half-close test. Output is saved in `outputs/socket-relay.txt`.
6. Ran scope build/lint verification. `cargo check` passed. Strict Clippy is currently blocked by `SequentialPrepare` missing `Debug` in concurrently modified `exchange/src/pipeline.rs`, outside this subtask's ownership. Output is saved in `outputs/static-checks.txt`. Strict Clippy had passed for this scope before that concurrent shared-file change.

## Result

PASS for the owned typed Socket I/O behavior: focused tests passed `1 + 4`, the Socket relay suite passed `61`, and scope `cargo check` passed. Final strict Clippy and the full Exchange suite remain integration blockers in concurrently modified `exchange/src/pipeline.rs` and its stale writer assertion; both were reported to the parent owner. No HTTP, infrastructure, CI, external service, packet capture, or binary fixture is applicable to this internal Rust refactor.

## Replay

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-exchange error::tests::structured_annotations_do_not_modify_the_human_message
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime socket_relay::raw_exchange::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime socket_relay --lib
cargo check --manifest-path src-tauri/Cargo.toml -p intercept-proxy-exchange -p intercept-proxy-runtime
cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-exchange -p intercept-proxy-runtime --lib --tests -- -D warnings
```

## Final integration

The initial shared compile/assertion blockers are resolved. Final Runtime library regression is 202/202 PASS, including typed Socket I/O and Frame rejection classification. See [integrated verification](../integrated-verification/README.md); initial failing logs remain historical evidence. Local HTTP Q1 is outside this case.
