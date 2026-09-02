# FINAL Architecture Validation Evidence

- Task: `TASK-20260825-004`
- Case: `FINAL-ARCHITECTURE-VALIDATION`
- Executed: `2026-08-25 22:56:32 +08:00` to `2026-08-25 23:00:03 +08:00`
- Result: `PASS`

## Purpose and fixed contract

Validate the complete G023-G031 architecture tree after behavior-preserving source cleanup. The fixed user contract is:

- Rust owns protocol-rule stages, schema bindings, and creation drafts; React renders the Rust contract and submits intent.
- Listener client CIDR configuration and admission do not exist; every source IP is allowed subject only to existing listener-mode and capacity contracts. Android destination-routing CIDRs remain.
- SQLite remains on the pre-1.0 incompatible-schema reset policy. This task does not freeze the Release 1.0 schema or add a pool/dependency. The reviewed schema wording was corrected to describe the current pre-release incompatible schema.
- External protocol packages are trusted and unauthenticated. This task does not add token, HMAC, mTLS, Origin, enrollment, loopback, CIDR, or source-identity authorization.
- Full HTTP, Socket, Document, diagnostic, report, and test payload evidence is allowed. Credential/private-key handling remains a separate correctness boundary, not a new privacy scope.

## G031 architecture and lifecycle implementation

- Listener start/stop now has one explicit cancellation owner and a run-token/epoch compare-and-swap guard, so a stale task cannot publish or clear a newer runtime.
- Rule execution now uses an asynchronous `RuntimeRule` actor; durable compare-and-swap persistence completes before actions are applied.
- TLS certificate and rule bridges are bounded and cancellation-aware instead of leaving unowned blocking work.
- Dynamic SNI uses an atomic CA/fallback snapshot and removes panic-based certificate conversion paths.
- External registry/provider access is asynchronous and preserves exact package identity and failure propagation.
- Document-rule compilation runs behind a bounded CPU boundary and applies generation compare-and-swap semantics.
- Android and Host SQLite bootstrap paths use the shared executor and explicit owned state transitions.
- Responsibility-based source splits restored the handwritten source-size gate without adding dependencies, compatibility paths, fallbacks, or new business abstractions.

## Source cleanup inventory

- `certificates.rs`: now 477 lines; certificate material is isolated in `certificates/material.rs` (57 lines).
- `listener_certificates.rs`: now 452 lines; runtime resolution is isolated in `listener_certificates/resolution.rs` (93 lines).
- `host/tests/architecture.rs`: now 370 lines; scan helpers are isolated in `host/tests/architecture/support.rs` (246 lines).
- `socket-rules-view.test.tsx`: now 491 lines; hoisted mocks/query runtime are isolated in `socket-rules-view.test-runtime.tsx` (170 lines).
- `external_relay/contract.rs`: now 218 lines; contract tests are isolated in `contract_tests.rs` (409 lines).
- `pipeline/rule_runtime.rs`: now 328 lines; actor ownership is isolated in `rule_runtime/actor.rs` (214 lines).
- `body_codec_lifecycle.rs`: now 391 lines; epoch-cleanup tests are isolated in `body_codec_epoch_cleanup.rs` (111 lines).

All handwritten source files are at or below 500 lines. Existing behavior, public contracts, tests, and assertions remain.

## Stable consecutive verification

The same task-scoped status and diff fingerprints were captured before and after every run. Three consecutive frontend suites and three consecutive Rust workspace suites passed:

| Run | Command | Started | Finished | Actual result |
| --- | --- | --- | --- | --- |
| V1 | `pnpm test` | 22:56:32 | 22:57:17 | PASS; 66 files / 648 tests |
| V2 | `pnpm test` | 22:57:18 | 22:58:03 | PASS; 66 files / 648 tests |
| V3 | `pnpm test` | 22:58:03 | 22:58:47 | PASS; 66 files / 648 tests |
| R1 | `cargo test --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features -q` | 22:58:47 | 22:59:30 | PASS |
| R2 | same as R1 | 22:59:30 | 22:59:47 | PASS |
| R3 | same as R1 | 22:59:47 | 23:00:03 | PASS |

Full run details and fingerprints are archived in [outputs/consecutive-runs.txt](outputs/consecutive-runs.txt).

## Complete local quality gate

`pnpm check` passed on the stable tree and covered generated bindings, architecture/runtime/socket/frontend/source-size scans, ESLint, TypeScript, 66-file/648-test Vitest, production build, bundle branding, Rust formatting, strict workspace Clippy with all targets/features and `-D warnings`, Windows Rust check, and workspace tests.

Additional fresh checks passed:

- `cargo test --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features`: PASS.
- `cargo fmt --manifest-path test-support/socket-relay-gate/Cargo.toml -- --check`: PASS.
- `cargo test --manifest-path test-support/socket-relay-gate/Cargo.toml --all-targets`: PASS; 10/10.
- `pnpm scan:architecture`: PASS; documentation 9/7/5, architecture fixtures 76 behavior/8 roles, runtime fixtures 43, Proxy owned task sites 10, Infrastructure owned task sites 12, zero debt, socket fixtures 24, frontend Rust-only ownership PASS.
- `pnpm scan:source-size`: PASS.
- Generated bindings remained stable across the validation runs.

## Independent review state

- Independent architect: `APPROVE`; architecture invariant status `CLEAR`.
- Independent code reviewer: final `APPROVE`; the earlier three P2 evidence/documentation findings were corrected and independently re-reviewed with zero remaining findings.
- Review closed: `2026-08-25 23:30:42 +08:00`. The final architecture, code, verification, evidence, and cleanup gates all passed.
- Machine-readable aggregate gate: [quality-gate.json](quality-gate.json).

## Archived replay materials

- [Replay commands](replay/commands.txt)

## Related evidence

- [G029 observability](../G029-OBSERVABILITY/README.md)
- [G030 external-package fault isolation](../G030-EXTERNAL-PACKAGE-FAULT-ISOLATION/README.md)
- G023-G028 implementation and review evidence is recorded in `.omx/ultragoal/ledger.jsonl` and the task document.

## Scope exclusions

The following user-owned or other-task paths are not part of the captured task content and were not modified by this evidence update:

- `examples/external-packages/au_eftex/au_eftex/codec.py`
- `examples/external-packages/au_eftex/tests/test_codec.py`
- `docs/tasks/upstream-multi-ca-pem-bundle.md`
- `docs/testing/evidence/2026-08-25/TASK-20260825-005/`
- `docs/testing/evidence/2026-08-25/TASK-20260825-007/`
- all other task archives and indexes except the specifically assigned evidence index row updates.

## N/A

- Screenshots and manual browser capture: N/A. This is an architecture, backend, static-boundary, and automated UI-contract validation; no visual acceptance requirement changed.
- Protocol binary/frame resource snapshot: N/A. The final gate replays existing automated contracts and does not introduce a new external frame vector; G030 archives its actual malformed-frame inputs separately.
- Real device, production endpoint, settlement result, release artifact, and remote CI: N/A. None was authorized or required for this local architecture task.
- Authentication/privacy hardening: N/A by explicit user decision.
