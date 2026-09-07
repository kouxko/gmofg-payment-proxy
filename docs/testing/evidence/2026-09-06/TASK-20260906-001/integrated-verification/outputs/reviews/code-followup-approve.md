# Follow-up code review: two P2 fixes

## Verdict

- Implemented subset: **APPROVE**
- Whole `TASK-20260906-001`: **BLOCK** until the user selects and the implementation enforces the absolute `read_chunk_bytes` maximum.
- New findings in this follow-up: P0 0, P1 0, P2 0.

## P2 closure: editor session ownership

Confirmed fixed.

`RulesView` now owns one explicit editor session in synchronized state/ref form. Every New, capture-derived replacement, selection, and Cancel starts a new session. The state value keys both top-level editor branches, which forces the old `RuleSinglePairEditor` to unmount even when the business identity remains `rule_id = null` with the same listener/stage. Its cleanup increments the local operation generation, so an old materialization result cannot call `onSave`. The ref value independently gates late editor loads and save/copy/delete UI writes.

Relevant source:

- `src/features/rules/rules-view.tsx:28-41,51-66,69-86,88-158,168-207`
- `src/features/rules/rule-single-pair-editor.tsx:53-66,105-113`
- `src/features/rules/rules-view-session-races.test.tsx:60-112`

The two regressions cover repeated New and capture A to B with equal nullable rule ID/listener/stage. Their RED logs fail before the session-key repair and their GREEN log passes afterward.

I also tested a narrower adversarial interleaving where capture B context completion and draft A materialization completion settle in the same `act` batch. It passed without calling the backend save:

```text
cd /tmp/proxy-code-review-session-same-turn.OvZo8W
deno task test src/features/rules/rules-view-session-races.test.tsx -t 'review reproduction' --reporter=dot
```

Log: `/tmp/proxy-code-review-session-same-turn.OvZo8W/reproduction.log`.

## P2 closure: failure projection budget

Confirmed fixed.

All three failure emission paths now use `fits_failure_projection`. It uses checked addition for the context, human error, package ID/version/stage/method/request ID/stable code/remote message/data summary, and compares their UTF-8 byte lengths against the existing 16 MiB limit. Socket hex is included after encoding. Overflow or an over-budget total emits the established fixed-size loss event instead of the oversized failure event. No error is swallowed and the business result remains unchanged.

Relevant source:

- `src-tauri/crates/exchange/src/observation/failure.rs:6-120`
- `src-tauri/crates/exchange/src/observation.rs:6-8,108-111`
- `src-tauri/crates/exchange/src/observation/tests.rs:153-343`

Coverage includes the original 6 MiB Socket context plus 5 MiB remote-message reproduction and exact-limit/one-extra-byte cases for HTTP, Socket, and no-context failures. The latter uses multibyte UTF-8 external text, so it verifies byte counting rather than character counting. The existing small-event test confirms that all Socket correlation fields remain present.

## Fresh validation

- `deno task test src/features/rules/rules-view-session-races.test.tsx src/features/rules/rules-view.test.tsx --reporter=dot`: 2 files, 15 tests passed.
- `cargo test --offline --manifest-path src-tauri/Cargo.toml -p intercept-proxy-exchange observation::tests`: 7 tests passed.
- `deno task typecheck`: passed.
- `cargo check --offline --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features`: passed.
- Relevant `git diff --check`: passed.
- Targeted scan found no console logging, empty catches, hardcoded API keys, or masking fallback branches.
- Integrated evidence reviewed: 45 Rust groups / 1647 passed; 73 frontend files / 550 passed; fmt, strict Clippy, zero-warning lint, architecture gates, source-size gate, build, and diff check all passed; the 1354-file source comparison reports no change during validation.

The current tool surface has no LSP diagnostics command. Fresh Rust compiler diagnostics and TypeScript `tsc --noEmit` were used as the available type-safety checks.

## Stop condition

The two prior review findings are closed and this implemented subset is ready. The known unresolved `read_chunk_bytes` absolute maximum is outside this follow-up finding count, but it remains a requirements gate, so the whole task must not be archived or marked complete yet.
