# Test Spec: MCP Environment Configuration

- Task: TASK-20260825-006
- Mode: RALPLAN deliberate planner draft, revision 16 after Architect review 15 APPROVE and Critic review 6 REQUEST CHANGES
- Status: Consensus approved, ready for G033 execution; planning artifact only

## Verification Claim

The change is correct only if MCP accepts the exact `environment_configuration_candidate.v1` DTO, validates all mandatory layers, rejects all client-submitted final refs/server state, uses Application-held leases for apply-time non-DB consistency, prepares protected material before the SQLite transaction with zeroizing buffers, and commits only through `EnvironmentCommitPort`.

## Evidence Roots

- Evidence root: `docs/testing/evidence/<execution-date>/TASK-20260825-006/`
- Required IDs after the user-confirmed 2026-08-27 execution amendment: `MCP-CONFIG-CONTRACT-001` and integrated packaged-App case `MCP-CONFIG-APP-001`. Separate G034/G035/G036/G038 evidence directories are waived; their behavior remains mandatory in fresh repository tests, the task test ledger, and whole-task independent review.

Every retained evidence directory records task-related file names and readable source extracts, pre/post-test runtime stability, `resources/`, `inputs/`, `outputs/`, `steps/`, `replay/`, purpose, environment, preconditions, commands, expected output, actual output, comparison, logs/stdout/stderr, N/A explanations, and result. Per the user's 2026-08-27 instruction, evidence does not record Git state, Commit records, HEAD, diffs, or hashes. `steps/` records preparation, success-path execution, and cleanup exactly as run. `replay/` records from-zero reproduction using archived resources, including dependency setup, packaged App path, service/device prerequisites, environment variables, commands, expected terminal result, and cleanup. Network/protocol cases additionally record sent requests/responses, TCP facts where applicable, decoded validation result, and no-business-payload/no-package-RPC proof. If `resources/`, `inputs/`, `outputs/`, `steps/`, or `replay/` has no applicable files for a case, the evidence omits the empty directory and records `N/A` with a reason in `README.md` or `metadata.json`.

Original private-material fixtures are archived under evidence `resources/` exactly as used. Secret redline scans cover generated outputs/logs/responses/serialized results only.

## Unit Tests

