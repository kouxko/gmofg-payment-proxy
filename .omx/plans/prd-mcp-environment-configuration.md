# PRD: MCP Environment Configuration

- Task: TASK-20260825-006
- Mode: RALPLAN deliberate planner draft, revision 16 after Architect review 15 APPROVE and Critic review 6 REQUEST CHANGES
- Status: Consensus approved, ready for G033 execution; planning artifact only
- Scope: Remote plaintext MCP tools for validated full Workspace environment configuration
- Non-scope: Client auth, MCP TLS transport, CIDR/source IP allowlist, business-message validation, Listener auto start/stop, cross-network eternal-online guarantee

## Outcome

MCP exposes an all-interface plaintext configuration workflow. A reachable client submits one complete `environment_configuration_candidate.v1` payload, receives mandatory technical validation and a private-material-free preview, then starts an Application-owned apply task with a one-use token. Apply checks Application-held runtime/package leases, prepares protected material outside the DB transaction, then commits Workspace plus protected certificate/secret records through one SQLite `IMMEDIATE` transaction. Any failure before commit leaves no new Workspace reference and no persistent material row; hard kill during commit relies only on SQLite atomicity.

## Source Evidence

- Product boundary/no-auth/plaintext/all-IP: `docs/tasks/pending/2026-08-25/mcp-environment-configuration.md:19`, `:24`, `:43`, `:111`.
- Direct MCP material input and memory-only pre-apply handling: `docs/tasks/pending/2026-08-25/mcp-environment-configuration.md:53`, `:57`, `:60`.
- Technical validation sends no business message: `docs/tasks/pending/2026-08-25/mcp-environment-configuration.md:76`, `:87`.
- Current MCP is loopback/read-only/read-budgeted: `src-tauri/src/mcp/server.rs:26`, `:64`, `:137`; `src-tauri/src/mcp/protocol.rs:103`; `src-tauri/src/mcp/catalog.rs:307`; `src-tauri/src/mcp/tests.rs:51`, `:79`.
- Workspace aggregate: `src-tauri/crates/domain/src/workspace.rs:43`, `:52`, `:56`, `:67`, `:71`.
- Listener/HTTP/TLS/Basic auth/Socket topology: `src-tauri/crates/domain/src/workspace/listener_model.rs:25`, `:32`, `:77`, `:102`, `:118`, `:154`, `:164`, `:190`, `:197`, `:204`, `:225`, `:245`, `:314`, `:332`, `:345`, `:358`.
- Socket runtime/topology: `src-tauri/crates/domain/src/workspace/socket_topology.rs:18`, `:37`, `:45`, `:57`, `:73`.
- HTTP rule fields/actions: `src-tauri/crates/domain/src/rule/types.rs:20`, `:29`, `:124`, `:167`.
- Protocol Document rule fields/actions/value wire and current `ProtocolDocumentRuleDefinition.created_order`: `src-tauri/crates/domain/src/protocol_document_rule.rs:54`, `:66`, `:117`, `:174`, `:209`, `:215`; `src-tauri/crates/domain/src/protocol_document_rule/wire.rs:8`, `:41`; `src-tauri/crates/domain/src/document/model.rs:7`.
- Android profile fields and validation: `src-tauri/crates/domain/src/android_network.rs:168`, `:181`, `:260`, `:295`.
- Exact package refs: `src-tauri/crates/domain/src/protocol_package/identity.rs:161`.
- Workspace name is trim-only/non-empty: `src-tauri/crates/application/src/workspaces.rs:262`; `src-tauri/crates/domain/src/workspace.rs:94`.
- Protected secret/protector boundaries: `src-tauri/crates/application/src/facade/secrets.rs:12`, `:42`; `src-tauri/crates/host/src/platform.rs:21`, `:31`.
- SQLite aggregate writes use `IMMEDIATE`: `src-tauri/crates/infrastructure/src/sqlite/workspaces.rs:253`.

## Required Architecture

Application owns candidate registry, validation orchestration, confirmation tokens, target capacity, shutdown state, runtime/package lease acquisition, apply task lifecycle, and terminal status. MCP owns only wire parsing/catalog/schema/transport adaptation. Domain owns pure validation/model invariants. Infrastructure owns concrete protected-material preparation and SQLite transaction mechanics.

### EnvironmentApplyLeasePort

Non-DB consistency uses one implementable Application lease boundary:

```rust
trait EnvironmentApplyLeasePort {
    async fn acquire(&self, scope: EnvironmentApplyScope) -> AppResult<EnvironmentApplyLeaseGuard>;
}
```

`EnvironmentApplyLeaseGuard` contains held guards and an epoch snapshot for affected Listener runtime mutation gates, Android network mutation gates, external package registry publication gates, package online generation, certificate inventory generation, and protected secret inventory generation.

All Listener/Android mutation and external-package status publication must pass through corresponding gates. Physical external Socket disconnect cannot be prevented; its offline publication and epoch advance are queued until guard release. The apply guarantee is only: at the transaction linearization point, the exact connection generation observed by Application before commit has not changed. If the connection drops immediately after guard release, committed Workspace state is not rolled back.

If guard acquisition fails, shutdown has begun, runtime is active, or any guarded epoch mismatches, protected preparation and DB transaction do not start. Candidate terminal state is `failed_before_commit`, except subscribed stale events before apply set terminal `stale`. Package projection generation drift, package disappearance, enabled-flag changes, and online-flag changes are excluded from this generic guarded-epoch rule and always use the package-specific stale mapping below.

Canonical acquisition order:

1. Shutdown/apply-state gate.
2. Application mutation gate.
3. Affected Listener gates sorted by canonical Listener UUID string.
4. Affected Android profile/device gates sorted by tuple `(profile_id, device_key)` using UTF-8 byte order for strings and numeric order for integers.
5. Exact `ProtocolPackageRef` and external publication gates sorted by tuple `(package.id, semver(version), version_text)`. `package.id` sorts by ASCII bytes; `semver(version)` sorts by semantic SemVer order; `version_text` is the deterministic tie-breaker for build metadata text.
6. Capture epoch/guard snapshots.

Every operation that needs more than one of these gates follows this same total order. After logical lease guards are acquired, implementation must release all registry-map locks, `parking_lot` guards, and other internal mutex/RwLock guards before any `.await` into protector or `SqliteExecutor`. The protector may run while logical lease guards are held, but it must not hold internal registry/Application locks across await. SQLite/protector/registry callbacks must not acquire the Application mutation gate in reverse. Guards release in exact reverse acquisition order only after SQLite transaction, terminal status write, and memory cleanup complete.

### Protected material sequence

1. Create stage parses, validates, classifies, and fingerprints certificate/secret input. It stores plaintext only in `Zeroizing` memory behind opaque staged handles. It has no keychain, protected store, file, or DB side effect. Create cancellation, validation failure, stale, shutdown before return, or pre-return disconnect zeroes plaintext buffers.
2. Apply preparation runs after token consumption and lease acquisition. Application calls an infrastructure preparation port with opaque staged handles. The port invokes platform protector/keychain/master-key initialization if needed, converts plaintext into opaque prepared protected handles, and immediately zeroes plaintext. Failure maps to `PROTECTED_MATERIAL_PREPARE_FAILED` and terminal `failed_before_commit`; no DB transaction starts and no DB rows exist. Application receives only opaque prepared handles, never bytes or infrastructure records.
3. Commit uses `EnvironmentCommitPort::commit(request)` to start one SQLite `IMMEDIATE` transaction. It rechecks persistable baseline/CAS, dedupes prepared material by fingerprint, inserts protected rows, rewrites aliases to final refs, inserts/updates Workspace, and applies selection semantics. It does not invoke the protector.
4. Cleanup zeroes prepared buffers after commit success or rollback. Candidate private memory is zeroed at every terminal state. Hard kill cannot guarantee terminal status query after restart, but SQLite atomicity must preserve DB all-or-nothing.

### EnvironmentCommitPort

There is exactly one Application-callable commit boundary:

```rust
trait EnvironmentCommitPort {
    async fn commit(&self, request: EnvironmentCommitRequest) -> AppResult<EnvironmentCommitResult>;
}
```

Application must not call old certificate restore/store ports, protected secret store ports, Workspace save/import/create methods, or concrete SQLite APIs during candidate apply. Architecture tests fail if candidate apply imports or calls `restore_certificate_materials`, `listener_import_*`, `store_basic_auth`, `workspace_save`, `workspace_create`, `workspace_import`, or concrete SQLite adapter APIs.

Commit request fields:

- `baseline: EnvironmentCandidateBaseline`.
- `target: Existing { workspace_id, expected_revision } | New { workspace_id_generated_by_application, display_name }`.
- `workspace_template: WorkspaceCommitTemplate`.
- `prepared_certificate_handles: BTreeMap<MaterialAlias, PreparedProtectedMaterialHandle>`.
- `prepared_secret_handles: BTreeMap<MaterialAlias, PreparedProtectedMaterialHandle>`.
- `selection_policy: PreserveExistingSelectionOrSelectNewWhenNone`.

Existing target updates exactly one row by `workspace_id` and `expected_revision`. New target inserts exactly one row with Application-generated `workspace_id` and trim-only `display_name`. Existing update never changes selected Workspace. New insert changes selection only when current selected Workspace is null. Result is `EnvironmentCommitResult { workspace_id, revision, selected_workspace_id, reused_materials, inserted_materials, diagnostics }`. Any failure before DB commit rolls back and leaves no new material rows, no secret rows, no Workspace refs.

## `environment_configuration_candidate.v1` DTO

