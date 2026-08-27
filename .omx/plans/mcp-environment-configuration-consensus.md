# RALPLAN-DR Consensus Draft: MCP Environment Configuration

- Task: TASK-20260825-006
- Current lane: Planner revision 16 final consensus complete after Architect review 16 APPROVE and Critic review 7 APPROVE
- Required next gate: none; implementation handoff approved for G033 execution
- Consensus status: complete; planning artifact ready for G033 execution
- Planning artifacts:
  - `.omx/plans/prd-mcp-environment-configuration.md`
  - `.omx/plans/test-spec-mcp-environment-configuration.md`
  - `.omx/plans/mcp-environment-configuration-architect-review-1.md`
  - `.omx/plans/mcp-environment-configuration-critic-review-1.md`
  - `.omx/plans/mcp-environment-configuration-architect-review-3.md`
  - `.omx/plans/mcp-environment-configuration-architect-review-4.md`
  - `.omx/plans/mcp-environment-configuration-critic-review-2.md`
  - `.omx/plans/mcp-environment-configuration-architect-review-6.md`
  - `.omx/plans/mcp-environment-configuration-architect-review-7.md`
  - `.omx/plans/mcp-environment-configuration-critic-review-3.md`
  - `.omx/plans/mcp-environment-configuration-architect-review-9.md`
  - `.omx/plans/mcp-environment-configuration-architect-review-10.md`
  - `.omx/plans/mcp-environment-configuration-architect-review-11.md`
  - `.omx/plans/mcp-environment-configuration-architect-review-12.md`
  - `.omx/plans/mcp-environment-configuration-architect-review-13.md`
  - `.omx/plans/mcp-environment-configuration-critic-review-4.md`
  - `.omx/plans/mcp-environment-configuration-architect-review-14.md`
  - `.omx/plans/mcp-environment-configuration-critic-review-5.md`
  - `.omx/plans/mcp-environment-configuration-architect-review-15.md`
  - `.omx/plans/mcp-environment-configuration-critic-review-6.md`
  - `.omx/plans/mcp-environment-configuration-architect-review-16.md`
  - `.omx/plans/mcp-environment-configuration-critic-review-7.md`

## Principles

1. Field contracts over placeholders. `environment_configuration_candidate.v1` is defined through explicit `WorkspaceCommitTemplate` fields, not “current object” shortcuts.
2. Apply-time consistency, not impossible distributed atomicity. Application-held lease guards linearize Application-observed runtime/package state at commit time.
3. Protected material has a strict phase boundary. Create keeps plaintext only in zeroizing memory; apply preparation protects bytes before SQLite; commit persists only prepared records.
4. Product boundary is plaintext/no-auth/all-IP. Do not add auth, TLS transport, CIDR, Origin checks, or privacy redaction beyond generated-output secret redlines.
5. Lock order is a product invariant. All multi-gate operations use the same canonical order and never await protector/SQLite while holding internal registry locks.

## Decision Drivers

1. Critic P0: MITM root CA is installation-scoped and must not be accepted as Workspace-submitted material.
2. Architect P1/Critic P1: canonical JSON and public literals must match the closed v1 registry exactly, including terminal-action field names, warning literals, status literals, cancel-result literals, and the full `WeakNetworkProfileTemplate` object.
3. Architect P1/Critic P1/P2: package SemVer must be parsed before all gates; apply must expose queued-vs-in-progress ownership and exact tagged terminal result semantics; lease implementation must be split into independently verifiable slices, evidence must include explicit AGENTS 10.x snapshots, create deadline reporting must be deterministic, and rule-level diff keys/content must preserve retained existing rule identity while lifting the correct runtime Listener set.
4. Critic review 4: normal cancel must linearize against worker ownership, `IPV6_DEGRADED` must be a WarningCode literal rather than an error, package existence/enabled/online availability must be projection-based with no RPC/health/business bytes, terminal retention must be bounded, and affected Android targets must block all runtime-owner states without auto stop/recovery.
5. Critic review 5: authoritative state table must list normal `cancelled` as an `apply_queued` exit, and the closed public literal registry plus drift tests must register retention eviction literal `oldest_first`.
6. Critic review 6: package projection generation drift, package disappearance, enabled-flag changes, and online-flag changes must be excluded from generic guarded epoch/non-DB mismatch and must always use package-specific terminal `stale` with `CANDIDATE_STALE`, with precedence over `APPLY_LEASE_MISMATCH`.