1. Full-shape DTO fixture
   - Use the single authoritative JSON fixture `src-tauri/src/mcp/tests/fixtures/environment_configuration_candidate_v1/full-shape.json`.
   - Fixture contains new target, HTTP listener, Socket relay, Socket local responder, HTTP Basic auth, downstream TLS/mTLS, upstream TLS/mTLS, MITM enabled with fixed `root_ca_selector:"installation:root-ca"` and no submitted MITM root material, HTTP body protocol package, scripted Socket package, HTTP rules, protocol Document rules, terminal fault actions, Android weak-network profile/routes/targets, certificate materials, and the only v1 secret role `proxy_basic_auth`.
   - Store field-by-field `expected-preview.json` for preview, alias graph, generated IDs, target key, validation layer summary, and public baseline.
   - Store `schema.snapshot.json` and compare every DTO field/tag/payload exactly.
   - Fixture must include `weak_network` with every required `WeakNetworkProfileTemplate` field present: `seed`, `fixed_delay_millis`, `uniform_jitter_millis`, `upload_bytes_per_second`, `download_bytes_per_second`, `random_loss_basis_points`, `burst_loss`, `duplicate_basis_points`, `reorder_basis_points`, `maximum_reorder_hold_millis`, `blackout_windows`, `dns_blackhole`, `nth_tcp_flag_drops`, `path_mtu`, and `corruption`.
   - Fixture/schema/expected-preview must include both explicit-null and object/value weak-network forms: `upload_bytes_per_second:null`, `download_bytes_per_second:null`, `burst_loss` as a four-field object in the full-shape fixture plus a separate accepted fixture with `burst_loss:null`, `path_mtu.mtu:null`, and `path_mtu.mss_clamp:null`.
   - Weak-network negative fixtures reject omitted required fields, unknown fields at each nested level, scalar shorthand for `burst_loss`, scalar shorthand for `path_mtu`, object shorthand for enum values, alternate enum casing/tags, omitted optional values where explicit `null` is required, zero rate-limit values, out-of-range basis points, `nth_tcp_flag_drops[].nth=0`, too-small MTU, MSS greater than or equal to MTU, and `corruption.bits_per_packet>64`.
   - Schema snapshot must require all weak-network object fields listed above, recurse through `blackout_windows.items`, `nth_tcp_flag_drops.items`, `path_mtu`, `corruption`, and `burst_loss`, and disallow additional properties for each object.
   - Add a real serde round-trip test using the current Domain terminal action types and DTO mapping: construct every terminal action variant in typed Rust, serialize to JSON, assert the full-shape fixture uses the same field names, deserialize fixture JSON back through the typed DTO/Domain mapping, and reserialize to byte-for-byte canonical JSON for `TruncateResponse.bytes`, `DisconnectDuringUpstreamWrite.after_bytes`, and `DisconnectDuringDownstreamWrite.after_bytes`.
   - Add a field-name drift gate that fails if current Domain serde output contains terminal-action fields not present in the fixture/schema/expected-preview, or if fixture/schema/expected-preview contain terminal-action fields not emitted by current Domain serde.
   - Add terminal-result fixture/schema/status/literal drift tests for the explicit tagged union: committed terminal results must serialize `{ "result":"committed", "workspace_id", "revision", "status_code":null, ... }`; validation_failed, stale, cancelled, cancelled_by_shutdown, failed_before_commit, and rolled_back terminal results must each serialize `{ "result": <variant>, "status_code": <registered ErrorCode>, "diagnostics": [...] }` with no `workspace_id`, no `revision`, and no `selected_workspace_id`; not_found status must not serialize a terminal result. Tests fail for untagged terminal result objects, omitted `result`, omitted `status_code` on terminal results, non-null committed codes, null failure/cancel/rollback codes, multiple codes, aliases, raw library/transport strings, unregistered literals, or fabricated persisted identifiers on any non-committed variant.
   - Add a Protocol Document value round-trip gate: construct typed `DocumentValue::String`, `DocumentValue::Int`, `DocumentValue::Bool`, and `DocumentValue::Blob`, assert canonical fixture/schema/expected-preview wire is exactly `{ "type": "string", "value": "abc" }`, `{ "type": "int", "value": 7 }`, `{ "type": "bool", "value": true }`, and `{ "type": "blob", "value": [0, 255] }`, deserialize through `DocumentCondition` and `DocumentAction`, and reserialize byte-for-byte.
   - Add a Protocol Document drift gate that fails when Domain serde emits any new `DocumentValue` type, `DocumentCondition` operator, `DocumentAction` type, or `ProtocolRuleStage` string not represented in fixture/schema/expected-preview.
   - Add a weak-network serde drift gate that serializes the current Domain `WeakNetworkProfile` with non-default nested values and asserts byte-for-byte canonical equality with the chosen DTO wire shape, including explicit `null` for absent `Option` values and snake_case enum strings.

2. DTO forbidden fields and unknown fields
   - Reject root `validation_request`.
   - Reject `workspace.name`, `workspace.description`, `certificate_references`, `secret_references`, `rules`, `protocol_rule_created_order_high_water`.
   - Reject client-submitted final `CertificateReference`, `SecretReference`, new Workspace ID, revision, selected Workspace ID, runtime epoch/state, active connection count, rule hit counters, and timestamps.
   - Reject `existing_rule_id` outside HTTP and Protocol Document rule templates, reject it for `target.mode=new`, and reject any attempt to use it as a final generated ID for new-rule input.
   - Reject unknown fields at root, target variants, workspace template, listener array items, HTTP settings, Socket settings, rule arrays, Android nested arrays, material items, and tagged variants through serde `deny_unknown_fields`.
   - Reject alternate or ambiguous weak-network forms: omitted `upload_bytes_per_second` instead of explicit `null`, omitted `burst_loss` instead of explicit `null` or object, `direction:"Upload"`, `flag:"SynAck"`, `mode:{ "Pass": {} }`, and any field outside the exact full object.
   - Reject Protocol Document scalar/raw value forms: `"abc"`, `7`, `true`, `[0,255]`, `{ "String": "abc" }`, `{ "type": "string" }`, `{ "value": "abc" }`, `{ "type": "float", "value": 1.25 }`, `{ "type": "object", "value": {} }`, nested array/object values, and any unknown field beside `type` and `value`.
   - Verify JSON schema recurses through `array.items` and all variants.

