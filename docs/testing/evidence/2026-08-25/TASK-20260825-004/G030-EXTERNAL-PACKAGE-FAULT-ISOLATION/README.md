# G030 External Package Fault Isolation Evidence

- Task: `TASK-20260825-004`
- Case: `G030-EXTERNAL-PACKAGE-FAULT-ISOLATION`
- Executed: `2026-08-25 18:05:04 +08:00`
- Result: PASS for the G030 scope; the repository-wide source-size gate remains FAIL on four non-G030 accumulated files listed below.

## Purpose and contract

Characterize and verify fault isolation for trusted, unauthenticated external packages. Every reachable package that obeys the wire contract is trusted. This case does not add token, HMAC, mTLS, Origin, enrollment, identity authorization, loopback, CIDR, source filtering, privacy filtering, or payload redaction.

The verified boundary is per connection/package/business connection:

- malformed JSON or malformed WebSocket transport closes only the offending package connection;
- clean Close/EOF remains `Disconnected`, malformed transport maps to stable public code `EXTERNAL_PACKAGE_TRANSPORT_ERROR`, and `MessageTooLong` remains `MessageTooLarge`;
- a stalled package does not consume another package's in-flight capacity;
- registration's single 30-second deadline includes initial request write, heartbeat flush/send, and response wait;
- disconnect cleanup stops only listeners bound to the exact package generation while another package remains online and makes progress;
- `consumed_bytes > buffer.len()` closes only that business connection; the package and listener continue and accept the next connection;
- accepted connections are capped at 256, reject immediately at capacity with `EXTERNAL_PACKAGE_CONNECTION_LIMIT_REACHED`, and release permits after handshake failure or task completion.

## Inputs and actual outputs

- Raw malformed WebSocket frame: [`inputs/malformed-raw-websocket-frame.hex`](inputs/malformed-raw-websocket-frame.hex), exact bytes `83 80 00 00 00 00`.
- Invalid external frame boundary and follow-up business request: [`inputs/oversized-frame-boundary.json`](inputs/oversized-frame-boundary.json).
- Expected/actual comparison and coverage matrix: [`outputs/fault-isolation-results.json`](outputs/fault-isolation-results.json).
- Fresh command results: [`outputs/verification-summary.txt`](outputs/verification-summary.txt).
- Reproduction commands: [`replay/commands.txt`](replay/commands.txt).
- Test-state snapshot and task-related full diff: [`outputs/task-state.txt`](outputs/task-state.txt) and [`outputs/task-related.diff`](outputs/task-related.diff).

## Verification result

- Infrastructure external-package tests: 124 passed, 0 failed.
- Domain external-package tests: 37 passed, 0 failed.
- Application external-package lifecycle/settings tests: 7 passed, 0 failed.
- Host external-package lifecycle test: 1 passed, 0 failed.
- Tauri MCP tests: 23 passed, 0 failed.
- Strict Infrastructure Clippy: PASS with `-D warnings`.
- Rust formatting: PASS.
- Architecture docs/boundaries/runtime/socket-relay/frontend gates: PASS.
- G030 source files: all at or below 500 lines after test-module split.
- Repository source-size gate: FAIL only on `host/tests/architecture.rs` (608), `socket-rules-view.test.tsx` (574), `listener_certificates.rs` (553), and `certificates.rs` (524). These files are outside the G030-owned change and were deliberately not expanded into this protocol fault-isolation slice.

## Documentation audit

`rg` checked `docs/onboarding-guide.md`, `docs/testing/release-validation-matrix.md`, `examples/external-packages/au_eftex/README.md`, `docs/mcp/external-package-integration-guide.md`, and `examples/external-packages/iso8583-deno/README.md` for trusted-network and full-payload conflicts. Two stale statements were corrected:

- release evidence may include full payment frames/payload/Document;
- cross-host external packages do not require Proxy WSS, CIDR, Origin, source, or enrollment authorization.

The AU EFTEX `不完整报文` wording is framing semantics (`need_more` versus `complete`), not a prohibition on storing full payload. The release matrix statement that its active AU fixture contains no real production secrets is fixture provenance, not a trust or payload policy.

## N/A

- Cumulative accepted-connection counter: N/A. The stable EventHub diagnostic is sufficient for the measured observability gap; adding a second cumulative owner was deliberately avoided.
- Screenshots/UI state: N/A; these are backend protocol, actor, registry, and listener-runtime tests.
- Real device, production endpoint, CI, release artifact, and business settlement result: N/A; no external production operation or CI was authorized.
- Raw `.bin` duplicate of the six-byte frame: N/A because the repository editing path is text-patch based; the exact byte-for-byte input is archived as canonical hexadecimal and is replayed directly by the Rust test.

## Workspace safety

The shared worktree already contained G023-G029 and AU codec changes. G030 did not alter those unrelated paths.
