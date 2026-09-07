# HTTP upstream request commit verification

## Purpose

Verify that an HTTP upstream Writer completes when the complete request has actually been flushed, while the Reader independently waits for the response. Preserve valid early final responses without recording a false request `sent` event.

## Environment

- Executed: `2026-09-06 00:49:07 +08:00`
- OS: `Darwin 27.0.0 arm64`
- Rust: `rustc 1.98.0 (88d9e12ae 2026-08-18)`
- Cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05)`
- Manifest: `src-tauri/Cargo.toml`

## Verified inputs and expectations

- A request whose upstream response is held back: the write receipt must become `Committed` before the response future completes.
- A transport whose request flush remains pending: it must time out as `UPSTREAM_WRITE_TIMEOUT` and must not confirm the receipt.
- A valid final response returned before request flush: it must remain readable as `ReadReady` and must not be reported as committed.
- Cancellation and receipt owner drop: completion waiters must be released with a stable error.
- Raw injected upstream writes and LocalServer connectors: acknowledge only after their complete request handoff/flush.
- Socket protocol writers: remain normal `Committed` writers with existing half-close and relay behavior.

## Implementation boundary

- `WriteOutcome<C>` distinguishes `Committed(C)` from `ReadReady(C)` at the protocol-neutral Writer boundary.
- `ProtocolExchange` records `sent` only for `Committed` and proceeds to Reader for both outcomes.
- `UpstreamWriteReceipt` is resolved exactly once by a real flush, an early response, or an error.
- The connection-owned HTTP child task runs `UpstreamConnector::send`; Writer awaits only the receipt outcome and Reader awaits the response channel. Cancellation, task abort, panic, and channel drop release both waiters.

## Commands and results

The complete command output is stored in `outputs/verification.txt`.

- Upstream commit regressions: `4 passed`.
- Buffered HTTP endpoint actor regressions: `6 passed`, including delayed response and early response.
- Exchange core: `26 passed`.
- Upstream HTTP integration: `7 passed`.
- Socket relay regression: `61 passed`.
- Exchange/runtime `cargo check`: PASS.
- Exchange/runtime strict Clippy with `-D warnings`: PASS.
- Additional verification run before final capture: proxy library `195 passed`; raw HTTP proxy integration `19 passed`.

## Result

PASS. Request `sent` now represents a confirmed complete transport flush. Response latency no longer delays the upstream Writer, and a valid early response follows `ReadReady` without a false commit or fail-closed behavior.

## Replay

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime upstream_write_commit --lib
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime http::exchange_runtime::endpoints::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-exchange
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime --test upstream_http
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime socket_relay --lib
cargo check --manifest-path src-tauri/Cargo.toml -p intercept-proxy-exchange -p intercept-proxy-runtime
cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-exchange -p intercept-proxy-runtime --lib --tests -- -D warnings
```

## Not applicable

- CI, deployment and external production traffic: outside this subtask.
- TLS certificate material and protocol binary fixtures: no contract or fixture changed.
- Hash inventory: intentionally not generated for this evidence set.