3. Alias graph
   - Resolve each certificate/secret role to every allowed consumer field.
   - Reject duplicate aliases, missing aliases, certificate-vs-secret type mismatch, role mismatch, zero-consumer aliases, unsupported multiple-consumer aliases, unsupported secret roles `upstream_basic_auth`/`downstream_basic_auth`, and unsupported certificate role `mitm_root_ca`.
   - Assert uploaded `mitm_root_ca` rejects before material staging, locks/leases, protector, SQLite, or Workspace persistence with `UNSUPPORTED_MATERIAL_ROLE`.
   - Assert MITM enabled validates the fixed installation root is present and valid; absent/invalid installation root fails create validation without staged MITM material.
   - Assert apply never changes installation root CA and running listeners continue using the lifecycle-frozen current root.
   - Assert final refs appear only after `EnvironmentCommitPort` commit and never in MCP create/status preview.
   - Assert all alias errors leave no persistent material or Workspace change.

4. Server-generated IDs
   - New Workspace ID generated by Application, never accepted from client.
   - New listener IDs generated when omitted; existing target may reference existing listener IDs.
   - Android profile IDs generated when omitted and validated when supplied.
   - HTTP `created_order`, Protocol Document `created_order`, rule IDs, revisions, and high-water mark generated server-side.
   - New HTTP and Protocol Document rules never accept final generated rule IDs; the only allowed client-side rule identity field is `existing_rule_id` for retained target rules in `target.mode=existing`.
   - Existing HTTP rule selector tests: valid `existing_rule_id` in the selected target Workspace with the same HTTP rule kind, exact listener binding, and exact HTTP stage preserves `RuleId` and HTTP `created_order` from the existing entity regardless of any submitted mutable draft value, updates mutable content/revision, and appears in preview as a retained rule.
   - Existing Protocol Document rule selector tests: valid `existing_rule_id` in the selected target Workspace with the same rule kind, exact listener binding, exact package ref, schema version, and stage preserves `ProtocolDocumentRuleId` and Protocol Document `created_order` from the existing entity regardless of any submitted mutable draft value, updates mutable content/revision, and appears in preview as a retained rule.
   - Negative selector tests reject duplicate `existing_rule_id`, unknown IDs, cross-workspace IDs, cross-kind IDs, cross-listener/cross-binding IDs, HTTP cross-stage IDs with `EXISTING_RULE_ID_STAGE_MISMATCH`, and for Protocol Document rules cross-package, cross-stage, and cross-schema-version IDs before shutdown/apply gates, runtime leases, package gates, material staging/protector, or SQLite.
   - Existing target rule removal tests assert old HTTP and Protocol Document target rules not referenced by `existing_rule_id` are classified as removed and are deleted at commit.

5. Unicode/new Workspace target key
   - `display_name = trim(input)`.
   - Empty after trim rejected.
   - Collision key is exact UTF-8 bytes hex.
   - Assert `A` and `Ａ`, NFC and NFD variants, and case differences do not collide.
   - Assert no ASCII-only, lowercase, casefold, or Unicode normalization behavior is introduced.

6. Tool budgets and annotations
   - Old read tools retain 256 KiB input, 8 MiB output, 8 second deadline.
   - Create has 1 MiB input/output and 30 second validation budget.
   - Status/cancel/apply have 16 KiB input and 8 second ack deadline.
   - Tool annotations exactly match PRD.
   - Public literal registry drift tests scan environment MCP tool DTOs, catalog projections, JSON schema, fixture, expected preview, validation layer outputs, diagnostics, status outputs, cancel outputs, apply outputs, capability warning outputs, and capability policy/retention outputs. The test fails on any warning/error/status/cancel-result/severity/layer/policy/tool/schema/capability-retention literal that is not registered in the PRD Stable Public Literal Registry.
   - Registry completeness tests assert every registered public literal is either emitted by a positive fixture, accepted by an input contract, or covered by a named negative fixture. The test must include warning literals `ipv6_unsupported`, `ipv6_dual_stack_covered`, and `IPV6_DEGRADED`; capability/policy literal `oldest_first`; cancel-result statuses `cancelled`, `apply_in_progress_not_cancellable`, and `not_found_or_terminal`; no cancel-result `cancelled_by_shutdown`; all candidate/apply statuses; all validation layer statuses; all terminal-result union `result` variants; all terminal-result `status_code` cases; package availability codes `PROTOCOL_PACKAGE_DISABLED` and `EXTERNAL_PACKAGE_OFFLINE`; Android runtime-owner code `ANDROID_RUNTIME_OWNER_ACTIVE`; all `existing_rule_id` selector error codes including HTTP `EXISTING_RULE_ID_STAGE_MISMATCH`; and all stable error codes.

