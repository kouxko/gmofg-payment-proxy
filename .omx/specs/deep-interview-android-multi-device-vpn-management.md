# Execution-ready specification: Android multi-device VPN management

## Metadata

- Workflow: deep-interview quick
- Rounds: 4
- Final ambiguity: 0.08
- Threshold: 0.30
- Context type: brownfield
- Context snapshot: `.omx/context/android-multi-device-vpn-management-20260827T031028Z.md`
- Transcript: `.omx/interviews/android-multi-device-vpn-management-20260827T031028Z.md`
- Prompt-safe initial-context summary: not_needed
- Workflow-state note: OMX deep-interview state could not be activated because an unrelated durable Ultragoal state is already active; that state was preserved and not cleared.

## Intent

Allow up to eight Android devices to run VPN concurrently and be managed independently from the desktop application. A device disconnect or failure must remain isolated to that device and must never block, overwrite, stop, or clear another device.

## Desired outcome

- Multiple device serials can simultaneously own verified or transitional VPN runtime records.
- Every device has an explicit independent profile and per-device actions.
- Runtime facts survive desktop restart for every recorded device.
- Disconnected devices remain visible and reconcile only when the same serial reconnects.

## In scope

- Replace the singleton runtime-owner model with a bounded collection keyed by device serial and guarded by owner epoch.
- Persist and restore up to eight runtime-owner records with per-owner reverse ports, runtime endpoints, resume state, fingerprints, mode, transition reason, and timestamps.
- Make install, update, VPN consent, package list/query/refresh, start, apply, status, stop, emergency restore, and endpoint queries explicitly device-scoped.
- Support independent profiles per device; different devices may use different profiles, and using the same saved profile on multiple devices does not merge their runtime ownership.
- Keep ADB Reverse, LAN, and device-only modes isolated per serial/epoch.
- Present connected and disconnected managed devices with per-device state and actions.
- Create the multi-owner collection as the new current schema; do not migrate or preserve an existing singleton owner across schema upgrade.
- Add deterministic concurrency, restart, disconnect/reconnect, capacity, stale-epoch, and cross-device isolation tests.

## Out of scope / non-goals

- Shared-profile broadcast or bulk apply to multiple devices.
- More than eight concurrent runtime records.
- Remote/fleet management outside devices visible through the configured local ADB server.
- Automatic deletion of disconnected runtime records.
- Treating disconnect as verified stop.
- UI redesign outside the Android device-network management surface.
- New dependencies or unrelated listener/protocol changes.
- Compatibility upgrade, in-place migration, or data preservation for an older singleton-owner database.

## Decision boundaries

- Implementation may choose the internal collection/storage representation, schema migration shape, and bounded polling scheduler, subject to atomic persistence and cross-device isolation.
- Implementation may choose table/list/card presentation, provided every action is explicitly bound to the displayed serial and epoch.
- Implementation must not introduce batch semantics, implicit current-selection targeting, or destructive offline-owner abandonment without a new user decision.

## Constraints

- Device operations carry an explicit serial; runtime-mutating operations also validate the expected epoch where stale results could clear or overwrite state.
- One device's failure cannot change another device's owner, endpoints, reverse mappings, status, selected profile, or UI query cache.
- ADB forward/reverse cleanup is serial-specific. Device-side ports may repeat across distinct serial namespaces, but desktop ownership facts remain independent.
- Persistence updates are atomic and bounded inside the new schema; partial multi-owner writes fail without losing the prior authoritative collection state.
- Current worktree contains unrelated user changes and they must not be reverted or included accidentally.

## Acceptance criteria

1. With device A running profile PA, selecting device B and starting profile PB succeeds without stopping or mutating A.
2. Status, apply, stop, and emergency restore for A issue commands only to A; the same operations for B issue commands only to B.
3. Install/update, consent, and package inventory commands accept and use the explicitly chosen device serial rather than ambient global selection.
4. A and B may simultaneously use ADB Reverse, LAN, or device-only modes in any combination, with independent endpoints and epochs.
5. Disconnecting A changes only A to `waiting_reconnect`; B remains operable and can start/apply/stop normally.
6. Reconnecting serial A reconciles only A and restores its prior protected state classification without changing B.
7. Desktop restart restores all recorded owners, reverse cleanup facts, endpoints, and transition states.
8. An operation attempting to create a ninth concurrent runtime record fails before device mutation with a stable capacity error and leaves the eight records unchanged.
9. A stale A epoch cannot clear or overwrite a newer A epoch and cannot affect any B epoch.
10. A fresh current-schema database creates the bounded multi-owner table, and a version-mismatched development database follows the repository's existing rebuild contract without a special owner migration path.
11. UI lists all managed devices, distinguishes connected/disconnected and selected/editing context, and exposes complete per-device management actions without relying on a singleton owner query.
12. Frontend cache keys and events include serial and, for runtime facts, epoch; late A responses cannot overwrite B.
13. Targeted Rust, Application/Host/IPC, frontend, migration, typecheck/lint/architecture, and Android companion compatibility checks pass; unavailable real multi-device validation is reported `NOT_RUN`, not PASS.

## Technical context

- Current singleton guard: `src-tauri/crates/infrastructure/src/adapters/android_adb/owner.rs`.
- Current singleton persistence: `src-tauri/crates/infrastructure/src/sqlite/android_runtime_owner.rs` and schema lifecycle modules.
- Current implicit selection and singleton port contract: `src-tauri/crates/application/src/ports/android.rs` and `src-tauri/crates/infrastructure/src/adapters/android_adb.rs`.
- Current singleton UI/query model: `src/features/android-network/android-network-view.tsx`, `device-control-card.tsx`, `profile-cards.tsx`, and runtime-owner model/tests.

## Documentation and terminology ledger

- `docs/architecture/android-vpn-transparent-routing.md` currently documents one runtime owner and must be updated only after implementation establishes the new authoritative model.
- `docs/architecture/security-and-persistence.md` requires preservation of disconnect cleanup facts.
- `.omx/plans/prd-todo-remediation.md` and its test spec intentionally locked the old single-owner behavior; this task supersedes only that bounded R02 product contract.
- Canonical new term: `runtime owners` / `运行设备记录` as a bounded per-device collection. `selected device` remains editing context, not runtime authority.

## Recommended handoff

Use `$ralplan --deliberate --direct` because this changes persistence schema, public command contracts, concurrency/lifecycle ownership, frontend query identity, and migration behavior. After consensus, use a durable execution lane with independent implementation and verification ownership.
