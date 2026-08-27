# Execution Specification: Create an HTTP Mock Rule from Real History

## Metadata

- Profile: standard
- Rounds: 8
- Final ambiguity: 15%
- Threshold: 20%
- Context type: brownfield
- Context snapshot: `.omx/context/http-history-local-mock-template-20260825T124357Z.md`
- Interview summary: `.omx/interviews/http-history-local-mock-template-20260825T125336Z.md`
- Prompt-safe initial context: not_needed

## Clarity breakdown

| Dimension | Score | Remaining implementation freedom |
| --- | ---: | --- |
| Intent | 90% | Generated labels and help copy |
| Outcome | 90% | Exact UI composition inside the confirmed per-event flow |
| Scope | 85% | Internal module split and migration mechanics |
| Constraints | 75% | Safe capacity limits and eligibility representation |
| Success criteria | 75% | Exact focused test decomposition |
| Brownfield context | 87.5% | Final impact mapping against the actively changing branch |

Weighted brownfield ambiguity is 15%, meeting the standard 20% threshold.

## Intent

Let a user reuse a previously observed real HTTP response as a local Mock without manually re-entering the status, Headers, and Body, while preventing the generation step itself from changing runtime traffic.

## Desired outcome

From an eligible HTTP response event in the current Exchange timeline, the user can open the normal HTTP rule editor with one unsaved request-stage Mock draft. After review and explicit Save, matching requests receive the copied local response without contacting the upstream.

## In scope

- Add a “create Mock rule” action to each eligible complete HTTP response event in Exchange detail.
- Resolve the selected response to its corresponding request interaction in the same ordered Exchange timeline.
- Build one normal request-stage HTTP `RuleDraft` with:
  - current Listener/Workspace channel binding as required by the existing rule model;
  - one exact `PathOrRequestType` condition from the corresponding request target;
  - one terminal `MockResponse` action;
  - captured response status;
  - captured response Body when it is losslessly supported as text;
  - all captured response Headers allowed by the existing rule validation contract.
- Exclude transport-managed and hop-by-hop Headers, including at least `Content-Length`, `Transfer-Encoding`, `Connection`, `Proxy-Connection`, `Keep-Alive`, `Upgrade`, `TE`, and `Trailer`.
- Preserve allowed duplicate response Headers and their values when supported by the existing wire model.
- Recompute `Content-Length` from Mock Body bytes through the existing Rust response pipeline.
- Navigate to the normal HTTP rule editor with the generated unsaved draft.
- Explain why creation is unavailable for incomplete, unpaired, non-HTTP, binary, compressed, or otherwise non-losslessly-textual response events.
- Add focused Rust and React regression coverage plus an end-to-end local HTTP replay that proves the upstream is not contacted after a saved Mock matches.

## Out of scope and non-goals

- A separate reusable Mock-template catalog or new template persistence model.
- Saving or enabling a generated rule automatically.
- Persisting capture history as a new history database.
- New request match fields in phase 1.
- Method, arbitrary request Header, or raw request Body matching.
- Binary, compressed, or non-UTF-8/non-lossless response Body Mock generation.
- Lossy conversion or silent fallback for ineligible Body content.
- Bulk generation from all interactions in an Exchange.
- Socket Mock generation.
- Automatic filtering, redaction, rewriting, or additional warning for cookies, tokens, timestamps, trace IDs, or business data.
- HTTP Schema/Document rule migration.

## Follow-up feature

Create a separate task that extends HTTP request matching to exact `Method + complete request target`, then allows history-generated Mock drafts to use both conditions. This follow-up must not be silently bundled into phase 1.

## Decision boundaries

Implementation may decide without further user confirmation:

- internal Rust DTOs and application ports;
- whether draft projection is a dedicated application use case or a narrowly extended rule-draft service;
- generated rule name and description;
- stable error codes and localized messages;
- safe Body/Header/event capacity limits consistent with existing project limits;
- the exact lossless-text eligibility marker and validation mechanics, provided raw binary Body bytes are not retained as the phase-1 solution;
- React component composition and focused test organization;
- migration/removal of the legacy `rule_create_from_session` path when current-tree evidence proves it is unreachable or superseded and regression coverage protects behavior.

User confirmation is required before changing any in-scope behavior or adding any non-goal, including raw-byte capture retention, automatic redaction, a template catalog, bulk generation, Socket support, or new request match fields.

## Constraints

- Rust remains the source of truth for event eligibility, interaction pairing, Header parsing/filtering, draft construction, and rule validity.
- The frontend must not reconstruct HTTP responses or duplicate the Header denylist.
- The generated value is an unsaved draft; generation performs no repository write.
- Use the selected event as the response source. Do not silently substitute a different upstream/downstream response event.
- Pair only with a request that is unambiguous under the existing ordered HTTP Exchange contract; otherwise fail closed with a clear reason.
- Preserve the HTTP/Socket boundary and the existing request-stage terminal-action contract.
- Do not add a fallback that produces a partial or corrupt Mock.
- Preserve unrelated working-tree modifications and coordinate with the active architecture task before editing overlapping files.
- Do not Push or trigger CI without explicit user authorization.