7. Package SemVer and availability preflight
   - Parse and validate every `ProtocolPackageExactRef` into typed `ProtocolPackageRef` before target-key conflict checks, one-per-target capacity checks, sorting, or gate acquisition.
   - Invalid version returns `INVALID_PROTOCOL_PACKAGE_VERSION`.
   - Assert zero calls to shutdown/apply-state gate, runtime leases, package publication gates, material stage/protector, and SQLite.
   - Sorting accepts typed `ProtocolPackageVersion` only.
   - Create validation asserts every referenced exact package `id`/`version` exists in the Application-owned package projection and is enabled.
   - Disabled package fixture fails package projection validation with `PROTOCOL_PACKAGE_DISABLED`; missing package fixture fails with `PROTOCOL_PACKAGE_NOT_INSTALLED`; external package offline fixture fails with `EXTERNAL_PACKAGE_OFFLINE`.
   - Zero-RPC tests assert package validation sends no package RPC, no package health probe, no HTTP business body, no Socket frame, no Document decode/encode/Display, and no MAC/cipher call.
   - Apply lease/preflight race tests start from a valid preview and then mutate package projection before commit: exact package projection generation drift, package disappearance, enabled-flag change, and online-flag change each yield terminal `stale` with exact status code `CANDIDATE_STALE`, whether discovered by subscribed pre-apply invalidation or by apply preflight/lease recheck.
   - Package race tests assert precedence and mutually exclusive outcomes: package projection generation drift/disappearance/enabled/online changes never return `APPLY_LEASE_MISMATCH`, `PROTOCOL_PACKAGE_DISABLED`, or `EXTERNAL_PACKAGE_OFFLINE` after create has succeeded; when package projection change and another non-package guard mismatch are both detected in the same pre-commit check, package-specific `stale` + `CANDIDATE_STALE` wins.
   - All apply-time package availability failures assert no protected preparation, no package RPC/health probe/business bytes, no hot replacement, no SQLite transaction, and no Workspace persistence.

8. Candidate state/capacity/token/retention
   - Cover `validating`, `preview_ready`, `validation_failed`, `stale`, `cancelled`, `cancelled_by_shutdown`, `apply_queued`, `apply_in_progress`, `committed`, `rolled_back`, `failed_before_commit`.
   - Token created only for `preview_ready`, atomically consumed once while creating `apply_queued`.
   - Worker transitions `apply_queued` to `apply_in_progress` only after dequeueing and taking cleanup ownership.
   - Shutdown transitions `apply_queued` to `cancelled_by_shutdown`, zeroes material, and releases capacity; `apply_in_progress` drains to committed/rolled_back/failed_before_commit.
   - Normal cancel linearization tests deterministically race cancel against worker transition. Cancel-wins cases from `validating`, `preview_ready`, and `apply_queued` produce cancel-result `cancelled`, terminal status `cancelled`, zero private material, release capacity, and prove queued worker observation never prepares material, acquires commit authority, starts SQLite, or commits. Worker-wins case after `apply_in_progress` returns cancel-result `apply_in_progress_not_cancellable` and the worker drains to committed/rolled_back/failed_before_commit without interruption. Absent and already-terminal candidates return cancel-result `not_found_or_terminal`. Normal cancel never returns `cancelled_by_shutdown`; shutdown status remains observable only through create/status.
   - Token reuse returns `TOKEN_CONSUMED`.
   - Terminal public status retained only in process until exit and bounded by terminal retention limits.
   - Terminal retention limit tests advertise capability fields `terminal_retention.max_terminal_candidates=32`, `terminal_retention.max_terminal_public_bytes=4194304`, `terminal_retention.eviction="oldest_first"`, and `terminal_retention.evicted_status_code="CANDIDATE_NOT_FOUND"`; the literal `oldest_first` is covered by closed-registry drift/completeness tests.
   - Retention count tests create exactly N=32 terminal public results and assert all are queryable, then create N+1 and assert the oldest terminal is evicted first and status lookup returns `not_found` with `CANDIDATE_NOT_FOUND`.
   - Retention byte-budget tests retain exactly B=4194304 serialized public terminal/tombstone bytes, then exceed B+1 and assert deterministic oldest-first eviction until both count and byte budgets are satisfied.
   - Active-candidate protection tests assert non-terminal candidates are never evicted by terminal retention even when terminal count/byte budgets are exceeded.
   - Cleanup tests assert retained terminal results/tombstones contain no private material, prepared handles, plaintext, protected bytes, confirmation tokens, secret values, local secret paths, or business payloads; counters and oldest-terminal sequence are deterministic if exposed.
   - Max 4 active candidates, one active per target key, one active apply globally and per target.

