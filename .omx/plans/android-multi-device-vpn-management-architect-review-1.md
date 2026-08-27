# Architect Review 1: Android multi-device VPN management

Verdict: `REVISE`

## Required revisions

1. P0: Application `mutation_gate` currently spans Android ADB awaits and would serialize A/B even after Infrastructure gains per-serial gates. Replace the relevant global exclusive contract with shared runtime-read/exclusive configuration-write semantics and prove A can block while B proceeds.
2. P0: `app_shutdown` currently reads/stops one owner. Define all-owner graceful shutdown versus persisted restart recovery and test every owner is attempted without short-circuit.
3. P1: Environment apply baseline/lease/publishing must reconcile an exact sorted collection keyed by `profile_id + serial`; `Option` wording is insufficient.
4. P1: Freeze one capacity implementation. Use SQLite transaction plus a DB trigger so a future caller cannot bypass the 8-row invariant.

## Accepted boundary

- No upgrade/migration is coherent, but `CURRENT_SCHEMA_VERSION` must still advance so current v20 development databases enter the existing rebuild path.

## Evidence

- `src-tauri/crates/application/src/facade/android/activation.rs`
- `src-tauri/crates/application/src/facade/lifecycle.rs`
- `src-tauri/crates/infrastructure/src/adapters/environment_configuration_baseline_capture.rs`
- `src-tauri/crates/infrastructure/src/adapters/environment_configuration_lease.rs`
- `src-tauri/crates/infrastructure/src/adapters/android_adb/environment_apply.rs`
- `src-tauri/crates/infrastructure/src/sqlite/schema.rs`
- `src-tauri/crates/infrastructure/src/sqlite/core.rs`

## Synthesis

Keep Option A and the no-migration boundary. Explicitly close the Application gate, graceful shutdown, environment set-diff, and database capacity holes before Critic review.
