# MCP Environment Configuration — Deep Interview Context

- Created: 2026-08-25T12:42:50Z
- Context type: brownfield
- Prompt-safe initial-context summary: not_needed
- Working tree note: the repository already contains a large in-progress architecture task touching MCP, Application, certificate, listener, persistence, tests, and docs; no implementation edits are authorized during this interview.

## Task statement

Add an MCP capability that can use information supplied during a conversation—such as certificates, external server addresses, and other environment parameters—to validate the target environment and, only after successful validation, configure the running Intercept Proxy application for that environment.

## Desired outcome

An AI client can gather the required external-environment inputs, obtain typed validation evidence, and apply a valid Proxy environment without manually reproducing every field in the UI.

## Stated solution

Expose application-configuration operations through MCP and use a validate-before-apply flow for certificates, addresses, and related Proxy settings.

## Probable intent hypothesis

Reduce error-prone manual setup and allow conversational onboarding/troubleshooting to end in a usable, verified Proxy configuration.

## Current verified facts

- `docs/architecture/decisions/ADR-004-embedded-read-only-mcp.md:9-17` defines the embedded MCP as loopback-only, unauthenticated, and read-only; it explicitly forbids save/import/export/clear/start/stop and other mutations.
- The same ADR at lines 53-62 rejects write operations under the existing trust boundary; a write-capable MCP therefore requires a new security/authority decision rather than a catalog-only extension.
- `src-tauri/src/mcp/catalog.rs` marks the current static catalog read-only and every tool with the read-only annotation.
- `src-tauri/crates/application/src/facade/workspaces.rs` already provides Application-owned Workspace create/validate/save operations guarded by the mutation gate and running-state checks.
- `src-tauri/crates/application/src/facade/listeners.rs` already provides Listener validate/save/start/stop operations.
- `src-tauri/crates/application/src/facade/listener_certificates.rs` already exposes role-specific certificate imports: downstream server identity, downstream client trust, upstream client identity, and upstream server trust.
- Domain validation already distinguishes target address, TLS/SNI, timeouts, topology, and certificate-reference roles.
- Recent project history records SSL/proxy configuration as coordination only: no saved implementation or successful proxy-to-server handshake was proven.
- Prior TLS evidence requires validation layers to remain separate: certificate parsing/role, TCP reachability, TLS handshake, hostname/chain verification, optional mTLS, protocol framing, and business response are not one success condition.

## Constraints

- Preserve clean dependency direction: MCP must call Application use cases, never SQLite or arbitrary infrastructure directly.
- Do not expose or echo private keys, certificate passwords, or unrestricted file paths through ordinary MCP results.
- Do not report partial validation as full environment success.
- No unconfirmed fallback, retry, auto-success, or TLS downgrade behavior.
- Existing unrelated worktree edits must not be overwritten, reverted, staged, or committed.
- Deep Interview is requirements-only; no implementation occurs before crystallization and handoff.

## Unknowns / open questions

- Apply target: mutate the selected Workspace in place, create a new Workspace/environment, or prepare a UI draft only.
- Mutation authority: automatic apply after validation, explicit user confirmation/approval token, or UI-mediated confirmation.
- What “current Proxy” means when the selected Listener is running, has active connections, or contains unsaved changes.
- Atomicity and rollback contract for imported certificate material plus Workspace/Listener persistence.
- Accepted input channels: uploaded MCP content/blob, local path, pasted PEM/text, remote URL, or bounded combinations.
- Required certificate formats and roles, including encrypted PKCS#12/password handling.
- Validation success ladder and whether a business-protocol probe is required or optional.
- Whether successful apply should start/restart listeners or only persist configuration.
- Audit/redaction requirements for write attempts and supplied secrets.
- Initial scope across HTTP, Socket, TLS, mTLS, Android routing, protocol packages, rules, and global settings.

## Decision-boundary unknowns

- Which persistent object MCP may create or replace.
- Which actions require a fresh human confirmation.
- Which running-state changes MCP may perform automatically.
- Which credentials/material may be accepted and how long staged material may live.

## Likely codebase touchpoints