9. Lease and ABA
   - `EnvironmentApplyLeasePort::acquire(scope)` returns guards and epoch snapshot.
   - Acquisition order is exactly shutdown/apply-state gate, Application mutation gate, sorted Listener IDs, sorted Android profile/device keys, sorted exact package refs/publication gates, then capture epochs/guards.
   - Different operations needing multiple gates follow the same order.
   - No registry-map lock, `parking_lot` guard, internal mutex, or RwLock guard is held across protector or `SqliteExecutor` await.
   - SQLite/protector/registry callbacks cannot acquire Application gate in reverse.
   - Listener/Android mutation and external package publication through gates block or queue while guard held.
   - External physical disconnect queues offline publication/epoch advance until guard release.
   - Value restored but epoch/generation changed still marks stale/fails before commit, except package projection generation drift/disappearance/enabled/online changes always use package-specific `stale` + `CANDIDATE_STALE`.
   - Generic guard acquisition failure or epoch mismatch maps to `failed_before_commit` or `stale` per PRD only for non-package guards. `APPLY_LEASE_MISMATCH` is reserved for non-package guarded epoch/non-DB generation mismatches; package projection generation drift/disappearance/enabled/online changes are excluded.
   - Lease precedence tests inject simultaneous package projection change and non-package guard mismatch and assert the terminal outcome is exactly package-specific `stale` with `CANDIDATE_STALE`, with no secondary `APPLY_LEASE_MISMATCH` result or availability error.
   - Affected-resource diff contract tests build an old Workspace and candidate Workspace and assert sorted `added`, `removed`, `changed`, and `unchanged` sets for Listener, Android profile/routing, protocol package refs, HTTP rule material refs, Protocol Document refs, and certificate/secret material refs.
   - Reference-only changes count as `changed`: update only a listener alias reference in an Android route, only a package version in HTTP body processing, only a package binding in a Protocol Document rule, and only a material alias edge while leaving the referenced payload unchanged.
   - HTTP rule diff tests assert the exact key `(bound_listener_target_key, "existing_rule_id", existing_rule_id)` for retained persisted rules, `(bound_listener_target_key, "candidate_index", candidate_http_rule_index)` for new candidate rules, and canonical content bytes that include retained selector identity, enabled, priority, matchers, conditions, action sequence, terminal action payloads, bound listener key, HTTP body protocol package ref, material alias edges, and every persistable mutable rule field.
   - Protocol Document rule diff tests assert the exact key `(bound_listener_target_key, package.id, semver(version), version_text, schema_version, stage, "existing_rule_id", existing_rule_id)` for retained persisted rules, `(bound_listener_target_key, package.id, semver(version), version_text, schema_version, stage, "candidate_index", candidate_protocol_rule_index)` for new candidate rules, and canonical content bytes that include retained selector identity, enabled, priority, package ref, schema version, stage, condition/action sequence, Document `{type,value}` payloads, reference edges, bound listener key, and every persistable mutable rule field.
   - Pure HTTP rule body-change test changes only matcher/action/body content for a retained HTTP rule and asserts the HTTP rule is `changed`, its bound Listener is lifted into affected runtime set, and no unrelated Listener is lifted.
   - Pure Protocol Document rule body-change test changes only Document condition/action/value content for a retained protocol rule and asserts the Protocol Document rule is `changed`, its bound Listener is lifted into affected runtime set, and no unrelated Listener is lifted.
   - Add/remove rule diff tests assert an old target rule omitted from the retained selector set is `removed`, a candidate rule without `existing_rule_id` is `added`, aliases for new rules are unique at candidate scope, and retained-plus-new mixtures generate stable preview entries without exposing final generated IDs for new rules.
   - Retained metadata tests assert HTTP retained rules preserve persisted `created_order` and Protocol Document retained rules preserve persisted `created_order` without a schema migration and without trusting submitted draft metadata.
   - Protocol Document retained-metadata drift tests assert DTO mapping, JSON schema, and expected preview expose the current `ProtocolDocumentRuleDefinition.created_order` field exactly, reject any legacy alias spelling, and require no alias migration.
   - Terminal-result persistence tests assert `committed` is the only terminal-result variant that contains persisted `workspace_id` and `revision`; validation_failed/stale/cancelled/cancelled_by_shutdown/failed_before_commit contain no persisted identifiers; rolled_back contains no new persisted identifiers whether no commit was attempted or a started transaction rolled back, and existing-target baseline context is exposed only through status context/diagnostics rather than as a committed revision.
   - Changed rule on unchanged active Listener rejects apply before protected preparation, hot rule replacement, SQLite transaction, or Workspace persistence with `RUNTIME_ACTIVE`.
   - Changed rule on unchanged stopped Listener proceeds past runtime-active rejection and reaches later lease/protector/commit checks.
   - Removed HTTP rule, added HTTP rule, removed Protocol Document rule, and added Protocol Document rule scope tests assert the bound Listener is lifted for each case.
   - Reference-only and material-only change scope tests assert every consuming Listener reached through the changed alias/package/reference edge is lifted, including HTTP rule consumers and Protocol Document rule consumers.
   - Lease-scope tests assert whole-Workspace scope for new target, selected Workspace changes, capacity/selection constraints, and unmappable diff; otherwise assert changed-resource-only gates.
   - Runtime rejection tests assert an unchanged active listener plus a changed stopped listener allows apply to proceed to later checks, while a changed, removed, added/lifted, starting, stopping, or active-connection listener rejects before protected preparation, hot rule replacement, SQLite transaction, or Workspace persistence with the stable runtime-active code.
   - Android runtime-owner tests assert affected Android profile/device targets proceed only when idle with no runtime owner. States `active`, `uncertain`, `waiting_reconnect`, `cleanup_required`, `stop_failed`, and `faulted` each reject before protected preparation, auto stop/recovery, hot replacement, SQLite transaction, or Workspace persistence with exact code `ANDROID_RUNTIME_OWNER_ACTIVE`; `idle` with no runtime owner reaches later lease/protector/commit checks. Tests assert this workflow performs no Android auto stop, auto recovery, or cleanup attempt.
   - No-hot-rule-replacement test asserts apply never swaps HTTP rules or Protocol Document rules in a running Listener as part of this workflow.

