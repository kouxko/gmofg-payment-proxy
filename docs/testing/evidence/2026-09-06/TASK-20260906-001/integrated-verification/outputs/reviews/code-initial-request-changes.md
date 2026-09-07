# Implemented fixes code review

Verdict: **REQUEST CHANGES** for the implemented batch.

- P0: 0
- P1: 0
- P2: 2

## P2: replacing a same-identity editor session does not cancel materialization

`RulesView` increments its own generation for capture-derived drafts and New, but an already-open creation editor remains mounted. `RuleCreationEditor` keys `RuleSinglePairEditor` only by listener/stage, and `RuleDefinitionEditor` keys it only by rule ID/listener/stage. New while already creating, or capture draft A to B with `rule_id = null` and the same listener/stage, therefore reuses the child. The child invalidates its materialization generation only on unmount. When draft A finishes materializing it invokes the parent `save`; that function captures the new current parent generation at invocation time and persists the abandoned draft.

Source:

- `src/features/rules/rules-view.tsx:33-53,159-183`
- `src/features/rules/rule-creation-editor.tsx:88`
- `src/features/rules/rule-definition-editor.tsx:55,62`
- `src/features/rules/rule-single-pair-editor.tsx:60,62-66,105-113`

Fix: introduce an explicit editor-session identity that changes for every New, capture replacement, selection, and Cancel. Pass it as a key or cancellation token through the editor boundary so a same-domain-identity replacement invalidates in-flight materialization before it can call `onSave`. Add regressions for New while creation materializes and capture A to B with the same listener/stage.

Reproduction:

```text
cd /tmp/proxy-code-review-new-race.mfnVIe
deno task test src/features/rules/rules-view.test.tsx -t 'review reproduction' --reporter=dot
```

Log: `/tmp/proxy-code-review-new-race.mfnVIe/reproduction-current.log`. Expected `onSave` zero calls; actual one call with the abandoned `HTTP combined` rule.

## P2: new Socket external fields are omitted from the fixed projection budget

The stated invariant at `observation.rs:108-111` caps one UI tracing projection at 16 MiB. The Socket failure guard at `observation.rs:321-322` counts only the hex context and human error, then lines 333-341 add all external-package text fields. A valid 6 MiB Socket context produces 12 MiB of hex; a 5 MiB external `remote_message` makes the emitted projection exceed 17 MiB, yet the current code emits `failed` instead of the fixed `observation_lost` event.

This is bounded to the newly exposed contextual Socket path. The analogous HTTP omission existed before this batch and is not counted as a separate regression. The production entry can carry the remote text because `listener_runtime/package_error.rs:11-23` copies the package RPC message into `remote_message`; the default 4 MiB body setting yields an RPC message limit of roughly 5.4 MiB plus framing (`adapters/bundle.rs:226-238`), so the 5 MiB example fits current transport limits.

Source:

- `src-tauri/crates/exchange/src/observation.rs:108-111,321-344`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/package_error.rs:11-23`
- `src-tauri/crates/infrastructure/src/adapters/bundle.rs:226-238`
- `src-tauri/crates/domain/src/settings.rs:12`

Fix: use one helper that exposes or measures every external projection string, and include those strings in `fits_projection` before emitting the event. Add the bounded Socket regression. Using the same helper for the existing HTTP branches prevents the two paths from diverging further.

Reproduction:

```text
cd /tmp/proxy-code-review-observation-budget.a1FEQf
CARGO_TARGET_DIR=/Users/codin/Code/gmofg-payment-proxy/src-tauri/target cargo test --offline --manifest-path src-tauri/Cargo.toml -p intercept-proxy-exchange review_socket_external_fields_respect_single_projection_budget
```

Log: `/tmp/proxy-code-review-observation-budget.a1FEQf/reproduction.log`. The current implementation records `Some("failed")`; the invariant requires `Some("observation_lost")`.

## Other reviewed behavior and validation

No confirmed implementation regression was found in the Android active-runtime fingerprint/frozen-endpoint path, the Exchange EOF `READ_FAILED` / `Read` / `Downstream` mapping, or lazy observer queue allocation.

Fresh validation passed:

- Cargo workspace check, all targets and all features
- Deno typecheck
- Rules focused tests: 13 passed
- EOF, Socket external-field, and lazy-queue focused Rust tests: passed
- relevant `git diff --check`
- scan for console logging, empty catches, hardcoded API keys, and broad fallback patterns: no added match

No LSP diagnostics tool is callable in this reviewer environment; Cargo check and Deno typecheck are the available compiler/type diagnostics. The known `read_chunk_bytes` absolute-maximum choice is excluded from the issue count, but the overall task cannot be approved or completed until that user-gated contract is resolved and implemented.
