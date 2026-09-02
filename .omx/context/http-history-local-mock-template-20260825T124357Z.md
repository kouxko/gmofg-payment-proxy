# HTTP History Local Mock Template - Context Snapshot

- Task statement: Add a feature that creates a local HTTP Mock template from real traffic history captured by the proxy.
- Desired outcome: Let a user select a previously observed real HTTP exchange and reuse its response locally without manually re-entering all response fields.
- Stated solution: Create a local Mock template from the current real-access history.
- Probable intent: Reduce repeated manual Mock configuration and make a known real response reproducible when the upstream is unavailable or should be bypassed.
- Context type: Brownfield.
- Prompt-safe initial-context summary: not_needed.

## Confirmed repository facts

- The current capture page renders `ExchangeObservationView`; it no longer mounts the legacy HTTP Session list.
- HTTP Exchange observations retain ordered in-memory `Received` and `Sent` events with textual HTTP header and body contexts.
- Observation records are bounded in-memory evidence and are not persisted to SQLite.
- HTTP `MockResponse` already exists as a request-stage terminal rule action and is persisted as part of Workspace rules.
- `rule_create_from_session` exists, but it only creates a request-stage draft with a path/request-type equality condition and no actions.
- The current Exchange observation detail UI has no create-rule or create-Mock action.
- The repository already uses both “rule” and “fault template” as distinct concepts; “local Mock template” is not yet a settled canonical artifact.

## Historical continuity

- Historical notes observed a missing response-packet-template workflow, but no current-tree implementation or accepted contract was proven.
- Historical HTTP Schema/Document-rule alignment remains a separate pending design topic and must not be silently merged into this feature.

## Constraints

- Do not persist raw capture history as a new data store unless explicitly confirmed.
- Preserve the existing HTTP/Socket boundary; this request currently names HTTP only.
- Do not implement during the deep-interview requirement phase.
- The working tree contains extensive pre-existing edits; treat them as owned by other work and do not overwrite them.

## Unknowns and decision boundaries

- Which request fields become match conditions and whether the user edits them before saving.
- Which response components are copied: status, headers, body, encoding, and transport metadata.
- Whether volatile or sensitive headers/body fields are copied, removed, parameterized, or confirmed interactively.
- Whether generation is single-record only or supports grouping/variants.
- Whether saving immediately enables the Mock or creates a disabled draft.
- Required conflict behavior when multiple Mock rules match.
- Acceptance criteria and non-goals.

## Interview decisions

### Round 1 - Output artifact

- [from-user] Selecting one real HTTP history record creates a normal editable HTTP Mock rule.
- [from-user] A separate reusable Mock-template catalog is out of scope.
- The generated rule should prefill request match conditions and the captured real response; the exact copied fields and save/enable behavior remain unresolved.

### Round 2 - Save and activation boundary

- [from-user] Generation opens a prefilled unsaved rule draft.
- The draft must not affect runtime traffic until the user explicitly reviews and saves it.
- Immediate persistence, whether disabled or enabled, is out of scope.

### Round 3 - Request matching scope

- [from-user] The generated draft uses only the existing exact `PathOrRequestType` condition populated from the captured request target.
- [from-user] Phase 1 does not add Method, arbitrary Header, or raw Body match fields.
- Accepted tradeoff: requests with different Methods but the same request target may match the same Mock rule; the user can review the draft but cannot add an unsupported Method condition in phase 1.
- [from-user] Adding exact `Method + complete request target` matching is an explicit follow-up feature, not part of this first implementation.

### Round 4 - Captured response projection

- [from-user] The generated Mock action copies the captured HTTP status, Body, and every response Header allowed by the existing rule contract.
- Transport-managed and hop-by-hop Headers such as `Content-Length`, `Transfer-Encoding`, and `Connection` are excluded from the draft.
- Rust recomputes `Content-Length` from the copied Mock Body when producing the response.

### Round 5 - Body fidelity boundary

- [from-user] Phase 1 supports only HTTP response Bodies that can be represented losslessly as text by the captured history model.
- Binary, compressed, or non-UTF-8 response Bodies must not be converted through a lossy display string.
- For an ineligible response, the create-Mock action is unavailable and the UI explains why.
- Follow-up possibility, not phase 1: retain raw HTTP Body bytes in Exchange observations for byte-exact binary Mock generation.
- Pressure-pass result: “copy the real Body” was narrowed to lossless text only; silent corruption is explicitly rejected.

### Round 6 - Interaction selection

- [from-user] The create action is attached to each complete HTTP response event in the Exchange timeline.
- One action creates exactly one draft from that response and its corresponding preceding request.
- Bulk generation for every interaction in a connection is out of scope.
- An Exchange containing multiple interactions remains supported through per-response selection; phase 1 is not restricted to single-interaction connections.

### Round 7 - Sensitive and volatile data

- [from-user] The generated unsaved draft copies all rule-valid response Headers and the eligible text Body exactly as observed.
- Phase 1 performs no automatic filtering, redaction, rewriting, or additional warning for cookies, tokens, timestamps, trace IDs, or business data.
- The user owns review of the unsaved draft before choosing to persist and enable it.

### Round 8 - Scope and acceptance closure

- [from-user] Confirmed the complete phase-1 contract without changes.
- Non-goals: separate template catalog, immediate persistence, binary/compressed/non-text Body support, batch generation, Socket Mock, new request match fields, and automatic sensitive-data handling.
- Follow-up: add exact `Method + complete request target` matching as a separate future feature.
- OMX may choose internal DTOs, service boundaries, generated rule name/description, stable error codes, safe capacity limits, and test organization without further confirmation.
- Acceptance requires no persistence before Save, correct draft projection, no upstream access after the saved Mock matches, byte-correct eligible text Body, correct status and allowed Headers, transport Header normalization, and an unavailable action with a clear reason for ineligible events.

## Readiness

- Final ambiguity: 15% (standard threshold: 20%).
- Non-goals: PASS.
- Decision boundaries: PASS.
- Pressure pass: PASS; Body fidelity was narrowed from generic copying to lossless text only.
- Practical closure audit: PASS.

## Likely touchpoints

- `src/features/capture/exchange-observation-view.tsx`
- `src/features/capture/exchange-observation-detail.tsx`
- `src-tauri/crates/application/src/models/exchange_observation.rs`
- `src-tauri/crates/application/src/facade/`
- `src-tauri/crates/application/src/ports.rs`
- `src-tauri/crates/infrastructure/src/adapters/exchange_observation.rs`
- `src-tauri/crates/infrastructure/src/adapters/rules/`
- `src/features/rules/`
- generated Tauri bindings and focused Rust/React tests

## Inspected guidance and documentation

- Workspace `AGENTS.md` supplied in the current turn.
- `README.md`
- `docs/requirements.md`
- `docs/user-operation-guide.md`
- `docs/architecture/data-flow.md`
- `docs/architecture/exchange-pipeline.md`
- `docs/architecture/rules-and-protocol-packages.md`
- `docs/architecture/decisions/ADR-001-http-socket-boundary.md`
- `docs/architecture/decisions/ADR-002-protocol-packages-http.md`
- `docs/architecture/decisions/ADR-006-unified-exchange-observation.md`
- `.omx/context/nested-document-migration-20260825T030353Z.md`

## Terminology ledger

- `Exchange observation`: current in-memory ordered runtime history shown in Capture.
- `HTTP MockResponse`: current request-stage terminal rule action that bypasses upstream and returns a configured response.
- `Fault template`: current product catalog preset that creates a normal HTTP rule.
- Unsettled user term: “local Mock template” could mean a normal persisted rule or a separately reusable preset/template.