10. Protected material lifecycle
   - Create stage parse/validate/fingerprint only; no keychain/DB/file side effects.
   - Plaintext staged handles are opaque and zeroizing.
   - Apply preparation calls protector/keychain/master-key before SQLite transaction, returns opaque prepared handles, zeroes plaintext immediately.
   - Protector/keychain failure yields `PROTECTED_MATERIAL_PREPARE_FAILED`, `failed_before_commit`, no DB rows.
   - After commit/rollback, protected buffers are zeroed.

11. EnvironmentCommitPort architecture negatives
    - Candidate apply cannot call old restore/store/save/create/import methods or concrete SQLite APIs.
    - MCP cannot import concrete infrastructure adapters.
    - Stage ports expose no Application-callable finalization/reference method.
    - `EnvironmentCommitPort::commit(request)` is the only final-reference materialization path.
    - Lock-order static/architecture gate rejects reverse Application-gate acquisition from SQLite/protector/registry callbacks.

12. Shutdown
    - Shutdown begin rejects new create/apply with `SHUTDOWN_IN_PROGRESS`.
    - Queued not-yet-started apply becomes `cancelled_by_shutdown` and zeroes memory.
    - In-progress owned preparation/commit is awaited until terminal; mutation gate is not dropped mid-operation.
    - Hard kill during transaction leaves SQLite all-or-nothing; terminal status query after restart is not required.

## Integration Tests

1. `MCP-CONFIG-CONTRACT-001`
   - Exact schemas for capabilities/create/status/cancel/apply.
   - Full-shape fixture and per-field expected output.
   - Deep unknown-field/schema consistency tests.
   - Annotation and read-budget regression.
   - Closed public literal registry drift test over all environment MCP DTOs/catalog projections and every expected public output.

2. `MCP-CONFIG-CANDIDATE-001`
   - Create/status/cancel lifecycle.
   - Capacity and one-per-target key.
   - Event-driven stale invalidation for Workspace, selected Workspace, Listener runtime, Android runtime, package registry, certificate inventory, and secret inventory.
   - Pre-return disconnect and shutdown cancellation.

3. `MCP-CONFIG-VALIDATION-001`
   - Mandatory layers: schema, domain, certificate/secret material, package projection, DNS/TCP/port, TLS/mTLS.
   - Layer budgets/statuses: `passed`, `failed`, `cancelled`, `not_applicable`, `skipped_dependency`; no plain `skipped`.
   - Create total 30s dominates layer budgets: the in-flight layer at deadline is `cancelled` with reason `create_deadline_exceeded`, downstream not-started dependent layers are `skipped_dependency` with the same reason, completed layers retain their final status, and the response carries `MCP_CREATE_DEADLINE_EXCEEDED`.
   - Runtime-active listener creates preview but blocks apply.
   - Validation sends no business HTTP body, no Socket frame, no Document decode/encode/Display, no MAC/cipher call, no package RPC, no package health probe.