All DTO structs/enums use typed serde `deny_unknown_fields`, including nested array items and tagged variants. Runtime serde parse is authoritative; JSON schema is only catalog/schema consistency. Client input cannot submit final `CertificateReference`, final `SecretReference`, persisted `WorkspaceId` for new targets, persisted `Revision`, runtime state, selected Workspace state, hit counters, timestamps, active connection counts, or final protected record references. `existing_rule_id` is the only narrow rule-identity selector exception: it is accepted only on HTTP and Protocol Document rule templates for `target.mode=existing`, is never a final generated ID for a new rule, and is rejected everywhere else.

### Root and target

Root request fields:

- `schema_version`: literal `1`.
- `target: EnvironmentTarget`.
- `workspace: WorkspaceCommitTemplate`.
- `materials: EnvironmentMaterials`.

Forbidden root fields: `validation_request`, `workspace_id`, `revision`, `selected_workspace_id`, runtime state.

`EnvironmentTarget`:

- Existing: `{ "mode": "existing", "workspace_id": "<uuid>", "expected_revision": <u64> }`.
- New: `{ "mode": "new", "name": "<string>" }`.

New Workspace normalization:

- `display_name = name.trim()`.
- Empty after trim is rejected.
- Collision key is exact `hex(utf8_bytes(display_name))`.
- `A` and `Ａ`, NFC and NFD variants, and case differences intentionally do not collide.
- No ASCII-only, Unicode casefold, lowercase conversion, normalization, or locale behavior is introduced.
- `WorkspaceCommitTemplate` has no `name`; target is the only name source.

### WorkspaceCommitTemplate

Fields:

- `listeners: Vec<ListenerTemplate>`, max 8.
- `http_rules: Vec<HttpRuleTemplate>`, max 128; maps to current `RuleDraft`.
- `protocol_rules: Vec<ProtocolDocumentRuleTemplate>`, max 128; maps to current `ProtocolDocumentRuleDraft`.
- `android_network_profiles: Vec<AndroidNetworkProfileTemplate>`, max 8.

Forbidden fields:

- `name`: target owns display name.
- `description`: forbidden because current `ProxyWorkspace` has no description field.
- `certificate_references`: generated from certificate aliases.
- `secret_references`: generated from secret aliases.
- `rules`: use `http_rules`.
- `id`, `revision`, `protocol_rule_created_order_high_water`, selected state, runtime state.

### ListenerTemplate

Fields copied from `ProxyListener`, with server-generated identity:

- `alias: string`, ASCII `[A-Za-z0-9_.-]{1,64}`, unique in the candidate.
- `id`: optional UUID only for `target.mode=existing` and an existing Listener; new listeners omit it and Application generates `ListenerId`.
- `name: string`.
- `enabled: bool`.
- `bind_address: string`.
- `port: u16`.
- `connect_timeout_ms: u64`.
- `read_timeout_ms: u64`.
- `write_timeout_ms: u64`.
- `data_plane: ListenerDataPlaneTemplate`.

Forbidden listener fields: runtime status, active connections, local bound address, runtime epoch, diagnostics, final certificate refs outside alias fields.

`ListenerDataPlaneTemplate`:

- HTTP: `{ "kind": "http", "settings": HttpListenerTemplate }`.
- Socket: `{ "kind": "socket", "settings": SocketListenerTemplate }`.

### HTTP listener templates

`HttpListenerTemplate` fields:

- `authentication`: `{ "mode": "none" } | { "mode": "basic", "credential_alias": "<secret alias role proxy_basic_auth>" }`.
- `mitm: MitmTemplate`.
- `downstream_tls: DownstreamTlsTemplate`.
- `request_body_codec: auto | raw | utf8 | shift_jis`.
- `response_body_codec: auto | raw | utf8 | shift_jis`.
- `body_processing`: `{ "mode": "plain" } | { "mode": "protocol", "package": ProtocolPackageExactRef }`.
- `fixed_server: null | FixedServerTemplate`.

`MitmTemplate`: `enabled: bool`, `authority_allowlist: Vec<string>`, `root_ca_selector: null | "installation:root-ca"`, `maximum_cached_leaf_certificates: u16`.

MITM root ownership is installation-scoped, not Workspace-scoped. v1 cannot submit, stage, import, replace, or mutate a MITM root CA. When `enabled=true`, `root_ca_selector` must be exactly `"installation:root-ca"` and create validation verifies the installation-owned root exists and is currently valid. When `enabled=false`, `root_ca_selector` must be `null`. Uploaded material role `mitm_root_ca` is rejected before material staging, lease acquisition, protector calls, or persistence with stable `UNSUPPORTED_MATERIAL_ROLE`. Apply never changes the installation root. Running listeners keep the lifecycle-defined frozen root already in use; candidate/apply cannot retroactively swap it.

`DownstreamTlsTemplate`: `enabled: bool`, `server_identity_alias: null | <certificate alias role downstream_server_identity>`, `dynamic_sni_allowlist: Vec<string>`, `client_authentication: disabled | optional(trust_alias) | required(trust_alias)` where trust alias role is `downstream_client_trust`.

`FixedServerTemplate`: `upstream_url: string` must be an HTTP/HTTPS origin only, with no path,
query, fragment, or userinfo; `upstream_tls: HttpUpstreamTlsTemplate`.

`HttpUpstreamTlsTemplate`: `verify_hostname: bool`, `server_trust_alias: null | <certificate alias role upstream_server_trust>`, `client_identity_alias: null | <certificate alias role upstream_client_identity>`.

### Socket listener templates

`SocketListenerTemplate` fields:

- `topology: SocketTopologyTemplate`.
- `maximum_connections: u16`, current domain maximum 5000.
- `runtime_limits: SocketRuntimeLimitsTemplate`.
- `processing: SocketPayloadProcessingTemplate`.

`SocketRuntimeLimitsTemplate`: `read_chunk_bytes: u32`, `diagnostic_event_capacity: u32`, `diagnostic_memory_bytes: u64`.

`SocketTopologyTemplate`:

- Relay: `{ "mode": "relay", "settings": { "upstream": { "host": "string", "port": u16 }, "security": SocketRelaySecurityTemplate } }`.
- Local responder: `{ "mode": "local_responder", "settings": { "downstream_security": SocketDownstreamSecurityTemplate } }`.

`SocketRelaySecurityTemplate`:

- `transparent`: no TLS fields.
- `tcp_to_tls`: `upstream_tls`.
- `tls_to_tcp`: `downstream_tls`.
- `tls_to_tls`: both `downstream_tls` and `upstream_tls`.

`SocketDownstreamSecurityTemplate`:

- `tcp`: no TLS fields.
- `tls`: `downstream_tls`.

`SocketDownstreamTlsTemplate`: `server_identity_alias: <certificate alias role downstream_server_identity>`, `client_authentication: disabled | optional(trust_alias) | required(trust_alias)`.

`SocketUpstreamTlsTemplate`: `verify_hostname: bool`, `tls_server_name: null | string`, `server_trust_alias: null | <certificate alias role upstream_server_trust>`, `client_identity_alias: null | <certificate alias role upstream_client_identity>`.

`SocketPayloadProcessingTemplate`:

- Direct: `{ "mode": "direct" }`.
- Scripted: `{ "mode": "scripted", "settings": { "package": ProtocolPackageExactRef } }`.

### Package refs

`ProtocolPackageExactRef` maps to `ProtocolPackageRef`:

- `id: string`, `[a-z][a-z0-9-]*`, max 64 ASCII bytes.
- `version: string`, exact SemVer text, max 128 ASCII bytes.
- No version range, latest selector, fallback package, package RPC, or auto-upgrade field.

All package references are parsed into typed `ProtocolPackageRef` and typed SemVer before target-key conflict checks, candidate one-per-target checks, canonical package sorting, or gate 0 acquisition. Invalid version text returns `INVALID_PROTOCOL_PACKAGE_VERSION` and must produce zero calls to shutdown/apply gates, runtime leases, package publication gates, material staging/protector, or SQLite. Canonical ordering accepts only typed package refs.

Create validation then verifies every referenced package exact `id`/`version` exists in the current Application-owned package projection and is enabled. External packages must also be online in that same projection. Disabled packages fail package projection validation with `PROTOCOL_PACKAGE_DISABLED`; external packages currently offline fail with `EXTERNAL_PACKAGE_OFFLINE`. Validation must not call package RPC, health probes, decode/encode/Display, MAC/cipher, Socket frames, HTTP business bodies, or any business-byte exchange.

Apply lease/preflight rechecks the exact package baseline captured at create: exact `id`/`version`, projection generation, enabled flag, external-online flag, description fingerprint, and lease generation. Package projection generation drift, package disappearance, enabled-flag changes, and online-flag changes discovered either by subscribed pre-apply invalidation or by apply preflight/lease recheck always map to terminal `stale` with exact status code `CANDIDATE_STALE`. This package-specific stale mapping has precedence over generic guarded epoch/non-DB generation mismatch handling and is mutually exclusive with `APPLY_LEASE_MISMATCH`, `PROTOCOL_PACKAGE_DISABLED`, and `EXTERNAL_PACKAGE_OFFLINE` for post-create package drift. `PROTOCOL_PACKAGE_DISABLED` and `EXTERNAL_PACKAGE_OFFLINE` remain create-validation package availability errors only. All apply-time package stale failures happen before protected preparation, package RPC/health probe/business bytes, hot replacement, SQLite transaction, or Workspace persistence.

### HTTP rule template

`HttpRuleTemplate` maps to current `RuleDraft`:

- `existing_rule_id: null | RuleId`; allowed only when `target.mode=existing`, forbidden when `target.mode=new`, and required to reference an existing HTTP rule in the selected target Workspace with the same HTTP rule kind, the exact listener binding selected by `listener_alias`, and the exact HTTP `stage`.
- `name: string`.
- `description: string`.
- `enabled: bool`.
- `priority: u32`.
- `listener_alias: string`; replaces channel/listener binding and must resolve to an HTTP listener.
- `stage: request | response | tls_handshake`.
- `conditions: Vec<HttpMatchConditionTemplate>`.
- `actions: Vec<HttpRuleActionTemplate>`.
- `one_shot: bool`.

Server-generated/excluded for ordinary new-rule input: `expected_revision`, `created_order`, `RuleId`, `revision`, `hit_count`, `last_hit_at`, runtime epoch. `existing_rule_id` is not a generated-ID submission path; it is a selector for retaining an existing target rule identity only in `target.mode=existing`.

Existing HTTP rule identity rules:

- For `target.mode=existing`, `existing_rule_id` may be non-null only when it resolves to exactly one HTTP rule in the selected target Workspace.
- The resolved rule must be the same rule kind (`http`) and must have the exact listener binding selected by `listener_alias` after alias resolution and the exact submitted HTTP `stage`. Cross-workspace, cross-kind, cross-binding, cross-stage, duplicate, and unknown IDs fail before material staging, validation gates, protected preparation, hot replacement, SQLite transaction, or persistence.
- A given existing HTTP rule ID may appear at most once in the candidate.
- For `target.mode=new`, `existing_rule_id` is forbidden and all HTTP rules must use explicit candidate aliases/bindings; commit generates final `RuleId`, `created_order`, and revision.
- Existing target HTTP rules not referenced by any `existing_rule_id` are removed by the candidate. Referenced rules preserve their `RuleId` and existing HTTP `created_order` from the persisted entity regardless of any submitted mutable draft field, while mutable content and revision update according to the existing Domain persistence contract.

`HttpMatchConditionTemplate`:

- Semantics: field conditions over terminal IP, certificate fingerprint, path/request type, or JSONPath; operators are equals, contains, or regex.
- Nth hit condition carries a positive hit count.
- Exact JSON wire is only the Tagged JSON Wire Appendix shape.

`HttpRuleActionTemplate`:

- Set JSON field: path plus JSON value.
- Replace body text: text value.
- Set header: header name plus value.
- Delay: milliseconds.
- Jitter: minimum/maximum milliseconds plus before-message or per-chunk scope.
- Throttle/intermittent: rate/window fields plus upstream/downstream direction.
- Pause and custom HTTP status.
- Terminal actions: `reject_tls_handshake`, `disconnect_before_upstream`, `upstream_connect_timeout`, `upstream_write_timeout`, `upstream_read_timeout`, `drop_upstream_response`, `mock_response`, `invalid_json`, `incorrect_content_length`, `truncate_response`, `disconnect_during_upstream_write`, `disconnect_during_downstream_write`.

Terminal action semantics match current variants: timeout actions carry milliseconds; dropped response carries a drop mode; mock response carries status, ordered header pairs, and raw body bytes; invalid JSON carries raw body bytes; content length carries delta; truncate/disconnect actions carry byte counts. Exact JSON wire is only the Tagged JSON Wire Appendix shape.

### Protocol Document rule template

`ProtocolDocumentRuleTemplate` maps to `ProtocolDocumentRuleDraft`:

- `existing_rule_id: null | ProtocolDocumentRuleId`; allowed only when `target.mode=existing`, forbidden when `target.mode=new`, and required to reference an existing Protocol Document rule in the selected target Workspace with the same rule kind, exact listener binding selected by `listener_alias`, exact package ref, `schema_version`, and `stage`.
- `name: string`.
- `enabled: bool`.
- `priority: i32`.
- `listener_alias: string`; replaces `listener_id` and must resolve to a Listener whose HTTP body processing or Socket processing binds the same package.
- `package: ProtocolPackageExactRef`.
- `schema_version: u32`.
- `stage: app_to_proxy | proxy_to_upstream | upstream_to_proxy | proxy_to_app`.
- `conditions: Vec<DocumentConditionTemplate>`.
- `actions: Vec<DocumentActionTemplate>`.

Excluded/server-generated for ordinary new-rule input: `ProtocolDocumentRuleId`, `revision`, `created_order`, high-water mark. `existing_rule_id` is the narrow existing-target selector exception and is not allowed for new rules.

Existing Protocol Document rule identity rules:

- For `target.mode=existing`, `existing_rule_id` may be non-null only when it resolves to exactly one Protocol Document rule in the selected target Workspace.
- The resolved rule must be the same rule kind (`protocol_document`) and must match the exact listener binding selected by `listener_alias`, exact package ref, `schema_version`, and `stage`. Cross-workspace, cross-kind, cross-binding, cross-package, cross-schema-version, cross-stage, duplicate, and unknown IDs fail before material staging, validation gates, protected preparation, hot replacement, SQLite transaction, or persistence.
- A given existing Protocol Document rule ID may appear at most once in the candidate.
- For `target.mode=new`, `existing_rule_id` is forbidden and all Protocol Document rules use explicit listener/package/stage inputs; commit generates final `ProtocolDocumentRuleId`, `created_order`, and revision.
- Existing target Protocol Document rules not referenced by any `existing_rule_id` are removed by the candidate. Referenced rules preserve their `ProtocolDocumentRuleId` and existing Protocol Document `created_order` from the persisted entity regardless of any submitted mutable draft field, while mutable content and revision update according to the existing Domain persistence contract. DTO mapping, JSON schema, expected preview, and drift tests must use the current `ProtocolDocumentRuleDefinition.created_order` field exactly, with no legacy alias, compatibility migration, or alternate accepted spelling.

`DocumentConditionTemplate`: `equals { field: DocumentFieldName, value: DocumentValueTemplate }`.

`DocumentActionTemplate`: `record_match {}`, `set_field { field, value: DocumentValueTemplate }`, `clear_field { field }`, `clear_document {}`.

`DocumentValueTemplate` is the exact current adjacent-tag Protocol Document value wire:

- String: `{ "type": "string", "value": "abc" }`.
- Int: `{ "type": "int", "value": 7 }`.
- Bool: `{ "type": "bool", "value": true }`.
- Blob: `{ "type": "blob", "value": [0, 255] }`, where `value` is a JSON array of unsigned bytes `0..255`.

Nested arrays and objects never replace this `{type,value}` shape. `conditions[].value` and `actions[].value` must use one of the four shapes above. Scalar strings, numbers, booleans, raw byte arrays without the outer tag, heterogeneous arrays, object maps, whole-container Set/Equals, `null`, and any unknown `type` are rejected.

No HTTP JSONPath, wildcard array selector, package RPC, business payload, MAC/cipher, decode/encode execution, or Display call is allowed during create validation.

### AndroidNetworkProfileTemplate

Maps to `AndroidNetworkProfile`:

- `id`: optional safe ID `[A-Za-z0-9_.-]{1,128}`. If omitted, Application generates a stable candidate-local ID. Existing target may update by supplied existing ID; omitted creates new.
- `name: string`, trim-nonempty, max 80 chars.
- `target_applications: Vec<{ package_name: string, uid: u32 > 0, display_name: null | string }>`; 1..64.
- `destination_targets: Vec<{ cidr: string, ports: Vec<u16> }>`; max 128; current Android route contract accepts single IP or IPv4/IPv6 CIDR.
- `proxy_routes: Vec<{ destination: string, ports: Vec<u16>, listener_alias: string }>`; max 128; listener alias replaces final `listener_id`.
- `confirmed_shared_uids: Vec<u32>` stored as a set.
- `auto_resume_after_reboot: bool`.
- `weak_network: WeakNetworkProfileTemplate`, JSON <= 256 KiB.

`WeakNetworkProfileTemplate` is a required full object. Every field below is required in JSON; `Option` values are represented only by explicit JSON `null` or the typed value, never by omission. Unknown fields are rejected at every nested level.

```json
{
  "seed": 1,
  "fixed_delay_millis": 0,
  "uniform_jitter_millis": 0,
  "upload_bytes_per_second": null,
  "download_bytes_per_second": null,
  "random_loss_basis_points": 0,
  "burst_loss": {
    "enter_bad_state_basis_points": 100,
    "leave_bad_state_basis_points": 9000,
    "good_state_loss_basis_points": 25,
    "bad_state_loss_basis_points": 7500
  },
  "duplicate_basis_points": 0,
  "reorder_basis_points": 0,
  "maximum_reorder_hold_millis": 0,
  "blackout_windows": [
    {
      "start_after_millis": 1000,
      "duration_millis": 500
    }
  ],
  "dns_blackhole": false,
  "nth_tcp_flag_drops": [
    {
      "direction": "upload",
      "flag": "syn",
      "nth": 1
    },
    {
      "direction": "download",
      "flag": "syn_ack",
      "nth": 2
    }
  ],
  "path_mtu": {
    "mtu": null,
    "mss_clamp": null,
    "mode": "pass"
  },
  "corruption": {
    "probability_basis_points": 0,
    "bits_per_packet": 0
  }
}
```

Accepted enum strings are exactly: `PacketDirection` as `upload` or `download`; `TcpFlag` as `syn`, `syn_ack`, `ack`, `fin`, or `rst`; `PmtuMode` as `pass`, `fragment_or_packet_too_big`, `signal_too_big`, or `blackhole`. `burst_loss` may be explicit `null` or the four-field object shown above. `upload_bytes_per_second`, `download_bytes_per_second`, `path_mtu.mtu`, and `path_mtu.mss_clamp` may be explicit `null` or positive integers accepted by the current Domain/Application validation. Empty arrays are valid for `blackout_windows` and `nth_tcp_flag_drops`.

