# LISTENER-DRAFT-CONTRACTS-001

TASK-20260905-002 completed its accepted implementation and independent review. Result: **PASS_WITH_KNOWN_UNRELATED_SOURCE_SIZE_FAILURE_AND_NATIVE_UI_NOT_RUN**.

Application owns the editable ListenerDraft, six structural intents, copy and CAS-first validation. Only approved incomplete package/identity/trust fields may be null. Strict domain types and the existing save transaction remain unchanged. UI retains leaf edits, asynchronous intent coordination and certificate leases. The sole overall-review P2 (LocalResponder ServerTls reporting a no-op success) was reproduced and corrected before final approval.

Parent: TASK-20260905-001 / frontend-contracts / [original evidence](../../../2026-09-05/TASK-20260905-001/frontend-contracts/README.md). Its per-Listener conflict, partial discard, authoritative revision, delayed response and lease contracts remain covered. No old evidence was overwritten.

| Evidence | Actual result |
| --- | --- |
| `baseline-transition-tests.log`, `resources/before/`, [design review](design-review.md) | 53 original transition tests passed; rejected 5-API design reduced to 3 APIs and approved before implementation |
| [Backend](backend/README.md) | Draft 12, legacy validation 2, binding 1; fresh deterministic bindings and strict Clippy passed at original B1 state |
| [Frontend](frontend/README.md) | Full 536; final affected 72 and 39; typecheck/lint/boundaries passed; exact windows and v4-final snapshots retained |
| [Overall integration](integrated/README.md) | Rust libraries 969 before review repair; final Next build/architecture passed; 58 final resources matched |
| [Review repair](backend-review-fix/README.md) | True/false inputs RED then GREEN; final Draft 13, validation 2, binding 1, Clippy/fmt/diff passed |
| [Independent overall review](integrated/independent-review.md) | REQUEST CHANGES (one P2) then APPROVE; remaining P0/P1/in-scope P2 = 0 |
| [Archive configuration boundary](integrated/config-boundary/README.md) | Real TypeScript/ESLint RED/GREEN; only evidence resources excluded from source discovery |

The original 969-test and 536-test runs are not presented as a single post-repair run. Replay commands, actual inputs/expected outcomes, execution outputs and source/resource mappings are in the linked cases. The final source integrity replay is `integrated/check-final-source-state.py` (run from the repository root).

Limits: the source-size gate still fails on six existing unrelated files; no new oversized file remains. Native desktop UI, Android devices, CI, push, commit, installation and release were not executed. Automated jsdom accessibility/state assertions cover the affected UI interactions; protocol frames and device screenshots are N/A for this task. Task001 #3 recovery choice is still pending.