4. `MCP-CONFIG-ATOMIC-001`
   - Real SQLite/in-memory store plus environment commit adapter.
   - Existing branch revision CAS update.
   - New branch insert and selection semantics.
   - Prepared material dedupe/insert and alias rewrite.
   - Inject failures before transaction, during prepared row insert, alias rewrite, Workspace insert/update, and commit.
   - Assert zero residue after all rollback paths.
   - Hard-kill transaction atomicity test records all-or-nothing DB state.

5. `MCP-CONFIG-LEASE-CONTRACT-001`
   - Contract-only tests for `EnvironmentApplyLeasePort::acquire(scope)`, held guard shape, epoch snapshot fields, monotonic generation semantics, and failure mapping before any adapter integration.
   - ABA test where values restore but epoch/generation changes still marks candidate stale or fails before commit; package projection generation drift/disappearance/enabled/online changes are tested separately and always map to `stale` with `CANDIDATE_STALE`.

6. `MCP-CONFIG-LEASE-ADAPTER-001`
   - Listener and Android mutation adapters publish through gates.
   - Held guards block/queue Application-visible mutation publication while preserving lifecycle behavior for already-running listeners.
   - Cancellation while waiting for later listener/Android gate releases earlier guards in reverse order.

7. `MCP-CONFIG-LEASE-PACKAGE-001`
   - External package registry publication gates use typed package refs only.
   - Invalid SemVer has zero gate/protector/SQLite calls.
   - Exact package existence/enabled/online create validation maps disabled to `PROTOCOL_PACKAGE_DISABLED` and external offline to `EXTERNAL_PACKAGE_OFFLINE`.
   - Apply preflight rechecks exact package generation/enabled/online baseline; generation drift, package disappearance, enabled-flag change, and online-flag change all map to `stale`/`CANDIDATE_STALE`, whether discovered by subscribed pre-apply invalidation or apply preflight/lease recheck.
   - Precedence tests assert package-specific `stale`/`CANDIDATE_STALE` wins over generic `APPLY_LEASE_MISMATCH` when both package and non-package guard mismatches are present, and outcomes are mutually exclusive.
   - Zero-RPC tests prove package validation/preflight performs no package RPC, health probe, business payload, decode/encode/Display, MAC/cipher, Socket frame, or HTTP business body.
   - Physical external disconnect queues offline publication/epoch advance until guard release and does not imply eternal-online status after release.

8. `MCP-CONFIG-LEASE-ORDER-001`
   - Hold lease through preparation/commit/cleanup.
   - Verify canonical acquisition and reverse release order with deterministic sorted listener/profile/device/package inputs.
   - Queue external offline publication until release.
   - Assert commit linearization only depends on Application-observed generation at the transaction point.
   - Assert no claim of eternal external online status after guard release.
   - Run deadlock/order/cancel tests: cancellation while waiting for later gate releases earlier guards in reverse order and does not await while holding internal locks.
   - Run cancel linearization tests for cancel-wins, worker-wins, and terminal/not-found paths with exact cancel-result literals.
   - Integrated rule-scope rejection tests cover changed HTTP rule on active versus stopped Listener, changed Protocol Document rule on active versus stopped Listener, added/removed HTTP rule scope, added/removed Protocol Document rule scope, valid retained existing-rule selectors, illegal selector rejection before gates, reference-only scope, material-only scope, runtime listener lifting from retained body changes and omitted old rules, and no hot rule replacement.

9. `MCP-CONFIG-DISCONNECT-001`
   - Apply returns `{apply_task_id,status:"apply_queued"}` within ack deadline.
   - Status observes `apply_queued` before worker ownership when worker is intentionally paused.
   - Worker transition to `apply_in_progress` happens only after dequeue and cleanup ownership transfer.
   - Normal cancel wins against paused queued worker, terminal becomes `cancelled`, and worker observes terminal without prepare/commit.
   - Normal cancel loses after worker transition, returns `apply_in_progress_not_cancellable`, and does not interrupt owned preparation/commit.
   - Normal cancel for terminal or absent candidate returns `not_found_or_terminal`.
   - Disconnect after ack does not cancel owned task.
   - Status observes committed/rolled_back/failed_before_commit terminal state before App exit.