### Materials

`EnvironmentMaterials`:

- `certificates: Vec<CertificateMaterialInput>`, max 16.
- `secrets: Vec<SecretMaterialInput>`, max 16.

`CertificateMaterialInput`:

- `alias: string`, ASCII `[A-Za-z0-9_.-]{1,64}`, unique among certificates.
- `role: downstream_server_identity | downstream_client_trust | upstream_client_identity | upstream_server_trust`.
- `encoding: pem | base64_der | pkcs12_base64`.
- `content: string`, max 256 KiB decoded.
- `password: null | string`, max 4 KiB, required when encrypted identity/PKCS#12 parsing requires it.
- `label: string`, trim-nonempty; becomes public certificate label.

`SecretMaterialInput`:

- `alias: string`, ASCII `[A-Za-z0-9_.-]{1,64}`, unique among secrets.
- `role: proxy_basic_auth`.
- `username: string`, trim-nonempty, no colon for HTTP Basic.
- `password: string`, nonempty, max 4 KiB.
- `label: string`, trim-nonempty; public only.

`mitm_root_ca`, `upstream_basic_auth`, and `downstream_basic_auth` are not v1 material roles. Because role is a closed enum, they fail closed as unknown/unsupported input and must not persist anything. `mitm_root_ca` specifically maps to stable `UNSUPPORTED_MATERIAL_ROLE` before staging because the only v1 MITM root source is the installation-owned `installation:root-ca`.

Alias errors:

- duplicate within certificate or secret set: `MATERIAL_ALIAS_DUPLICATE`.
- missing reference: `MATERIAL_ALIAS_MISSING`.
- certificate/secret type mismatch or role mismatch: `MATERIAL_ALIAS_TYPE_MISMATCH`.
- submitted material with zero consumers: `MATERIAL_ALIAS_UNUSED`; no persistence.
- submitted material with more than one consumer is allowed only when every consumer role explicitly permits shared alias reuse. v1 allows multiple consumers for trust aliases (`downstream_client_trust`, `upstream_server_trust`) and rejects multiple consumers for identity/credential aliases (`downstream_server_identity`, `upstream_client_identity`, `proxy_basic_auth`) unless a later version explicitly widens this.
- Final refs are generated only by commit: certificate alias + role -> `CertificateReference { id, label, kind, reference }`; secret alias + role -> `SecretReference { provider, key }`.

## Tagged JSON Wire Appendix

This appendix is the single authoritative JSON wire contract. Earlier DTO sections define semantics and field ownership only; implementation, fixture, schema snapshot, and preview expected output must use the exact representation here, with no scalar shorthand or alternate tagged shape. The single authoritative fixture path for execution is `src-tauri/src/mcp/tests/fixtures/environment_configuration_candidate_v1/full-shape.json`; its schema snapshot and field-by-field expected preview live beside it as `schema.snapshot.json` and `expected-preview.json`.

Exact tagged variants:

- `EnvironmentTarget`: `{ "mode": "existing", "workspace_id": "00000000-0000-0000-0000-000000000001", "expected_revision": 1 }` or `{ "mode": "new", "name": "Store Lab" }`.
- `ListenerDataPlaneTemplate`: `{ "kind": "http", "settings": { "authentication": { "mode": "none" }, "mitm": { "enabled": false, "authority_allowlist": [], "root_ca_selector": null, "maximum_cached_leaf_certificates": 0 }, "downstream_tls": { "enabled": false, "server_identity_alias": null, "dynamic_sni_allowlist": [], "client_authentication": { "mode": "disabled" } }, "request_body_codec": "auto", "response_body_codec": "auto", "body_processing": { "mode": "plain" }, "fixed_server": null } }` or `{ "kind": "socket", "settings": { "topology": { "mode": "local_responder", "settings": { "downstream_security": { "mode": "tcp" } } }, "maximum_connections": 16, "runtime_limits": { "read_chunk_bytes": 4096, "diagnostic_event_capacity": 256, "diagnostic_memory_bytes": 1048576 }, "processing": { "mode": "direct" } } }`.
- `authentication`: `{ "mode": "none" }` or `{ "mode": "basic", "credential_alias": "proxy-admin" }`.
- `MitmTemplate`: `{ "enabled": true, "authority_allowlist": ["example.com"], "root_ca_selector": "installation:root-ca", "maximum_cached_leaf_certificates": 256 }` or disabled as `{ "enabled": false, "authority_allowlist": [], "root_ca_selector": null, "maximum_cached_leaf_certificates": 0 }`.
- `body_processing`: `{ "mode": "plain" }` or `{ "mode": "protocol", "package": { "id": "au-eftex", "version": "1.1.0" } }`.
- `client_authentication`: `{ "mode": "disabled" }`, `{ "mode": "optional", "trust_alias": "downstream-trust" }`, or `{ "mode": "required", "trust_alias": "downstream-trust" }`.
- `SocketTopologyTemplate`: `{ "mode": "relay", "settings": { "upstream": { "host": "pay.example.test", "port": 443 }, "security": { "mode": "transparent" } } }` or `{ "mode": "local_responder", "settings": { "downstream_security": { "mode": "tcp" } } }`.
- `SocketRelaySecurityTemplate`: `{ "mode": "transparent" }`, `{ "mode": "tcp_to_tls", "upstream_tls": { "verify_hostname": true, "tls_server_name": "pay.example.test", "server_trust_alias": "upstream-trust", "client_identity_alias": null } }`, `{ "mode": "tls_to_tcp", "downstream_tls": { "server_identity_alias": "listener-identity", "client_authentication": { "mode": "disabled" } } }`, or `{ "mode": "tls_to_tls", "downstream_tls": { "server_identity_alias": "listener-identity", "client_authentication": { "mode": "required", "trust_alias": "downstream-trust" } }, "upstream_tls": { "verify_hostname": true, "tls_server_name": "pay.example.test", "server_trust_alias": "upstream-trust", "client_identity_alias": "upstream-client" } }`.
- `SocketDownstreamSecurityTemplate`: `{ "mode": "tcp" }` or `{ "mode": "tls", "downstream_tls": { "server_identity_alias": "listener-identity", "client_authentication": { "mode": "optional", "trust_alias": "downstream-trust" } } }`.
- `SocketPayloadProcessingTemplate`: `{ "mode": "direct" }` or `{ "mode": "scripted", "settings": { "package": { "id": "au-eftex", "version": "1.1.0" } } }`.
- `HttpMatchConditionTemplate` uses exact current external-tag wire: `{ "Field": { "field": "TerminalIp" | "CertificateFingerprint" | "PathOrRequestType" | { "JsonPath": "$.field" }, "operator": { "Equals": "value" } | { "Contains": "value" } | { "Regex": "pattern" } } }` or `{ "NthHit": 1 }`.
- `HttpRuleActionTemplate` uses exact current external-tag wire: `"Pause"`, `{ "SetJsonField": { "path": "$.field", "value": 1 } }`, `{ "ReplaceBodyText": "text" }`, `{ "SetHeader": { "name": "X-Test", "value": "yes" } }`, `{ "Delay": { "milliseconds": 1 } }`, `{ "Jitter": { "minimum_milliseconds": 1, "maximum_milliseconds": 2, "scope": "BeforeMessage" | "PerChunk" } }`, `{ "Throttle": { "bytes_per_second": 1, "chunk_bytes": 1, "direction": "Upstream" | "Downstream" } }`, `{ "Intermittent": { "available_milliseconds": 1, "blocked_milliseconds": 1, "direction": "Upstream" | "Downstream" } }`, `{ "CustomHttpStatus": { "status": 503 } }`, or `{ "Terminal": <TerminalActionTemplate> }`.
- `TerminalActionTemplate` uses exact current external-tag wire: `"RejectTlsHandshake"`, `"DisconnectBeforeUpstream"`, `{ "UpstreamConnectTimeout": { "milliseconds": 1 } }`, `{ "UpstreamWriteTimeout": { "milliseconds": 1 } }`, `{ "UpstreamReadTimeout": { "milliseconds": 1 } }`, `{ "DropUpstreamResponse": { "mode": "ReadCompleteResponse" | "CloseAfterRequestWrite" } }`, `{ "MockResponse": { "status": 200, "headers": [["X-Test", "yes"]], "body_bytes": [79, 75] } }`, `{ "InvalidJson": { "body_bytes": [123] } }`, `{ "IncorrectContentLength": { "delta": 1 } }`, `{ "TruncateResponse": { "bytes": 1 } }`, `{ "DisconnectDuringUpstreamWrite": { "after_bytes": 1 } }`, or `{ "DisconnectDuringDownstreamWrite": { "after_bytes": 1 } }`.
- `ProtocolRuleStage`: exact strings `app_to_proxy`, `proxy_to_upstream`, `upstream_to_proxy`, `proxy_to_app`.
- `DocumentValueTemplate`: `{ "type": "string", "value": "abc" }`, `{ "type": "int", "value": 7 }`, `{ "type": "bool", "value": true }`, or `{ "type": "blob", "value": [65, 66] }`.
- `DocumentConditionTemplate`: `{ "operator": "equals", "field": "amount", "value": { "type": "int", "value": 1000 } }`.
- `DocumentActionTemplate`: `{ "type": "record_match" }`, `{ "type": "set_field", "field": "approval_code", "value": { "type": "string", "value": "OK1234" } }`, `{ "type": "clear_field", "field": "approval_code" }`, `{ "type": "clear_document" }`.
- `WeakNetworkProfileTemplate`: exact full object shown in the Android section. Required root fields are `seed`, `fixed_delay_millis`, `uniform_jitter_millis`, `upload_bytes_per_second`, `download_bytes_per_second`, `random_loss_basis_points`, `burst_loss`, `duplicate_basis_points`, `reorder_basis_points`, `maximum_reorder_hold_millis`, `blackout_windows`, `dns_blackhole`, `nth_tcp_flag_drops`, `path_mtu`, and `corruption`. Required `burst_loss` object fields are `enter_bad_state_basis_points`, `leave_bad_state_basis_points`, `good_state_loss_basis_points`, and `bad_state_loss_basis_points`; `burst_loss` itself may also be explicit `null`. Required `blackout_windows[]` fields are `start_after_millis` and `duration_millis`. Required `nth_tcp_flag_drops[]` fields are `direction`, `flag`, and `nth`. Required `path_mtu` fields are `mtu`, `mss_clamp`, and `mode`; `mtu` and `mss_clamp` may be explicit `null`. Required `corruption` fields are `probability_basis_points` and `bits_per_packet`. All listed fields must be present; omitted required fields, scalar shorthand, alternate enum tags, and unknown fields are rejected. `PacketDirection`: `upload | download`; `TcpFlag`: `syn | syn_ack | ack | fin | rst`; `PmtuMode`: `pass | fragment_or_packet_too_big | signal_too_big | blackhole`.
- `CertificateMaterialInput.role`: `downstream_server_identity | downstream_client_trust | upstream_client_identity | upstream_server_trust`; `encoding`: `pem | base64_der | pkcs12_base64`. `mitm_root_ca` is unsupported in v1 and must fail with `UNSUPPORTED_MATERIAL_ROLE`.
- `SecretMaterialInput.role`: only `proxy_basic_auth`.