## Options

### Option A: Reuse `ProxyWorkspace` as the write DTO

Pros:
- Small surface area.

Cons:
- Allows client-submitted final IDs/revisions/runtime-derived refs unless wrapped heavily.
- Fails Architect requirement for alias replacement and server-generated fields.

Verdict: reject.

### Option B: Explicit `WorkspaceCommitTemplate` with aliases and server-generated IDs

Pros:
- Field-level DTO is testable.
- Prevents final refs, revisions, runtime state, and generated IDs from entering the client contract.
- Keeps Domain model reusable while giving MCP a purpose-built write DTO.

Cons:
- Larger schema and more mapping tests.

Verdict: choose.

### Option C: Application lease guards for non-DB state

Pros:
- Implementable with current Application ownership boundaries.
- Provides a precise apply-time precondition and avoids false distributed atomicity.
- Can queue Application-visible external offline publication until guard release.

Cons:
- Cannot prevent physical network disconnect; docs/tests must state the narrower guarantee.

Verdict: choose.

### Option D: True cross-resource atomic snapshot across DB, runtime, package sockets, Android, and keychain

Pros:
- Stronger theoretical guarantee.

Cons:
- Not implementable over physical sockets/keychain/runtime processes.
- Would create false correctness claims.

Verdict: invalid.

## Decision

Use explicit `WorkspaceCommitTemplate` DTO plus Application candidate/lease lifecycle, infrastructure protected-material preparation, and one `EnvironmentCommitPort` SQLite commit boundary.

## ADR

### Decision

Create `ADR-00X-mcp-environment-configuration.md`, explicitly superseding `ADR-004-embedded-read-only-mcp.md`. The ADR records that MCP becomes plaintext all-interface with write tools, no auth, no CIDR, no TLS transport, explicit v1 write DTO, Application apply leases, protected-material preparation before transaction, and `EnvironmentCommitPort` as sole DB commit authority.

### Drivers

- Current read-only/loopback MCP contract is intentionally replaced.
- Full-field DTO prevents implementation-time invention.
- Lease semantics give implementable consistency without claiming physical network immutability.
- Protected material must not be left orphaned or exposed across layer boundaries.

### Alternatives Considered

- Reusing current `ProxyWorkspace` wire shape directly: rejected because it admits final server fields.
- Staged ports with Application-callable finalization/reference methods: rejected because authority splits.
- Cross-resource atomic snapshot: invalid because physical sockets/keychain/runtime cannot participate.
- Auth/TLS/CIDR: invalid because it contradicts explicit product scope.

### Why Chosen

The chosen design keeps MCP as adapter, Application as use-case/lifecycle/lease owner, Domain as pure invariant owner, and Infrastructure as protector/SQLite owner. It makes DTO, aliasing, generation, cancellation, shutdown, and rollback testable.

### Consequences

- New DTO/schema is intentionally detailed and needs full-shape fixtures.
- v1 only accepts `proxy_basic_auth`; upstream/downstream Basic auth roles are fail-closed unsupported.
- v1 does not accept submitted `mitm_root_ca`; MITM uses only the installation-owned `installation:root-ca` selector.
- Apply consistency is an apply-time precondition, not a promise that external services remain online after commit.
- Candidate/apply status is process-local and not restored after restart.
- Hard-kill evidence proves DB atomicity, not status availability.

### Follow-ups

- Revisit SQLite schema after release 1.0.
- Create a separate task if future secure remote management needs auth/TLS/CIDR.
- Consider CLI/TUI reuse after MCP implementation is complete.

## Technical Closure Map