10. `MCP-CONFIG-IPV6-001`
   - Dual-stack success.
   - IPv4 bind failure starts no MCP service.
   - IPv4 success plus IPv6-only success.
   - IPv4 success plus IPv6 unsupported.
   - IPv4 success plus other IPv6 bind error reports warning `IPV6_DEGRADED` in `Vec<WarningCode>` and `ipv6.available=false`; it must not emit `IPV6_DEGRADED` as an error code.
   - Non-loopback IPv4 peer can call capabilities/create/status/apply when the platform route permits TCP connection.
   - Repeat a valid MCP request with Host values `proxy-admin.test`, `proxy-admin.test:17653`, `192.0.2.10:17653`, and `[2001:db8::10]:17653`; all reach MCP protocol handling and produce the same protocol-level result as the default Host.
   - Repeat a valid MCP request with Origin absent, `null`, `http://example.test`, `https://admin.example.test:8443`, and `app://local-tool`; all reach MCP protocol handling and produce the same protocol-level result.
   - Repeat without `Authorization`, with `Authorization: Bearer invalid`, with `X-API-Key: invalid`, and with a cookie; all reach MCP protocol handling and no response requests or validates auth credentials.
   - Assert there is no Host allowlist, no Origin gate, no source-IP/loopback rejection, no CIDR/private-network filter, and no auth header/token requirement.
   - Malformed HTTP, unsupported method/path, body over limit, invalid JSON-RPC/MCP envelope, missing required MCP protocol metadata, schema-invalid tool arguments, and invalid protocol messages reject with protocol correctness codes only.

## E2E Tests

1. `MCP-CONFIG-CHAIN-001`
   - Replay the full-shape fixture through MCP create/status/apply/status.
   - Verify preview, validation layers, alias graph, public cert metadata, secret refs without values, exact package refs, HTTP rules, protocol Document rules, fault behavior, Android profile, and selection.

2. `MCP-CONFIG-APP-001`
   - Must use packaged App. Dev App is supplemental only.
   - From non-loopback IPv4, create candidate, status, apply, status terminal, restart App, re-read Workspace.
   - Report no security/business-success overclaim.

3. Running runtime apply rejection
   - Candidate create previews while runtime active.
   - Apply rejects with runtime-active code before preparation/transaction and no persistence.

## Observability and Redline Tests

- Candidate diagnostics include candidate ID, target key, public baseline summary, status, and stable codes only.
- Logs/MCP outputs/previews/terminal results/serialized Workspace never include private key, certificate password, Basic auth password, protected blob bytes, raw request body, or local secret path.
- Public certificate label/metadata and secret aliases/labels remain present.
- Shutdown and hard-kill logs distinguish status unavailability from DB atomicity.

## Static and Architecture Checks

- `pnpm check`
- `pnpm scan:architecture`
- `pnpm scan:source-size`
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all --check`
- `cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features -- -D warnings`
- `cargo test --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features`
  - New architecture checks:
  - MCP adapter imports Application API only.
  - Domain crate has no MCP/rmcp/hyper/Tauri/SQLite/protector dependency.
  - Candidate apply calls no old restore/store/save/create/import commands.
  - Stage ports expose no Application-callable finalization/reference method.
  - Application sees opaque staged/prepared handles only, never bytes/protected records.
  - `EnvironmentCommitPort` is the only final-reference materialization path.
  - `mitm_root_ca` material is unsupported in v1; candidate apply never imports or replaces installation root CA.
  - Invalid package SemVer fails before target-key conflict checks, canonical sorting, any gate/lease/protector call, or SQLite.
  - Package availability validation and apply preflight use only the Application-owned projection and emit no package RPC/health probe/business bytes.
  - Cancel linearization uses one atomic state transition between normal cancel and worker ownership.
  - Terminal retention is bounded to 32 public terminal candidates and 4 MiB serialized public bytes, with no private material retained.
  - Affected Android target mutation is allowed only for idle/no-owner and blocks active/uncertain/waiting_reconnect/cleanup_required/stop_failed/faulted without auto stop/recovery.
  - All multi-gate operations use the canonical acquisition order and never acquire Application gate from SQLite/protector/registry callbacks.
  - ADR supersedes ADR-004.

## Pass Threshold

100% of acceptance criteria require direct evidence. The only allowed exception is environment-specific IPv6 capability; evidence must still prove accurate capability output and working IPv4.

## Stop Condition

Consensus is approved after Architect review 16 and Critic review 7. This test specification is ready for G033 execution handoff as a planning artifact. Execution completion still requires fresh implementation evidence, packaged App E2E, and mandatory whole-task adversarial review.