- `src-tauri/src/mcp/catalog.rs`
- `src-tauri/src/mcp/backend/dispatch.rs`
- `src-tauri/src/mcp/catalog/contract.rs`
- `src-tauri/crates/application/src/facade/`
- `src-tauri/crates/application/src/ports.rs`
- `src-tauri/crates/domain/src/workspace/`
- `src-tauri/crates/infrastructure/src/adapters/listener_certificates/`
- `src-tauri/crates/infrastructure/src/adapters/listener_runtime/`
- MCP contract tests, Application requirements tests, evidence and architecture docs

## Relevant repo guidance inspected

- Repository `AGENTS.md` supplied in the current prompt
- `README.md`
- `docs/architecture/decisions/ADR-004-embedded-read-only-mcp.md`
- `docs/mcp/tool-reference.md`
- `docs/mcp/certificate-concepts.md`
- Existing `.omx/context/` and `.omx/specs/` listener/runtime artifacts
- Recent project memory for 2026-08-25 SSL/proxy coordination and 2026-08-21 certificate/TLS validation

## Terminology / contract conflict

- User term “MCP 对外提供配置” conflicts with the current canonical contract “本机只读 MCP”. “对外” must be disambiguated as exposure to a local AI client versus remote-network exposure.
- User term “验证成功” is broader than the existing layered validation model and must be defined before implementation.
- User term “当前 proxy” does not identify whether the mutation target is the selected Workspace, one Listener, or the whole application configuration.

## Interview rounds

### Round 1 — Mutation authority

- User decision: A — after validation, MCP returns a complete change preview and may apply it atomically only after explicit confirmation in the conversation.
- Consequence: validation never grants mutation authority by itself; a preview/confirmation must be bound to the exact candidate revision so stale confirmation cannot authorize changed inputs.
- Still unresolved: apply target, running-listener behavior, rollback boundary, validation ladder, accepted input channels, and non-goals.
- Updated ambiguity: 0.41 (standard threshold: 0.20).

### Round 2 — Running listener boundary

- User decision: A — reject the write while any affected Listener is running or has active connections; preserve the current runtime unchanged.
- Consequence: after the affected runtime is stopped, MCP must re-read current state, re-run validation, issue a new revision-bound preview, and obtain a new explicit confirmation. A previous confirmation cannot be replayed.
- Pressure-pass result: explicit confirmation does not authorize MCP to stop/restart a Listener or terminate connections.
- Still unresolved: whether the stopped selected Workspace is replaced in place or cloned, atomic certificate/config rollback, validation success ladder, accepted input channels, and explicit non-goals.
- Updated ambiguity: 0.35 (standard threshold: 0.20).

### Round 3 — Workspace target selection

- User decision: while creating the candidate configuration, MCP must ask the user to select either a specific existing Workspace or creation of a new Workspace.
- Consequence: MCP cannot infer the target from the currently selected Workspace. The target mode and exact Workspace ID/new Workspace name become revision-bound preview inputs.
- Existing-Workspace and new-Workspace flows are both in scope, but neither may apply until validation and explicit confirmation complete.
- Still unresolved: mutation granularity inside an existing Workspace, name/conflict behavior for new Workspaces, atomic certificate/config rollback, validation ladder, input channels, and non-goals.
- Updated ambiguity: 0.29 (standard threshold: 0.20).

### Round 4 — Existing Workspace mutation granularity

- Initial answer A limited mutation to one Listener and its certificate references.
- Superseded by the user's newer requirement: rules and every related component across the complete proxy chain must also be configurable.
- Current interpretation requiring confirmation: the candidate owns one coherent environment chain, potentially including certificates, Listener transport/TLS, protocol package, document rules, fault/routing behavior, and Android routing needed for that environment.
- Still unresolved: whether application-global settings and shared package/certificate inventory are inside the chain boundary, exact validation success level, input channels and secret lifetime, atomic rollback, and explicit non-goals.
- Updated ambiguity: 0.30 (scope expanded; standard threshold: 0.20; readiness gates still open).

### Round 5 — Complete-chain outer boundary