- DTO: explicit `WorkspaceCommitTemplate`; no placeholders.
- Tagged wire: exact JSON appendix plus one full-shape fixture/schema snapshot; terminal actions use `TruncateResponse.bytes`, `DisconnectDuringUpstreamWrite.after_bytes`, and `DisconnectDuringDownstreamWrite.after_bytes`; Protocol Document values use only adjacent-tag `{type,value}` wire for `string`, `int`, `bool`, and `blob`.
- Serde drift gate: execution must round-trip current Domain terminal action types, Protocol Document value/condition/action types, and weak-network types through full-shape fixture/schema/expected-preview and fail on field-name or variant drift in either direction.
- Android weak network: v1 `WeakNetworkProfileTemplate` is an exact required full object matching current Domain serde field names; `Option` values use explicit JSON `null` or the typed value; omitted required fields, scalar shorthand, alternate enum tags, and unknown fields are rejected.
- Secret role: only `proxy_basic_auth` in v1; unsupported roles fail closed.
- MITM ownership: no Workspace-submitted MITM root material; `MitmTemplate` only enables/disables and references fixed `installation:root-ca`; absent/invalid installation root fails validation; apply never mutates installation root.
- Material aliases: every submitted material must have exactly one consumer unless its role explicitly permits multiple consumers; zero consumers fail closed.
- Package SemVer: all package refs typed-parse before target-key/capacity checks, sorting, gate acquisition, protector, or SQLite; invalid versions return `INVALID_PROTOCOL_PACKAGE_VERSION` with zero side effects.
- Package availability: create validates every exact package `id`/`version` exists in the Application-owned projection and is enabled; external packages must be online in that projection. Disabled maps to create validation `PROTOCOL_PACKAGE_DISABLED`, external offline maps to create validation `EXTERNAL_PACKAGE_OFFLINE`, and package validation/preflight emits no package RPC, health probe, business bytes, decode/encode/Display, MAC/cipher call, Socket frame, or HTTP business body. After create succeeds, apply rechecks exact package generation/enabled/online baseline before preparation/commit; package projection generation drift, package disappearance, enabled-flag changes, and online-flag changes always map to terminal `stale` with `CANDIDATE_STALE`, whether discovered by subscribed pre-apply invalidation or apply preflight/lease recheck. This package-specific stale mapping has precedence over generic guarded epoch/non-DB mismatch and is mutually exclusive with `APPLY_LEASE_MISMATCH`, `PROTOCOL_PACKAGE_DISABLED`, and `EXTERNAL_PACKAGE_OFFLINE` for post-create package drift.
- Final refs: forbidden in client input; generated only by commit.
- Server IDs/revisions/runtime state: forbidden unless explicitly existing-target references. `existing_rule_id` is the only narrow rule-identity selector exception, allowed only on HTTP and Protocol Document rule templates for `target.mode=existing`; it is forbidden for `target.mode=new` and for ordinary new-rule final ID submission.
- Unicode: trim-only display name; exact UTF-8 byte collision key; no normalization/casefold.
- Lease: `EnvironmentApplyLeasePort::acquire(scope)` with canonical acquisition order and held logical guards through preparation/commit/cleanup.
- Public literal registry: PRD owns one complete closed registry for environment MCP tool names, protocol/policy strings, warning codes, stable error codes, validation layers, validation statuses, candidate/apply statuses, cancel-result statuses, diagnostic severities, terminal-result union variant literals, and terminal-result status-code literals. `ipv6_unsupported`, `ipv6_dual_stack_covered`, `IPV6_DEGRADED`, retention eviction literal `oldest_first`, `not_found_or_terminal`, `apply_in_progress_not_cancellable`, exact package availability codes, exact Android runtime-owner code, exact `existing_rule_id` selector error codes for forbidden/unknown/duplicate/workspace/kind/binding/package/schema-version/stage failures, and every non-null terminal failure/cancel/rollback code are registered explicitly. Static drift tests must scan all environment MCP DTOs/catalog projections and expected public outputs, including capability retention output, against the registry.
- Terminal result status: `terminal_result` is an explicit tagged union. `committed` carries persisted `workspace_id`, `revision`, and `status_code:null`; `validation_failed`, `stale`, `cancelled`, `cancelled_by_shutdown`, `failed_before_commit`, and `rolled_back` carry exactly one registered non-null `ErrorCode` and no persisted Workspace identifiers. `rolled_back` never reports a new persisted result; existing-target context belongs in status context/diagnostics, not in a claimed committed revision. `not_found` has no terminal result.
- Affected-resource diff: old versus candidate target comparison produces deterministic `added`, `removed`, `changed`, and `unchanged` sets for Listener, Android profile/routing, package refs, HTTP rules, Protocol Document rules, and material refs. HTTP rule keys/content and Protocol Document rule keys/content use `existing_rule_id` for retained target rules and candidate indexes only for new candidate rules. Reference-only and material-only changes count as changed and lift every consuming Listener. Any added/removed/changed HTTP or Protocol Document rule lifts its bound Listener into the affected runtime set. Active/starting/stopping/active-connection lifted listeners reject before preparation/hot rule replacement/transaction. Affected Android profile/device targets apply only when idle with no runtime owner; active, uncertain, waiting_reconnect, cleanup_required, stop_failed, and faulted block with `ANDROID_RUNTIME_OWNER_ACTIVE` before preparation/commit and without auto stop/recovery. This workflow does not hot-replace rules on running listeners.
- Retained rule identity: for `target.mode=existing`, HTTP and Protocol Document rule templates may use `existing_rule_id` only when the ID belongs to the selected target Workspace and matches the same rule kind and exact binding. HTTP retained rules also match exact HTTP stage; Protocol Document retained rules also match exact package ref, schema version, and stage. Duplicate, unknown, cross-workspace, cross-kind, cross-binding, cross-package, cross-stage, and cross-schema IDs fail before gates, with HTTP stage mismatch covered by `EXISTING_RULE_ID_STAGE_MISMATCH`. Old target rules not referenced are removed. Referenced HTTP rules preserve ID and persisted `created_order`; referenced Protocol Document rules preserve ID and persisted `created_order`; mutable content/revision update per Domain contract without a schema migration.
- Deadlock avoidance: release registry/internal locks before awaiting protector/SQLite; callbacks cannot acquire Application gate in reverse.
- External offline: physical disconnect may happen; publication/epoch advance queues until guard release.
- Protected material: create zeroizing plaintext only; apply preparation protects before SQLite; commit persists prepared records only.
- Apply state: apply request atomically consumes token and creates `apply_queued`; the authoritative state table lists `apply_queued` exits as `apply_in_progress`, normal `cancelled`, and `cancelled_by_shutdown`. Owned worker alone transitions to `apply_in_progress`; normal cancel atomically races worker transition. If cancel wins while validating/preview_ready/apply_queued, terminal becomes `cancelled` and queued worker observes terminal without prepare/commit. If worker first transitions `apply_in_progress`, cancel returns `apply_in_progress_not_cancellable` and does not interrupt. Terminal/absent cancel returns `not_found_or_terminal`; `cancelled_by_shutdown` remains a status/terminal result, not a normal cancel result. Token reuse returns `TOKEN_CONSUMED`.
- Shutdown: reject new create/apply; `apply_queued` becomes `cancelled_by_shutdown`; `apply_in_progress` preparation/commit is awaited to terminal.
- Retention: terminal public results/tombstones are bounded to 32 retained terminal candidates and 4 MiB serialized public bytes. Eviction is deterministic oldest-first, active candidates are never evicted, retained tombstones contain no private material, capability output advertises limits, and evicted status lookup returns `CANDIDATE_NOT_FOUND`.
- Guard mismatch precedence: `APPLY_LEASE_MISMATCH` applies only to non-package guarded epoch/non-DB generation mismatches. Package projection generation drift, package disappearance, enabled-flag changes, and online-flag changes are explicitly excluded and always use package-specific `stale` with `CANDIDATE_STALE`; if package and non-package guard mismatches are discovered together, the package-specific stale outcome wins and no secondary mismatch/availability outcome is emitted.
- Deadlines: create's 30s total deadline dominates layer budgets; in-flight layer becomes `cancelled`, downstream dependent layers become `skipped_dependency`, and code is `MCP_CREATE_DEADLINE_EXCEEDED`.
- Evidence: required IDs include split lease evidence and explicit AGENTS 10.x snapshot fields (`base_head`, file list, status, full staged/unstaged diff, untracked files, pre/post stability, resources/inputs/outputs/steps/replay), with preparation/success/cleanup capture, from-zero replay instructions, and N/A explanations instead of empty placeholders.
- Status: process-local until exit; no restart recovery promise.
- Stable public literals: one PRD registry enumerates the complete v1 warning/error/status/cancel-result/policy/layer/severity/tool/schema literal set; no undefined inherited code references remain.
- Transport: plaintext all-interface MCP intentionally has no auth token/header, Host allowlist, Origin gate, source-IP/loopback rejection, or CIDR filter; malformed HTTP/MCP still rejects only for protocol correctness.
- Architect review 7: REQUEST CHANGES closed by Planner revision 8; P1 weak-network DTO ambiguity resolved with exact full-object wire and negative tests; P2 evidence capture resolved with explicit `steps/`, cleanup, N/A, and from-zero replay requirements.
- Critic review 3: REQUEST CHANGES closed by Planner revision 9; P1 authoritative wire placeholders resolved by exact tagged appendix, Protocol Document `{type,value}` shapes, complete MCP tool/status/output shapes, complete stable code appendix, and fixture/schema/roundtrip/drift negative tests; P1 lease affected-resource ambiguity resolved by exact old/candidate diff algorithm, reference-only change handling, sorting, whole-Workspace versus changed-resource-only rules, and runtime rejection tests; P1 transport ambiguity resolved by non-loopback Host/Origin/auth/source-IP openness tests plus protocol-correctness rejection tests.
- Architect review 9: REQUEST CHANGES closed by Planner revision 10; public literal closure now uses a complete registry for warning/error/status/cancel-result/policy/layer/severity/tool/schema literals and static drift tests; affected-resource closure now defines full HTTP rule and Protocol Document rule keys/content, listener lifting for added/removed/changed/reference-only/material-only changes, active/starting/stopping/active-connection rejection, and no hot rule replacement.
- Architect review 10: REQUEST CHANGES closed by Planner revision 11 and refined by revisions 12 and 13; terminal result closure is now an explicit tagged union with `status_code:null` only for committed and exactly one registered non-null `ErrorCode` for failed/cancelled/rolled_back terminals, with fixture/schema/status/literal drift tests. Retained rule identity closure uses `existing_rule_id` only for existing-target HTTP/Protocol Document rule selectors, forbids it for new targets, defines selected-Workspace/kind/binding/package/schema/stage validation with exact registered selector codes before gates, preserves retained ID plus current metadata (`created_order` for HTTP, `created_order` for Protocol Document), removes unreferenced old rules, and adds mapping/diff/preview/runtime-lifting tests.
- Architect review 11: REQUEST CHANGES closed by Planner revision 12 and corrected by revision 13; HTTP retained selector validation now includes exact HTTP stage before any gate with `EXISTING_RULE_ID_STAGE_MISMATCH`, retained metadata now uses existing HTTP `created_order` and Protocol Document `created_order` rather than prior incorrect timestamp-field wording, and terminal results are an explicit tagged union that forbids fabricated persisted identifiers on every non-committed terminal variant including rollback.
- Architect review 12: REQUEST CHANGES closed by Planner revision 13; Protocol Document retained metadata/input/schema/test/closure wording now uses current exact `ProtocolDocumentRuleDefinition.created_order`, explicitly rejects legacy aliases or migrations, keeps HTTP retained metadata as `created_order`, and handed revision 13 to Architect review 13.
- Architect review 13: APPROVE with no P0/P1/P2 findings; Planner revision 13 was accepted for Critic review 4.
- Critic review 4: REQUEST CHANGES closed by Planner revision 14; normal cancel linearization, `IPV6_DEGRADED` WarningCode registry placement, package availability projection/preflight, bounded terminal retention, and Android runtime-owner blocking are now explicit in PRD, test spec, and consensus.
- Architect review 14: APPROVE with no P0/P1/P2 findings; Planner revision 14 was accepted for Critic review 5.
- Critic review 5: REQUEST CHANGES closed by Planner revision 15; authoritative state table now lists normal `cancelled` as an `apply_queued` exit, and `oldest_first` is registered as a capability/policy literal with drift/completeness coverage.
- Architect review 15: APPROVE with no P0/P1/P2 findings; Planner revision 15 was accepted for Critic review 6.
- Critic review 6: REQUEST CHANGES closed by Planner revision 16; package projection generation drift/disappearance/enabled/online changes are excluded from generic guarded epoch/non-DB mismatch and always map to package-specific terminal `stale` with `CANDIDATE_STALE`, with precedence over `APPLY_LEASE_MISMATCH`.
- Architect review 16: APPROVE with no P0/P1/P2 findings; package precedence closure accepted with no further architecture changes.
- Critic review 7: APPROVE with no P0/P1/P2 findings; RALPLAN consensus is complete and implementation handoff is approved.
- Consensus: final consensus complete; ready for G033 execution handoff.