## Testable acceptance criteria

1. An eligible complete HTTP response event exposes the create action; non-HTTP and ineligible events do not.
2. Invoking the action performs no persistence and does not change the active runtime rule set.
3. The editor opens with one new request-stage draft bound to the correct context.
4. The draft has one exact request-target condition derived from the paired request.
5. The draft has one terminal Mock action containing the selected response status, all allowed Headers, and losslessly encoded text Body.
6. Forbidden transport/hop-by-hop Headers are absent from the draft; `Content-Length` is generated by the Rust response pipeline from actual Body bytes.
7. Allowed duplicate Headers remain semantically preserved.
8. Saving the reviewed draft through the existing Save flow creates a normal persisted rule; cancellation/navigation without Save creates nothing.
9. When the saved rule matches, the client receives the expected status, allowed Headers, and Body, and the upstream test server records zero requests for that replay.
10. Binary, compressed, non-lossless, incomplete, or ambiguously paired events cannot generate a draft and return/display a stable reason without fallback.
11. Multiple interactions in one HTTP connection allow per-response generation of one correct draft each; no bulk draft is created.
12. Existing manual rule creation, existing Mock execution, capture rendering, and Socket observation behavior remain unchanged.

## Brownfield evidence

### Verified from current source

- Capture currently renders `ExchangeObservationView`, not the legacy HTTP Session list.
- HTTP Exchange events expose textual Header and Body contexts in an ordered connection-level timeline.
- Exchange observations are bounded in-memory runtime evidence.
- `MockResponse` is already a request-stage terminal HTTP rule action with status, Header pairs, and Body bytes.
- Existing validation rejects transport-managed/hop-by-hop Headers in rules.
- Existing Rust Mock response construction recomputes `Content-Length`.
- Legacy `rule_create_from_session` creates only a request-target condition and no action.
- Current Exchange detail has no create-rule action.

### Inference to verify during planning

- The precise event sequence needed to pair a selectable response with its corresponding App request under keep-alive.
- The exact place where text conversion currently becomes lossy and the smallest model change that can mark eligibility without retaining raw binary Bodies.
- Whether the active architecture branch is already changing any shared Exchange observation, rule repository, generated binding, or capture UI files.

## Assumptions exposed and resolved

- “Template” was ambiguous. It now means a normal HTTP Mock rule draft, not a separate reusable catalog item.
- “Create” could have meant immediate persistence. It now means opening an unsaved draft only.
- “Match the real request” could have implied Method/Header/Body matching. Phase 1 uses only the existing exact request target.
- “Copy the real Body” could have implied lossy binary conversion. Phase 1 supports only lossless text and fails closed otherwise.
- “History record” could have meant one connection or all transactions. The action is per complete response event and creates one draft.

## Pressure-pass finding

The Body fidelity assumption was revisited explicitly. The user chose a smaller honest scope—lossless text only—over a broader implementation that could silently corrupt binary or compressed content.

## Scenario pressure findings

- Same target with GET and POST: both may match the phase-1 rule because Method matching is deferred; this is an accepted tradeoff and documented follow-up.
- One keep-alive connection with multiple responses: the user selects an individual response event; only one draft is generated.
- Response with forbidden transport Headers: they are excluded and length is recalculated.
- Response with cookies/tokens/trace IDs: allowed values are copied exactly; no automatic data handling is added.
- Incomplete or non-text response: no draft is created and no lossy fallback is used.

## Documentation and terminology ledger

- Canonical current term: `Exchange observation` is the in-memory ordered runtime history shown in Capture.
- Canonical current term: `MockResponse` is a request-stage terminal action in a normal HTTP rule.
- Canonical current term: `fault template` is a product preset that creates a normal rule; it is not the artifact introduced here.
- User-facing wording should prefer “从历史创建 Mock 规则” over introducing another meaning of “模板”.
- Relevant inspected documents: root README, requirements, user operation guide, data flow, Exchange/Pipeline, rules/protocol packages, HTTP/Socket boundary ADR, HTTP protocol-package ADR, unified observation ADR, and the nested Document migration context.
- HTTP Schema/Document alignment remains separate and must not be implied by this feature.

## Documentation impact to assess during execution

- Root README feature summary.
- `docs/requirements.md` capture/rule contracts.
- `docs/user-operation-guide.md` Capture and rule-creation flow.
- `docs/architecture/data-flow.md` and rules/observation architecture if an application port or event eligibility model changes.
- Test matrix and task evidence index.

## Recommended handoff

Use `$ralplan` before implementation. The user contract is clear, but the current branch has extensive active edits and the design must reconcile the new Exchange observation path with the legacy Session-to-rule draft path, determine lossless-text eligibility without raw-byte retention, and define interaction pairing plus test evidence before touching shared files.