- User decision: A — the complete configuration chain is bounded by the user-selected or newly created Workspace.
- In scope for that Workspace: required certificate material/references, Listener transport and TLS/mTLS, protocol packages, protocol/document rules, fault behavior, and Android routing needed by the environment.
- Explicit non-goals: no mutation of any other Workspace and no mutation of application-global settings.
- Shared resources may be imported only to satisfy references reachable from the target Workspace; unrelated shared inventory must remain unchanged.
- Still unresolved: the exact validation success ladder, accepted input/secret channels and lifetime, atomic cross-resource rollback, and apply evidence.
- Updated ambiguity: 0.22 (standard threshold: 0.20; Non-goals resolved, Decision Boundaries mostly resolved).

### Round 6 — Validation success level

- User decision: B — before apply, the candidate must pass static configuration/domain validation, certificate format/role/key matching, rule validation, protocol-package compilation/validation, DNS/TCP connectivity, TLS/mTLS handshake, SNI/hostname/chain verification, and local port availability checks.
- Explicit non-goal: the validation phase does not send a business message and must not claim application-protocol or business success.
- Evidence must report every layer separately; a higher layer cannot hide a lower-layer failure, and partial success cannot authorize apply.
- Remaining closure gate: allowed certificate/key/password input channels and staging lifetime.
- Updated ambiguity: 0.16 (below standard threshold 0.20, but practical closure gate remains open).

### Round 7 — Sensitive material input channel

- User decision: A — accept only certificate/key material directly submitted in the MCP request as PEM text or bounded binary content.
- Reject arbitrary local filesystem paths and remote URLs; MCP must not gain general file-read or network-fetch authority.
- Before confirmation, private material and passwords are memory-only. After successful atomic apply, protected material is stored only through the existing protected certificate boundary.
- Private material/passwords must never appear in previews, normal tool results, diagnostics, logs, audit records, or errors; previews contain public certificate metadata and stable fingerprints only.
- Newly exposed hard requirement: because the current MCP transport is stateless, preview-to-apply needs an opaque, one-use, revision-bound candidate token with an explicit expiry; the timeout value requires user confirmation.
- Updated ambiguity: 0.08, but the final timeout contract is a blocking closure gate under repository zero-assumption rules.

### Round 8 — Candidate lifetime

- User decision: C — a candidate may remain valid until the application exits.
- Mandatory invalidation remains stricter than elapsed lifetime: the token is one-use and immediately invalidated by any change to the target Workspace, affected runtime state, submitted material/configuration fingerprint, or application generation relevant to the preview.
- Application shutdown clears all staged candidates, private material, and passwords from memory.
- Closure audit found one remaining security boundary: whether “对外” means local MCP-client exposure or remote-network exposure. This must be confirmed because the accepted ADR currently permits only unauthenticated loopback read access.
- Updated ambiguity: 0.08; remote exposure remains a blocking Decision Boundary.

### Round 9 — Remote exposure update (requires disambiguation)

- User direction: expose MCP on all IP addresses and require no “verification”.
- This supersedes the current loopback-only network boundary in ADR-004.
- Terminology ambiguity: “不需要验证” may mean no client identity authentication, or it may also revoke the previously selected technical environment-validation level B.
- Security consequence if it means unauthenticated remote writes: any host that can reach the MCP port can submit certificate/key material, request a candidate, obtain its own one-use token, and attempt a confirmed apply. The preview token provides integrity/staleness protection but no caller identity or authorization.
- Updated ambiguity: 0.20 until authentication vs environment-validation semantics are separated.

### Round 10 — Authentication versus technical validation

- User decision: A — no MCP client authentication, authorization, source-IP allowlist, or caller verification; retain the complete technical environment validation level B, revision-bound preview, and explicit apply call.
- Network boundary: MCP is intentionally reachable on all host IP addresses. Any reachable host has equal configuration authority and can generate and apply its own candidate after satisfying technical validation.
- The apply token is only an integrity/staleness control; it is not an identity, authorization, or user-presence proof.
- Closure audit found an independent transport question: the existing endpoint is plaintext HTTP, while the accepted input contract carries private keys/passwords. Server-side transport TLS must be decided separately from client authentication.
- Updated ambiguity: 0.09; MCP transport encryption is the remaining blocking security contract.