## Pre-Mortem

1. Full DTO, tagged terminal-result semantics, or public literal registry still drifts from implementation.
   - Mitigation: full-shape fixture plus field-by-field expected preview/schema checks, real serde round-trip against current Domain terminal action types/current Domain `WeakNetworkProfile`, terminal-result tagged-union fixture/schema/status tests, committed/null and failure non-null status-code tests, non-committed no-identifier tests, and static registry drift tests over every environment MCP DTO/catalog projection/public output.

2. Alias graph accepts invalid final refs.
   - Mitigation: negative tests reject final refs, missing aliases, duplicate aliases, and role/type mismatch.

3. Server ID generation becomes nondeterministic in tests.
   - Mitigation: inject deterministic ID generator at Application candidate builder boundary.

4. Lease design overclaims network stability.
   - Mitigation: tests prove only Application-observed generation at commit; queued offline publication after guard release is allowed.

5. Canonical gate order is bypassed in one operation and deadlocks under cancellation.
   - Mitigation: static architecture gate plus deterministic deadlock/order/cancel tests.

6. Protector/keychain failure creates DB residue.
   - Mitigation: protector runs before SQLite transaction; failure-before-transaction evidence asserts no rows.

7. Shutdown drops an in-progress mutation guard.
   - Mitigation: in-progress preparation/commit is awaited; queued apply has stable `cancelled_by_shutdown`.