## MCP Tools

`mcp_environment_capabilities` input: `{}` only. Output fields:

- `protocol_version: "environment_configuration_candidate.v1"`.
- `endpoint: string`.
- `plaintext_http: true`.
- `authentication: "none"`.
- `source_ip_filter: "none"`.
- `host_header_policy: "accept_any_syntactically_valid_http_host"`.
- `origin_policy: "ignored"`.
- `authorization_policy: "ignored_and_not_required"`.
- `ipv4: { available: bool, bind_address: "0.0.0.0", port: 17653, warning_codes: Vec<WarningCode> }`.
- `ipv6: { available: bool, bind_address: "[::]", port: 17653, warning_codes: Vec<WarningCode> }`.
- `warnings: Vec<WarningCode>`.
- `read_budgets: { input_bytes: 262144, output_bytes: 8388608, deadline_ms: 8000 }`.
- `write_budgets: { create_input_bytes: 1048576, create_output_bytes: 1048576, create_deadline_ms: 30000, status_cancel_apply_input_bytes: 16384, status_cancel_apply_output_bytes: 1048576, status_cancel_apply_deadline_ms: 8000 }`.
- `candidate_limits: { active_candidates: 4, active_per_target: 1, active_apply_global: 1, active_apply_per_target: 1 }`.
- `terminal_retention: { max_terminal_candidates: 32, max_terminal_public_bytes: 4194304, eviction: "oldest_first", evicted_status_code: "CANDIDATE_NOT_FOUND" }`.
- `schema_versions: ["environment_configuration_candidate.v1"]`.
- `validation_layers: ["schema", "domain", "material", "package_projection", "dns_tcp_port", "tls_mtls", "preview_baseline"]`.

`environment_candidate_create` input: `{ "candidate": EnvironmentConfigurationCandidateV1 }`. Output fields:

- `candidate_id: string`.
- `confirmation_token: null | string`; non-null only when `status="preview_ready"`.
- `status: "preview_ready" | "validation_failed" | "cancelled" | "cancelled_by_shutdown"`.
- `target_key: string`.
- `baseline_public: BaselinePublicSummary`.
- `validation_layers: Vec<ValidationLayerResult>`.
- `preview: null | EnvironmentPreview`.
- `expires_on: "app_exit_or_invalidation"`.
- `errors: Vec<EnvironmentDiagnostic>`.

`environment_candidate_status` input: `{ "candidate_id": string }`. Output fields:

- `candidate_id: string`.
- `status: "validating" | "preview_ready" | "validation_failed" | "stale" | "cancelled" | "cancelled_by_shutdown" | "apply_queued" | "apply_in_progress" | "committed" | "rolled_back" | "failed_before_commit" | "not_found"`.
- `target_key: null | string`.
- `baseline_public: null | BaselinePublicSummary`.
- `validation_layers: Vec<ValidationLayerResult>`.
- `preview: null | EnvironmentPreview`.
- `terminal_result: null | EnvironmentTerminalResult`.
- `errors: Vec<EnvironmentDiagnostic>`.

`environment_candidate_cancel` input: `{ "candidate_id": string }`. Output fields:

- `candidate_id: string`.
- `status: "cancelled" | "apply_in_progress_not_cancellable" | "not_found_or_terminal"`.
- `terminal: bool`.
- `errors: Vec<EnvironmentDiagnostic>`.

`environment_candidate_apply` input: `{ "candidate_id": string, "confirmation_token": string }`. Output fields:

- `candidate_id: string`.
- `apply_task_id: string`.
- `status: "apply_queued"`.
- `errors: []`.

`ValidationLayerResult`: `{ "layer": "schema" | "domain" | "material" | "package_projection" | "dns_tcp_port" | "tls_mtls" | "preview_baseline", "status": "passed" | "failed" | "cancelled" | "not_applicable" | "skipped_dependency", "code": null | ErrorCode, "reason": null | string, "duration_ms": u64 }`.

`EnvironmentDiagnostic`: `{ "code": ErrorCode, "field": null | string, "message": string, "severity": "error" | "warning" | "info" }`.

`EnvironmentTerminalResult` is an explicit tagged union. It is never an untagged object and never infers persisted identifiers from a failure status:

- Committed: `{ "result": "committed", "workspace_id": string, "revision": u64, "selected_workspace_id": null | string, "apply_task_id": null | string, "status_code": null, "diagnostics": Vec<EnvironmentDiagnostic> }`.
- Validation failed: `{ "result": "validation_failed", "status_code": ErrorCode, "diagnostics": Vec<EnvironmentDiagnostic> }`.
- Stale: `{ "result": "stale", "status_code": ErrorCode, "diagnostics": Vec<EnvironmentDiagnostic> }`.
- Cancelled: `{ "result": "cancelled", "status_code": ErrorCode, "diagnostics": Vec<EnvironmentDiagnostic> }`.
- Cancelled by shutdown: `{ "result": "cancelled_by_shutdown", "status_code": ErrorCode, "diagnostics": Vec<EnvironmentDiagnostic> }`.
- Failed before commit: `{ "result": "failed_before_commit", "status_code": ErrorCode, "diagnostics": Vec<EnvironmentDiagnostic> }`.
- Rolled back: `{ "result": "rolled_back", "status_code": ErrorCode, "diagnostics": Vec<EnvironmentDiagnostic> }`.

All non-committed variants carry exactly one non-null registered `ErrorCode` and omit persisted `workspace_id`, persisted `revision`, and `selected_workspace_id`. `rolled_back` means a SQLite transaction started and rolled back without creating a new persisted result; it still carries no new persisted identifiers. If no commit happened before transaction start, use `failed_before_commit` rather than `rolled_back`. Existing-target context, when needed for diagnostics or UI, is reported through `target_key`, `baseline_public`, or diagnostics fields; it is not reported as a claimed committed Workspace revision.

All public literal fields emitted by these five environment MCP tools must resolve through the Stable Public Literal Registry below. Environment MCP output must not emit tool-local string literals, library error strings, transport error text, or enum aliases outside that registry.

Annotations:

- capabilities/status: `read_only=true`, `destructive=false`, `idempotent=true`.
- create: `read_only=false`, `destructive=false`, `idempotent=false`.
- cancel: `read_only=false`, `destructive=true`, `idempotent=true`.
- apply: `read_only=false`, `destructive=true`, `idempotent=false`.

## State, Tokens, Capacity, and Retention

| State | Entry | Exit | Private memory |
| --- | --- | --- | --- |
| `validating` | create accepted | `preview_ready`, `validation_failed`, `cancelled`, `cancelled_by_shutdown` | held |
| `preview_ready` | all layers pass | `apply_queued`, `cancelled`, `stale`, `cancelled_by_shutdown` | held |
| `validation_failed` | validation fails | terminal | zeroed |
| `stale` | subscribed event before apply | terminal | zeroed |
| `cancelled` | cancel or pre-return disconnect | terminal | zeroed |
| `cancelled_by_shutdown` | shutdown begins before apply starts | terminal | zeroed |
| `apply_queued` | apply request atomically consumes token and creates owned task | `apply_in_progress`, `cancelled`, `cancelled_by_shutdown` | held by Application candidate registry |
| `apply_in_progress` | owned worker dequeues and takes cleanup ownership | `committed`, `rolled_back`, `failed_before_commit` | transferred to owned worker/zeroing in progress |
| `committed` | DB commit succeeds | terminal | zeroed |
| `rolled_back` | SQLite transaction starts then rolls back | terminal | zeroed |
| `failed_before_commit` | shutdown/lease/protector/runtime failure before SQLite txn | terminal | zeroed |

Rules:

- Max active candidates: 4.
- Max active candidate per target key: 1.
- Max active apply task globally: 1.
- Max active apply task per target key: 1.
- Terminal public result/tombstone retention is bounded to the newest 32 terminal candidates and at most 4 MiB of serialized public terminal/tombstone bytes across retained candidates, whichever limit is reached first.
- Confirmation token exists only in `preview_ready`.
- Apply atomically consumes token and creates an owned task in `apply_queued`; MCP request deadline covers only parse, token consumption, and enqueue/ack.
- The owned worker is the only component that can linearize `apply_queued` to `apply_in_progress`, and only after dequeueing and taking cleanup ownership.
- Normal cancel races atomically with the worker transition. If cancel wins while the candidate is `validating`, `preview_ready`, or `apply_queued`, the candidate becomes terminal `cancelled`, all private material is zeroed, capacity is released, and any queued worker observation sees terminal state and never prepares material, acquires commit authority, starts SQLite, or commits. If the worker first transitions to `apply_in_progress`, normal cancel does not interrupt preparation/commit and returns cancel-result `apply_in_progress_not_cancellable`; the owned worker drains to `committed`, `rolled_back`, or `failed_before_commit`. If the candidate is already terminal or absent, cancel returns `not_found_or_terminal`.
- Shutdown may transition `apply_queued` to `cancelled_by_shutdown`; Application candidate registry zeroes private material and releases candidate/apply capacity.
- Once `apply_in_progress`, shutdown waits for terminal `committed`, `rolled_back`, or `failed_before_commit`; material cleanup is owned by the worker.
- `cancelled_by_shutdown` is a candidate/terminal status visible through create/status only; it is not a normal cancel-result literal.
- Token reuse returns `TOKEN_CONSUMED`.
- Only terminal public results/tombstones are retained after terminal cleanup; private material, prepared handles, plaintext, protected bytes, confirmation tokens, staged aliases with secret values, and business payloads are never retained. Eviction is deterministic oldest-first by terminal sequence, then candidate ID as a stable tie-breaker if needed. Active non-terminal candidates are never evicted by terminal retention. Status lookup for an evicted terminal returns `status="not_found"`, no `terminal_result`, and exact error code `CANDIDATE_NOT_FOUND`. Capability output advertises `terminal_retention: { max_terminal_candidates: 32, max_terminal_public_bytes: 4194304, eviction: "oldest_first", evicted_status_code: "CANDIDATE_NOT_FOUND" }`.
- Terminal public status remains process-local until App exit and within the bounded retention limits above; it is not persisted/restored.

Stable warning/error/status/cancel-result literals are defined only by the Stable Public Literal Registry. There are no inherited or implicit code sets.

Terminal result status contract: `terminal_result` is an explicit tagged `EnvironmentTerminalResult` union. `result="committed"` carries persisted `workspace_id`, committed `revision`, optional `selected_workspace_id`, optional `apply_task_id`, `status_code:null`, and diagnostics. Every pre-commit terminal result (`validation_failed`, `stale`, `cancelled`, `cancelled_by_shutdown`, `failed_before_commit`) carries exactly one non-null registered `ErrorCode` and no persisted Workspace identifiers. `rolled_back` carries exactly one non-null registered `ErrorCode` and no persisted Workspace identifiers because SQLite rollback leaves no new persisted result. `status="not_found"` returns errors and no terminal result. The status code must not contain raw transport/library text, aliases, multiple codes, or unregistered literals.

## Baseline, Lease, ABA, and TOCTOU

`EnvironmentCandidateBaseline` includes target mode/key, existing target Workspace revision and structural hash or new target normalized name key, selected Workspace ID, affected-resource diff, listener runtime state/epoch/active connection count/mutation gate generation for affected Listener IDs, Android runtime owner/state generation for affected Android profile/device keys, external package registry service epoch/exact package refs/enabled flags/description fingerprints/online generation/lease generation, certificate inventory generation/fingerprint, protected secret inventory generation/fingerprint, MCP candidate schema version, and validation engine version.

Affected-resource diff algorithm:

1. Build `old_targets` from the current persisted Workspace for `target.mode=existing`; for `target.mode=new`, `old_targets` is empty and the whole candidate Workspace is treated as added.
2. Build `candidate_targets` after DTO parse, alias resolution, server-generated ID assignment, and typed package-ref parse, but before material staging/protector or DB transaction.
3. Target keys are stable per resource kind:
   - Listener: `listener_id` when updating an existing listener; otherwise `listener_alias`.
   - Android profile: `profile_id`; Android route key: `(profile_id, destination, sorted_ports, listener_target_key)`.
   - Protocol package ref: `(package.id, semver(version), version_text)`.
   - HTTP rule: `(bound_listener_target_key, "existing_rule_id", existing_rule_id)` for candidate rules that retain a persisted HTTP rule identity, otherwise `(bound_listener_target_key, "candidate_index", candidate_http_rule_index)`. `candidate_http_rule_index` is the zero-based index in the validated `http_rules` array after parse and before persistence. It is used only for diff classification of newly submitted rules and is not persisted.
   - HTTP rule material refs: `(http_rule_key, action_or_condition_path, alias)`.
   - Protocol Document rule: `(bound_listener_target_key, package.id, semver(version), version_text, schema_version, stage, "existing_rule_id", existing_rule_id)` for candidate rules that retain a persisted Protocol Document rule identity, otherwise `(bound_listener_target_key, package.id, semver(version), version_text, schema_version, stage, "candidate_index", candidate_protocol_rule_index)`. `candidate_protocol_rule_index` is the zero-based index in the validated `protocol_rules` array after parse and before persistence. It is used only for diff classification of newly submitted rules and is not persisted.
   - Protocol Document rule package/material refs: `(protocol_document_rule_key, ref_path, package.id, semver(version), version_text, schema_version, stage)`.
   - Certificate/secret material refs: `(material_kind, alias, role, fingerprint)`.
4. Canonical content comparison uses serde-derived canonical JSON bytes for the validated, server-normalized DTO projection with generated IDs, revisions, runtime fields, timestamps, hit counters, and final material refs removed. Object keys sort lexicographically by UTF-8 bytes; arrays preserve contract order where order is semantic and otherwise sort by the resource key above. HTTP rule content includes `existing_rule_id` only as the identity selector for retained rules, plus `enabled`, `priority`, matchers, conditions, action sequence, terminal action payloads, bound listener key, HTTP body protocol package ref, material alias edges, and every persistable mutable rule field. Protocol Document rule content includes `existing_rule_id` only as the identity selector for retained rules, plus `enabled`, `priority`, package ref, `schema_version`, `stage`, condition/action sequence, Document `{type,value}` payloads, material/package/reference edges, bound listener key, and every persistable mutable rule field. Listener content includes topology/TLS/auth/body/fault/capture/runtime-config persistable fields and all rule binding identities. Material content includes alias, role, format, public label, fingerprint, and consumer edge list, never private bytes.
5. For each kind, sort target keys deterministically using UUID canonical string order for UUIDs, UTF-8 byte order for strings, numeric order for integers, and SemVer order plus original `version_text` tie-breaker for versions.
6. Classify each key as `added`, `removed`, `changed`, or `unchanged`. `added` exists only in candidate targets. `removed` exists only in old targets. `changed` exists in both and has different canonical content bytes or any changed reference edge. `unchanged` exists in both and has byte-identical canonical content and reference edges. Reference-only changes count as `changed` even when the referenced target's own payload is unchanged.
7. Lease scope includes only `added`, `removed`, and `changed` resources plus unchanged resources whose runtime state can invalidate the changed reference graph. Any added, removed, or changed HTTP rule lifts its bound Listener into the affected runtime set. Any added, removed, or changed Protocol Document rule lifts its bound Listener into the affected runtime set. Reference-only changes and material-only changes lift every consuming Listener reached through the changed alias/package/reference edge. Whole-Workspace lease is required only when `target.mode=new`, when selected Workspace state changes, when Workspace-level capacity/selection constraints are involved, or when diff cannot map a candidate field to a stable target key; otherwise use changed-resource-only gates.
8. Runtime-active apply rejection checks `removed` and `changed` active Listener IDs, `starting` Listener IDs, `stopping` Listener IDs, Listener IDs with active connections, and affected Android profile/device targets. Affected Android profile/device targets apply only when there is no runtime owner. Android runtime states `active`, `uncertain`, `waiting_reconnect`, `cleanup_required`, `stop_failed`, and `faulted` all block apply before protected preparation, auto stop/recovery, hot replacement, SQLite transaction, or Workspace persistence with stable `ANDROID_RUNTIME_OWNER_ACTIVE`; `idle` with no runtime owner is permitted to reach later lease/protector/commit checks. If a lifted Listener is `active`, `starting`, `stopping`, or has `active_connection_count > 0`, apply rejects before protected preparation, hot rule replacement, SQLite transaction, or Workspace persistence with stable `RUNTIME_ACTIVE`. This workflow does not hot-replace HTTP rules or Protocol Document rules on running listeners and does not auto stop, auto recover, or clean up Android runtime owners. `unchanged` active Listener IDs do not reject apply solely because they are active; they remain in the baseline only if a changed resource references them.

Candidate subscribes to Workspace, selected Workspace, Listener runtime, Android runtime, external package registry, certificate inventory, and secret inventory events. Relevant events before apply mark candidate `stale` immediately and zero private material. Package projection generation drift, package disappearance, enabled-flag changes, and online-flag changes observed by these subscriptions always mark terminal `stale` with `CANDIDATE_STALE`, taking precedence over all generic guarded-epoch mismatch handling. If visible values return to the old value but generation/lease changed, apply still fails.

Apply acquires lease after token consumption and before protected material preparation. Guards are held through preparation, `EnvironmentCommitPort::commit`, terminal state write, and memory cleanup. DB transaction rechecks all persistable baseline fields. Non-DB generation mismatch before transaction maps to `APPLY_LEASE_MISMATCH` and `failed_before_commit` only for non-package guards such as Listener runtime, Android runtime, certificate inventory, protected secret inventory, selected Workspace, or Application mutation gates. Package projection generation drift, package disappearance, enabled-flag changes, and online-flag changes never map to `APPLY_LEASE_MISMATCH`; they always use terminal `stale` with `CANDIDATE_STALE`. These outcomes are mutually exclusive, and package-specific stale mapping wins when both a package projection change and another guard mismatch are discovered in the same pre-commit check. Queued external offline publication emits after guard release and does not roll back already committed Workspace.

