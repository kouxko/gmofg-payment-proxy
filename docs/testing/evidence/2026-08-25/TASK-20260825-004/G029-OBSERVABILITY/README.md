# G029 Observability Contract Consolidation Evidence

- Task: `TASK-20260825-004`
- Case: `G029-OBSERVABILITY`
- Executed: 2026-08-25 17:29:40 +08:00
- Result: PASS

## Purpose

Characterize the existing observability stores, expose missing loss/retention contracts, and prove
deterministic count/byte boundaries without changing the business result.
Full HTTP bodies, Socket bytes, decoded Documents and reproduction evidence remain allowed in their
dedicated bounded stores; no privacy filter, payload omission, authentication or dependency was added.

## Preconditions and tested state

- Repository root: `/Users/codin/Code/gmofg-payment-proxy`
- Rust commands used `--manifest-path src-tauri/Cargo.toml`.
- G023-G028 and the user's unrelated AU EFTEX edits were already present and were not reverted.
- G029-related files were stable during the final test sequence; the pre/post task-related status
  records were recaptured after the reviewer fixes.
- `docs/mcp/tool-reference.md` and `src-tauri/crates/infrastructure/src/error.rs` also contain earlier
  TASK-20260825-004 changes; G029 modified only the observability rows/comments shown by the task diff.

## Inputs and expected results

1. Count capacity: `N=3`; records 1..3 must be retained with zero eviction, record 4 (`N+1`) must evict
   only record 1 and expose `evicted_count=1`.
2. Queue byte budget: `B=128`; a 128-byte reservation must be admitted, the next 1 byte (`B+1`) must
   return `BytesFull`, and the surrounding business result must remain `Ok("business-completed")`.
3. Runtime-log queue: one-slot queue receives the first message, counts the next as full, then counts
   a post-disconnect message. The MCP/query projection must expose `1/1/0` for
   full/disconnected/contended.
4. Exchange store: oversized append must roll back the event, retain the opened record, mark
   `evidence_evicted`, and leave other connections intact.
5. Reproduction Markdown must include Store eviction plus all three current-process queue-drop counts.
6. Diagnostic EventHub count retention: `N=3`; at N the oldest retained event is 1 and no refresh is
   required. Publishing event 4 also emits the existing bounded overflow warning, so the three retained
   envelopes start at ID 3 and `snapshot_required=true` for `after_event_id=1`.
7. Diagnostic EventHub byte retention uses its existing shared `CapacityLedger`: fixed fixture
   `B=569` bytes is retained; the otherwise identical `B+1=570` byte event is not retained and an old
   cursor receives `snapshot_required=true`. No new byte-budget mechanism was introduced.
8. Exchange counters must remain responsibility-specific: a producer queue drop returns
   `dropped_events=1, ignored_events=0`; a missing-open consumer rejection returns
   `dropped_events=0, ignored_events=1`.
9. MCP smoke must validate fields on five observability outputs: application logs, diagnostics,
   Exchange observation, HTTP capture and reproduction report.

## TDD evidence

RED command:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy runtime_logs --lib
```

The compile failed with six `E0609` errors because `ApplicationLogPage` did not contain
`queue_dropped_full`, `queue_dropped_disconnected`, or `queue_dropped_contended`. This proved the
producer counters existed but were absent from the read contract.

After adding only the counter projection and report rendering, the same focused suite passed:
`38 passed; 0 failed`.

Reviewer RED added page-retention and split-counter expectations before production changes. Rust
compilation failed because `DiagnosticLogPageViewModel` lacked `oldest_retained_event_id` and
`snapshot_required`, the page snapshot accepted no cursor, and `ExchangeObservationPage` lacked
`dropped_events`. The final implementation reuses the existing EventHub/Store owners and adds no
parallel retention path.

## Final commands and actual results

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy runtime_logs --lib
# 38 passed; 0 failed

cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure exchange_observation
# 8 passed; 0 failed

cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application diagnostic
# 14 passed; 0 failed

cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy mcp --lib
# 23 passed; 0 failed

pnpm vitest run src/features/capture/exchange-observation-list.test.tsx \
  src/features/diagnostics/diagnostic-logs-view.test.tsx
# 2 files passed; 4 tests passed

pnpm typecheck
# passed

pnpm scan:architecture-docs
# passed: 9 current documents, 7 ADRs, 5 MCP documents

pnpm scan:architecture-boundaries
# passed: 76 behavior fixtures, 8 role cases, production boundary gate

node scripts/check-runtime-architecture.mjs --require-zero-debt
# passed: 33 fixtures, 8 owned task sites, zero phase-1 spawn debt

cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features -- -D warnings
# all passed
```

The final post-edit single boundary rerun also passed:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy \
  byte_budget_b_and_b_plus_one_leave_business_result_unchanged --lib
# 1 passed; 0 failed
```

## Capacity, retention and ownership result

- Runtime log Store: 20,000 records, 32 MiB JSONL, approximately 75% rotation low-water mark;
  query default 200 and maximum 500.
- Runtime/Exchange producer queues: each has `ui_event_capacity` slots (default 4,096, maximum
  65,536) and they share `max_memory_bytes / 4` logical bytes.
- Exchange loss notification: independent one-slot, 64-byte control lane.
- Exchange observations: complete connection evidence in memory, sharing `CapacityLedger` with
  sessions/events; oldest other connection is evicted before rejecting the protected current event.
- Diagnostic EventHub: default count capacity 4,096 and the same shared `CapacityLedger`; query returns
  global `oldest_retained_event_id` plus conservative `snapshot_required` when the requested cursor has
  a retention gap. Deterministic evidence covers N=3/N+1 and B=569/B+1=570.
- Exchange observation loss: `dropped_events` is producer queue admission loss;
  `ignored_events` is consumer parsing/identity/store rejection. The counters remain process-global
  because an unrecorded primitive event cannot be reliably assigned to a Workspace.
- Runtime queue counters: `queue_dropped_full`, `queue_dropped_disconnected`, and
  `queue_dropped_contended` are now returned by MCP/query and rendered in reproduction reports.
- Intentionally separate lanes: reproduction report does not aggregate Exchange observation or HTTP
  capture; MCP projects existing stores and creates no second retention owner.

## N/A evidence

- Network frames and binary `.bin`/`.hex`: N/A; no network/protocol wire behavior changed.
- UI screenshots/accessibility: N/A; the bounded retention warning and split counters are covered by
  component contract tests and do not introduce a new interaction flow requiring screenshot evidence.
- SQLite fixture/migration: N/A; no persistence schema changed.
- Remote CI/artifact/release/business outcome: N/A; not authorized and not required for this local
  observability-contract task.

## Re-run

Run the final commands above from the repository root. If any G029-related file changes during the run,
discard that run as completion evidence and repeat on a stable tree.