8. Hard kill loses terminal status and is misreported as data corruption.
   - Mitigation: docs/tests distinguish process-local status from SQLite atomic state.

9. Schema drift allows unknown nested fields.
   - Mitigation: serde `deny_unknown_fields` at every nested object/array item plus schema recursion tests.

10. Submitted MITM root mutates installation-global state through a Workspace workflow.
    - Mitigation: remove `mitm_root_ca` from v1 materials; fixed `installation:root-ca` selector only; uploaded role rejected before staging/locks/persistence.

11. Invalid package SemVer reaches lease ordering or package gates.
    - Mitigation: typed parse before target-key/capacity checks and gate 0; invalid version asserts zero gate/protector/SQLite calls.

12. Apply shutdown race loses private material ownership.
    - Mitigation: explicit `apply_queued` state owned by candidate registry, worker-only transition to `apply_in_progress`, and stable queued shutdown cleanup owner.

13. Deadline inversion reports ambiguous validation statuses.
    - Mitigation: total 30s dominates; deterministic `cancelled` current layer and `skipped_dependency` downstream statuses.

14. Rule-body-only changes fail to protect an active Listener.
   - Mitigation: exact HTTP/Protocol Document rule key/content tests plus runtime rejection tests for active versus stopped listeners, added/removed rule scope, reference-only scope, material-only scope, and no hot rule replacement.