## Deadlines, Cancellation, Shutdown, and Bind

Budgets:

- HTTP body max: 2 MiB.
- Existing read tools unchanged: 256 KiB input, 8 MiB output, 8 second deadline.
- Create: 1 MiB input/output, 30 second validation deadline.
- Status/cancel/apply: 16 KiB input, 1 MiB output, 8 second ack deadline.
- Create limits: listeners 8, HTTP rules 128, protocol rules 128, certificates 16, secrets 16, Android profiles 8, upstream addresses checked 16.
- Layer budgets: schema 1s, domain 1s, material parse/fingerprint 6s, package projection 4s, DNS/TCP/port 8s, TLS/mTLS 10s, preview/baseline 2s.
- Schema/domain/material/package run sequentially; DNS/TCP/TLS probes run with concurrency 4.

Layer statuses are `passed`, `failed`, `cancelled`, `not_applicable`, and `skipped_dependency`. `skipped_dependency` is allowed only after dependency failure/cancellation/deadline and never authorizes apply. There is no plain `skipped` status in v1.

Create's 30 second total deadline dominates all layer budgets. Dependency-order reporting is deterministic: completed layers keep `passed` or `failed`; the currently executing unfinished layer at deadline becomes `cancelled` with reason `create_deadline_exceeded`; downstream layers not started because of that cancellation become `skipped_dependency` with reason `create_deadline_exceeded`; layers proven irrelevant before the deadline remain `not_applicable`. The response status is `validation_failed` with code `MCP_CREATE_DEADLINE_EXCEEDED` unless request cancellation/shutdown is the direct cause, in which case it is `cancelled`/`cancelled_by_shutdown`.

Create may validate/preview while affected Listener runtime is active; only apply rejects runtime-active mutation. Request cancellation propagates through create before candidate return. Apply request deadline covers parse/token consumption/task enqueue only; caller disconnect after ack does not cancel the owned task.

Shutdown behavior:

- Shutdown begin rejects new create/apply with `SHUTDOWN_IN_PROGRESS`.
- Queued apply not yet started becomes terminal `cancelled_by_shutdown` with `CANDIDATE_CANCELLED_BY_SHUTDOWN`.
- In-progress apply that entered owned preparation/commit is awaited until terminal; mutation gate is not dropped mid-operation.
- Do not add a new shutdown timeout unless implementation finds and reuses an existing product-wide timeout.
- Hard kill may lose process-local status; SQLite atomicity remains required.

IPv4/IPv6 all-interface bind:

| Bind result | Capability output | Service state |
| --- | --- | --- |
| IPv4 bind fails | `ipv4.available=false`, `IPV4_BIND_FAILED` | startup fails; no IPv6-only partial service |
| IPv4 and IPv6 succeed | both available | service runs |
| IPv4 succeeds, IPv6 unsupported | warning `ipv6_unsupported`, IPv6 unavailable | service runs |
| IPv4 succeeds, dual-stack covers IPv6 | warning `ipv6_dual_stack_covered`, IPv6 available | service runs |
| IPv4 succeeds, other IPv6 bind fails | warning `IPV6_DEGRADED`, IPv6 unavailable | service runs degraded and must not claim IPv6 reachability |

Transport openness and protocol correctness:

- The MCP HTTP listener binds all interfaces according to the IPv4/IPv6 table. It accepts non-loopback peers when the platform route/firewall allows the TCP connection.
- No auth header, bearer token, API key, cookie, client certificate, Host allowlist, Origin allowlist, Origin requirement, source-IP allowlist, loopback-only peer check, or private-network CIDR gate is introduced.
- `Host` is checked only for syntactically valid HTTP parsing by the HTTP stack. Arbitrary syntactically valid Host values, including DNS names, IPv4 literals, IPv6 literals with brackets, and host:port forms, reach MCP protocol handling.
- `Origin` may be absent or any syntactically valid header value and is ignored by MCP authorization.
- Malformed HTTP, unsupported method/path, body over limit, invalid JSON-RPC/MCP envelope, missing required MCP protocol metadata, schema-invalid tool arguments, and invalid protocol messages still reject for protocol correctness only.

## Stable Public Literal Registry

This registry is the complete closed public literal set for MCP environment configuration v1. Every warning, error, status, terminal-result status code, cancel-result status, validation layer, severity, policy string, schema string, and tool name emitted by `mcp_environment_capabilities`, `environment_candidate_create`, `environment_candidate_status`, `environment_candidate_cancel`, and `environment_candidate_apply` must be defined here. Implementation must not reference inherited revision code names, unlisted aliases, synthetic success codes, or transport/library raw error strings in public MCP output. Tests must fail if any environment MCP DTO, catalog projection, JSON schema, fixture, expected preview, or status/cancel/apply response emits a literal not present in this registry, or if the registry contains a public literal that is no longer emitted or accepted by the active v1 contract.

Tool names:

- `mcp_environment_capabilities`
- `environment_candidate_create`
- `environment_candidate_status`
- `environment_candidate_cancel`
- `environment_candidate_apply`

Schema/protocol literals:

- `environment_configuration_candidate.v1`
- `app_exit_or_invalidation`

Capability/policy literals:

- `none`
- `accept_any_syntactically_valid_http_host`
- `ignored`
- `ignored_and_not_required`
- `oldest_first`

Warning codes:

- `ipv6_unsupported`
- `ipv6_dual_stack_covered`
- `IPV6_DEGRADED`

Validation/schema/material/package codes:

- `SCHEMA_INVALID`
- `UNKNOWN_FIELD`
- `FORBIDDEN_FIELD`
- `DTO_LIMIT_EXCEEDED`
- `WORKSPACE_NAME_EMPTY`
- `WORKSPACE_NAME_COLLISION`
- `LISTENER_ALIAS_DUPLICATE`
- `LISTENER_ALIAS_MISSING`
- `LISTENER_ALIAS_TYPE_MISMATCH`
- `LISTENER_DOMAIN_INVALID`
- `EXISTING_RULE_ID_FORBIDDEN`
- `EXISTING_RULE_ID_UNKNOWN`
- `EXISTING_RULE_ID_DUPLICATE`
- `EXISTING_RULE_ID_WORKSPACE_MISMATCH`
- `EXISTING_RULE_ID_KIND_MISMATCH`
- `EXISTING_RULE_ID_BINDING_MISMATCH`
- `EXISTING_RULE_ID_PACKAGE_MISMATCH`
- `EXISTING_RULE_ID_SCHEMA_VERSION_MISMATCH`
- `EXISTING_RULE_ID_STAGE_MISMATCH`
- `HTTP_RULE_INVALID`
- `PROTOCOL_DOCUMENT_RULE_INVALID`
- `DOCUMENT_VALUE_WIRE_INVALID`
- `WEAK_NETWORK_WIRE_INVALID`
- `WEAK_NETWORK_VALUE_INVALID`
- `MATERIAL_ALIAS_DUPLICATE`
- `MATERIAL_ALIAS_MISSING`
- `MATERIAL_ALIAS_TYPE_MISMATCH`
- `MATERIAL_ALIAS_UNUSED`
- `MATERIAL_ALIAS_MULTIPLE_CONSUMERS_UNSUPPORTED`
- `UNSUPPORTED_SECRET_ROLE`
- `UNSUPPORTED_MATERIAL_ROLE`
- `CERTIFICATE_PARSE_FAILED`
- `CERTIFICATE_ROLE_MISMATCH`
- `SECRET_VALUE_INVALID`
- `INVALID_PROTOCOL_PACKAGE_VERSION`
- `PROTOCOL_PACKAGE_NOT_INSTALLED`
- `PROTOCOL_PACKAGE_DISABLED`
- `EXTERNAL_PACKAGE_OFFLINE`
- `PROTOCOL_PACKAGE_INCOMPATIBLE`
- `MCP_CREATE_DEADLINE_EXCEEDED`
- `VALIDATION_LAYER_FAILED`

Candidate/apply lifecycle codes:

- `CANDIDATE_NOT_FOUND`
- `CANDIDATE_STALE`
- `CANDIDATE_CANCELLED`
- `CANDIDATE_CANCELLED_BY_SHUTDOWN`
- `CANDIDATE_CAPACITY_EXCEEDED`
- `TARGET_CANDIDATE_ALREADY_ACTIVE`
- `APPLY_ALREADY_ACTIVE`
- `CONFIRMATION_TOKEN_MISSING`
- `CONFIRMATION_TOKEN_INVALID`
- `TOKEN_CONSUMED`
- `SHUTDOWN_IN_PROGRESS`
- `RUNTIME_ACTIVE`
- `ANDROID_RUNTIME_OWNER_ACTIVE`
- `AFFECTED_RESOURCE_CHANGED`
- `AFFECTED_RESOURCE_REMOVED`
- `APPLY_LEASE_UNAVAILABLE`
- `APPLY_LEASE_MISMATCH`
- `PROTECTED_MATERIAL_PREPARE_FAILED`
- `COMMIT_BASELINE_MISMATCH`
- `COMMIT_ROLLED_BACK`
- `COMMIT_FAILED`
- `HARD_KILL_STATUS_UNAVAILABLE`

Transport/capability codes:

- `IPV4_BIND_FAILED`
- `HTTP_METHOD_NOT_ALLOWED`
- `HTTP_PATH_NOT_FOUND`
- `HTTP_BODY_TOO_LARGE`
- `HTTP_MALFORMED`
- `MCP_PROTOCOL_INVALID`
- `MCP_TOOL_ARGUMENTS_INVALID`

Stable validation layers:

- `schema`
- `domain`
- `material`
- `package_projection`
- `dns_tcp_port`
- `tls_mtls`
- `preview_baseline`

Stable layer statuses:

- `passed`
- `failed`
- `cancelled`
- `not_applicable`
- `skipped_dependency`

Stable candidate/apply statuses:

- `validating`
- `preview_ready`
- `validation_failed`
- `stale`
- `cancelled`
- `cancelled_by_shutdown`
- `apply_queued`
- `apply_in_progress`
- `committed`
- `rolled_back`
- `failed_before_commit`
- `not_found`

Stable terminal-result variants:

- `committed`
- `validation_failed`
- `stale`
- `cancelled`
- `cancelled_by_shutdown`
- `failed_before_commit`
- `rolled_back`

Stable cancel-result statuses:

- `cancelled`
- `apply_in_progress_not_cancellable`
- `not_found_or_terminal`

Stable diagnostic severities:

- `error`
- `warning`
- `info`

## Implementation Order

1. DTO/schema/full-shape JSON fixture, schema snapshot, field-by-field expected preview, public literal registry/drift tests, Android weak-network full-shape/negative fixtures, and read-tool regressions — owner MCP/Application; evidence `MCP-CONFIG-CONTRACT-001`; independent commit; rollback DTO/schema/tests.
2. Candidate lifecycle/capacity/token/target key/shutdown pre-gates — owner Application; evidence `MCP-CONFIG-CANDIDATE-001`; rollback candidate modules/tests.
3. Lease contracts and epoch model only — owner Application contracts; evidence `MCP-CONFIG-LEASE-CONTRACT-001`; independent commit; rollback trait/types/tests.
4. Listener and Android lease adapters — owner Application/runtime adapters; evidence `MCP-CONFIG-LEASE-ADAPTER-001`; independent commit; rollback adapters/tests.
5. External package publication gating and typed package-ref ordering — owner Application/package registry; evidence `MCP-CONFIG-LEASE-PACKAGE-001`; independent commit; rollback package gates/tests.
6. Integrated lease acquisition order, reverse release, cancellation, and deadlock gates — owner Application; evidence `MCP-CONFIG-LEASE-ORDER-001`; independent commit; rollback integration gate/tests.
7. Protected material preparation port and zeroizing handles — owner Infrastructure; evidence inside atomic; rollback preparation port/tests.
8. `EnvironmentCommitPort` SQLite transaction — owner Infrastructure; evidence `MCP-CONFIG-ATOMIC-001`; rollback commit port/tests.
9. Validation orchestration without business/package RPC — owner Application; evidence `MCP-CONFIG-VALIDATION-001`; rollback validator/tests.
10. Apply queued/in-progress task, disconnect, and shutdown terminal orchestration — owner Application/MCP boundary; evidence `MCP-CONFIG-DISCONNECT-001`; rollback apply use case.
11. MCP adapter/schema/annotations/all-interface bind — owner MCP; evidence `MCP-CONFIG-CONTRACT-001` and `MCP-CONFIG-IPV6-001`; rollback adapter/bind changes.
12. Packaged App E2E, ADR, docs, evidence indexes — owner docs/test; evidence `MCP-CONFIG-CHAIN-001`, `MCP-CONFIG-APP-001`, `MCP-CONFIG-ARCH-001`; rollback docs/evidence references.

## Evidence and Acceptance

Execution amendment (confirmed 2026-08-27): the user explicitly waived separate execution-evidence directories for G034/G035/G036/G038. The required task evidence is therefore `MCP-CONFIG-CONTRACT-001` plus the final integrated packaged-App case `MCP-CONFIG-APP-001`; the waived slices remain covered by repository regression tests, the task test ledger, and mandatory independent review. This amendment replaces the earlier thirteen-directory plan without weakening any behavioral acceptance criterion.

Each retained evidence directory records task-related file names and readable source extracts, pre/post-test runtime stability, `resources/`, `inputs/`, `outputs/`, `steps/`, and `replay/` contents where applicable, plus purpose, environment, preconditions, commands, expected output, actual output, comparison, logs/stdout/stderr, N/A explanations, and result. Per the user's 2026-08-27 instruction, evidence does not record Git state, Commit records, HEAD, diffs, or hashes. `steps/` must contain the actual preparation, success-path, and cleanup commands/scripts or operator steps used for the run. `replay/` must contain from-zero replay instructions plus archived resources and name every external dependency, environment variable, packaged App artifact, and cleanup action needed to reproduce the result. If a directory class is not applicable, do not create an empty placeholder directory; record `N/A` and the reason in `README.md` or `metadata.json`. Secret redline scans generated outputs/logs/diagnostics/serialized results/MCP responses only; original private fixtures are archived exactly as used. The only accepted real App E2E is packaged App.

Task document future update list must include the amended required evidence IDs above, link their directories, and record whether each subtask's targeted adversarial review was executed.

Acceptance criteria:

- AC1: Single full-shape JSON DTO fixture covers every field above, including exact Protocol Document `{type,value}` shapes and the exact `WeakNetworkProfileTemplate` full object; schema snapshot and field-by-field expected preview match exactly.
- AC2: Client-submitted final refs, new Workspace ID/revision/runtime state, unknown nested fields, omitted required `WeakNetworkProfileTemplate` fields, weak-network scalar shorthand, alternate enum tags, omitted-vs-null substitutions, scalar/raw Protocol Document values, and `existing_rule_id` outside existing-target HTTP/Protocol Document rule selectors are rejected.
- AC3: Alias graph resolves correctly; duplicate/missing/type-mismatch/unused/unsupported-multiple-consumer aliases, unsupported secret roles, and unsupported `mitm_root_ca` material fail with stable codes and no persistence.
- AC4: Old read tools retain behavior/budgets; write annotations match exactly; every public warning/error/status/cancel-result/capability-policy literal emitted by environment MCP tools is present in the Stable Public Literal Registry and static drift tests reject unregistered literals, including terminal-retention eviction literal `oldest_first`; terminal result is an explicit tagged union where committed alone carries persisted `workspace_id`/`revision` plus `status_code:null`, while validation_failed/stale/cancelled/cancelled_by_shutdown/failed_before_commit/rolled_back carry exactly one registered non-null code and no fabricated persisted identifiers.
- AC5: Create validates six layers, emits no private output, and can preview while runtime active.
- AC6: Create disconnect/shutdown before return zeroes memory and leaves no candidate.
- AC7: Apply returns `apply_task_id` with `apply_queued`; worker later transitions to `apply_in_progress`; caller disconnect after ack does not cancel owned task; normal cancel deterministically races queued worker transition so cancel-wins prevents preparation/commit, worker-wins returns `apply_in_progress_not_cancellable`, terminal/absent returns `not_found_or_terminal`, and `cancelled_by_shutdown` is never emitted as a normal cancel result.
- AC8: Lease acquisition follows the canonical total order, affected-resource diff drives changed-resource-only versus whole-Workspace lease scope exactly as specified, existing target rule selectors retain valid HTTP/Protocol Document rule identity while rejecting duplicates and cross-workspace/cross-kind/cross-binding/cross-package/cross-stage/cross-schema/unknown IDs before gates, old target rules not referenced are removed, added/removed/changed HTTP rules and Protocol Document rules lift their bound Listener into the affected runtime set, reference-only/material-only changes lift every consuming Listener, unchanged active listeners do not reject apply solely because they are active, changed/removed/lifted active/starting/stopping/active-connection listeners reject before preparation/hot rule replacement/transaction, affected Android targets permit apply only when idle with no runtime owner and block active/uncertain/waiting_reconnect/cleanup_required/stop_failed/faulted with `ANDROID_RUNTIME_OWNER_ACTIVE`, no hot rule replacement or Android auto stop/recovery occurs in this workflow, no internal locks are held across protector/SQLite awaits, lease guard blocks Application-visible generation changes until commit/memory cleanup, queued offline publication occurs after reverse-order release, invalid package SemVer causes zero gate/protector/SQLite calls, package disabled/offline availability is checked at create and rechecked at apply with exact terminal mapping, and package validation uses zero package RPC/health probe/business bytes.
- AC9: Protector/keychain failure before transaction creates no DB rows and terminal `failed_before_commit`.
- AC10: Existing/new Workspace commit preserves selection semantics.
- AC11: Any transaction failure leaves zero residue.
- AC12: Shutdown `apply_queued`/`apply_in_progress` semantics match this PRD.
- AC13: Hard-kill test proves SQLite all-or-nothing, not status recovery.
- AC14: IPv4/IPv6 behavior follows table; non-loopback peers with arbitrary syntactically valid Host and absent/present arbitrary Origin reach MCP protocol handling without auth/header/token/source-IP gates, while malformed HTTP/MCP still rejects for protocol correctness.
- AC15: New ADR supersedes ADR-004 and docs no longer claim loopback-only/read-only MCP.
- AC16: The amended required evidence IDs exist; waived slice directories are explicitly recorded in the task document, while terminal retention still proves N/N+1 and B/B+1 eviction, active candidates never evict, cleanup removes private material, counters/oldest sequence are deterministic if exposed, and evicted status lookup returns `CANDIDATE_NOT_FOUND`.

## Stop Condition

Consensus is approved after Architect review 16 and Critic review 7. This planning artifact is ready for G033 execution handoff. Implementation completion still requires code, docs, evidence, packaged App E2E, and mandatory whole-task adversarial review.
