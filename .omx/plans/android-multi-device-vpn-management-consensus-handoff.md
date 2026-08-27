# Consensus Handoff: Android multi-device VPN management

Task: `TASK-20260827-001`

Consensus time: `2026-08-27 11:45:45 +08:00`

Status: `APPROVED_FOR_IMPLEMENTATION`

## Frozen inputs

- PRD: `.omx/plans/prd-android-multi-device-vpn-management.md`
- Test spec: `.omx/plans/test-spec-android-multi-device-vpn-management.md`
- Requirement spec: `.omx/specs/deep-interview-android-multi-device-vpn-management.md`
- Formal task: `docs/tasks/pending/2026-08-27/android-multi-device-vpn-management.md`

## Consensus record

- Planner: Option A selected — bounded per-serial registry, explicit serial/epoch contracts, per-device lifecycle gates and SQLite row CAS.
- Architect Review 1: `REVISE`.
- Architect Review 2: `APPROVE` after Application RW gate, full-owner shutdown, Environment set-diff and DB capacity trigger were frozen.
- Critic Review 1: `REVISE`.
- Critic Review 2: `REVISE`.
- Critic Review 3: `APPROVE` after implicit-write query gates, epoch/error wire, exact MCP/Environment contracts, all-owner destructive configuration guards and gate retirement were frozen.

## Non-negotiable product boundaries

- Per-device independent profile and complete management.
- At most 8 retained runtime records; disconnected/failure/cleanup states consume capacity.
- Disconnect retains the record and waits for the same serial.
- No takeover, batch/broadcast, offline abandonment or implicit selected-device runtime targeting.
- No upgrade/migration: bump current schema version and use the existing development-database rebuild path; do not add a v20 owner migration.
- No new dependency and no Android Companion protocol change.

## Implementation waves

### Wave 1 — Shared contract freeze (serial)

Owner: one backend executor.

- Application Android port/view models/error codes.
- Host/Tauri DTOs and generated bindings.
- Fresh SQLite table/trigger/store collection API.
- RED tests for explicit serial, runtime_epoch, 8/9 capacity and no singleton API.

Do not parallelize shared DTO, schema or generated binding edits.

### Wave 2 — Runtime and UI (parallel after Wave 1 passes)

Backend owner:

- Infrastructure registry, Weak per-serial gates, Reverse/LAN/device-only lifecycle.
- status/endpoints reconciliation, restart, disconnect/reconnect, shutdown.
- Application configuration RW gate; profile/workspace delete, replace/import/reset safety.
- diagnostics, Environment collection/set-diff, MCP read-only and reset consumers.

Frontend owner:

- online devices plus runtime owners union keyed by serial.
- per-device profiles/actions/status/endpoints/packages.
- runtime_epoch cache/event isolation, offline display and capacity UI.

Shared-file rule: only the backend contract owner regenerates bindings; frontend consumes the frozen artifact. Any contract mismatch returns to the leader instead of local divergence.

### Wave 3 — Integration and verification (serial)

- Run targeted Rust/Frontend tests, binding determinism, fmt/clippy/typecheck/lint/static/build gates.
- Save formal evidence under `docs/testing/evidence/<date>/TASK-20260827-001/<case>/`.
- Update Android VPN architecture, persistence/security and user-operation docs.
- Independent code review for cross-device targeting, stale epoch, capacity, shutdown/reset and UI late responses.
- Verifier confirms evidence and task/index consistency before task closure.

## Required lock/order invariants

`Application configuration guard -> canonical sorted Environment resource gates -> one per-serial operation gate -> short registry snapshot/commit`.

Never hold registry lock across SQLite executor await. Never acquire two per-serial gates in one task. Shutdown/reset decompose the owner list into independent serial operations and use under-gate paths.

## Mandatory validation claims

1. A blocked/failing operation cannot delay or mutate B beyond shared immutable configuration reads.
2. Status/endpoints are treated as state-writing lifecycle operations for their serial.
3. Ninth admission fails before all device-side effects.
4. Failed mutations remain `Err(AppError)` and only persisted authoritative epochs are exposed.
5. Any retained owner protects its profile/Workspace from deletion/replacement; reset performs no destructive store write while cleanup remains unconfirmed.
6. Real multi-device validation is `NOT_RUN` when hardware is unavailable, never inferred from mocks.

## Workspace caution

The worktree contains unrelated user/agent changes. Implementation must inspect current diffs before every shared-file edit, preserve unrelated changes, and stage/commit nothing unless separately requested.

## Stop condition

Implementation is not complete until the PRD acceptance criteria and test-spec gates have fresh evidence, an independent reviewer returns APPROVE/CLEAR, and the formal task plus evidence indexes are consistent.