15. Existing rule identity is either lost or accepted from the wrong target.
    - Mitigation: `existing_rule_id` is only an existing-target selector; selector validation rejects duplicate, unknown, cross-workspace, cross-kind, cross-binding, HTTP cross-stage, cross-package, Protocol Document cross-stage, and cross-schema IDs with exact registered codes before gates, while retained body-change/add/remove/preview/runtime-lifting tests prove ID plus HTTP `created_order` / Protocol Document `created_order` preservation and correct deletion of unreferenced old rules.

16. Cancel/apply race permits queued work to prepare or commit after normal cancel.
    - Mitigation: atomic cancel-versus-worker transition tests cover cancel-wins, worker-wins, terminal, and absent candidates with exact cancel-result literals and no prepare/commit after cancel-wins.

17. Capability warnings, package availability, terminal retention, or Android runtime-owner gates drift into unregistered or contradictory behavior.
    - Mitigation: registry drift tests include `IPV6_DEGRADED`, `apply_in_progress_not_cancellable`, package availability codes, and `ANDROID_RUNTIME_OWNER_ACTIVE`; package tests prove zero RPC/health/business bytes and exact create/apply mappings; retention tests prove 32/4 MiB oldest-first eviction; Android tests cover active/uncertain/waiting_reconnect/cleanup_required/stop_failed/faulted plus idle permit.

18. Authoritative state table and public literal registry drift behind detailed contracts.
    - Mitigation: state-table review asserts `apply_queued` exits include normal `cancelled`; registry drift/completeness tests include retention eviction literal `oldest_first` in capability policy/retention outputs.

19. Generic guard mismatch hides package-specific stale semantics.
    - Mitigation: generic lease tests exclude package projection generation drift/disappearance/enabled/online changes from `APPLY_LEASE_MISMATCH`; package race tests cover subscribed invalidation and apply preflight discovery paths, simultaneous package/non-package mismatches, precedence, and mutually exclusive terminal outcomes.

## Implementation Handoff

1. DTO/schema/full-shape JSON fixture/schema snapshot, current-Domain serde round-trip/field-name drift gate, tagged terminal-result fixture/schema/status/literal drift tests, Protocol Document `{type,value}` roundtrip/negative tests, weak-network full-object acceptance/rejection gates, closed public literal registry conformance/drift tests including `IPV6_DEGRADED` as WarningCode, and read-tool regressions.
2. Candidate lifecycle, capacity, token, target key, shutdown pre-gates, normal cancel linearization, and bounded terminal retention.
3. Lease contracts, epoch model, retained rule identity validation, and affected-resource diff algorithm.
4. Listener and Android lease adapters.
5. External package publication gating, package availability projection/preflight, zero-RPC availability validation, and typed package-ref ordering.
6. Integrated lease acquisition order, reverse release, cancellation, and deadlock gates.
7. Protected material preparation port and zeroizing handles.
8. EnvironmentCommitPort SQLite transaction.
9. Validation orchestration without business/package RPC.
10. Apply queued/in-progress task, normal cancel race, disconnect, and shutdown terminal orchestration.
11. MCP adapter/schema/annotations/all-interface bind and transport openness/protocol-correctness tests.
12. Packaged App E2E, ADR, docs, and evidence indexes.

Each step requires focused evidence, independent commit, rollback path, and task-document update under project rules.

## Agent Types and Follow-up Path

- `architect`: complete; Architect review 16 APPROVE recorded.
- `critic`: complete; Critic review 7 APPROVE recorded.
- `executor`: implement approved G033 execution steps.
- `test-engineer`: own full DTO, lease, protector, shutdown, hard-kill, IPv6, no-RPC evidence.
- `verifier`: final claim/evidence audit.
- `writer`: ADR/docs synchronization after implementation facts exist.

For durable follow-up, `$ultragoal` is the default goal-mode handoff for G033 execution because the task is multi-step and evidence-heavy. `$performance-goal` is not primary unless validation/bind performance becomes the bottleneck. `$autoresearch-goal` is not primary because no external research is currently needed.

## Consensus Status

Planner revision 16 is final consensus complete after Architect review 16 APPROVE and Critic review 7 APPROVE. Prior revision 10, 11, 12, 13, 14, and 15 closures remain preserved: authoritative wire placeholders are removed from the active PRD contract, Protocol Document `{type,value}` and every supported value variant are explicit, MCP tool/status/output shapes and stable codes are enumerated, transport openness has positive non-loopback/Host/Origin/auth-header tests plus protocol-correctness negatives, the closed public literal registry remains authoritative, exact HTTP/Protocol Document rule diff keys/content still lift bound/consuming Listeners, no hot rule replacement is allowed, `existing_rule_id` remains the only retained-rule selector for existing-target HTTP/Protocol Document rule templates, Protocol Document retained metadata uses current exact `ProtocolDocumentRuleDefinition.created_order` without aliases/migrations, normal cancel linearization is explicit, `IPV6_DEGRADED` is a WarningCode, terminal retention is bounded to 32/4 MiB with registered `oldest_first`, and Android runtime-owner blocking covers active/uncertain/waiting_reconnect/cleanup_required/stop_failed/faulted with no auto stop/recovery. Revision 16 also excludes package projection generation drift/disappearance/enabled/online changes from generic guarded epoch/non-DB generation mismatch; these package projection changes always map to package-specific terminal `stale` with `CANDIDATE_STALE`, whether found by subscribed pre-apply invalidation or apply preflight/lease recheck, and they take precedence over `APPLY_LEASE_MISMATCH` with mutually exclusive outcomes. Implementation handoff is approved for G033 execution. This remains a planning artifact, not implementation proof.
