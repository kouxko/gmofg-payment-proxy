# Android multi-device VPN management context

- Task statement: support multiple Android devices running VPN simultaneously and manage them from the desktop application.
- Desired outcome: no global single-owner lock prevents a second device from starting; every running device remains independently observable and controllable.
- Scope change: this supersedes the earlier single-owner explicit-takeover direction. A prior device does not have to surrender ownership merely because another device starts.
- Prompt-safe initial-context summary: not_needed.

## Current code facts

- The current desktop adapter stores exactly one persisted `runtime_owner`, one active runtime, one reverse ownership record, one resume state, and one runtime endpoint set.
- Start/Apply rejects when the selected serial differs from that single owner.
- Stop, Status, Emergency Restore, runtime polling, UI display, and endpoint queries resolve through that single owner.
- Existing tests intentionally enforce the one-owner model.

## Confirmed user decisions retained where compatible

- Multiple devices must be allowed to run VPN concurrently.
- Devices must be manageable rather than silently abandoning old state.
- All transport modes are expected to be considered unless narrowed later.

## Superseded decisions

- “Explicitly take over from the old device” is no longer the primary model because concurrent owners are now required.
- “Automatically stop the old device after it reconnects” conflicts with valid multi-device concurrency and is not retained without a narrower cleanup-specific rule.

## Constraints

- Device operations must target an explicit serial plus owner epoch; never use the UI selection as an implicit substitute for another device.
- A failure or disconnect on one device must not stop, overwrite, or clear another device.
- ADB reverse port allocation and cleanup must remain isolated per device/epoch.
- Persistence and restart recovery must represent all running/uncertain/cleanup-required devices truthfully.
- No unrelated refactor or new dependency.
- Formal high-priority task registration, schema/lifecycle design, regression-first tests, evidence, and adversarial review are required before completion.

## Open decision boundaries

- Whether each device owns an independent profile or all devices mirror one shared profile.
- Which per-device actions are required in the first version: start, apply, status, stop, emergency restore, package/consent/install/update.
- Whether management uses one selected device plus a runtime list, or a full device table with per-row actions.
- Expected behavior for disconnected devices and devices restored after desktop restart.
- Maximum concurrent device count and resource limits.

## Likely touchpoints

- Application Android control port and Tauri command contracts
- Infrastructure Android owner persistence and ADB reverse lifecycle
- SQLite Android runtime-owner schema/migration
- Generated Rust/TypeScript bindings
- Android network management UI and polling/event routing
- Rust/Application/Host/frontend lifecycle and race tests

## Repo contracts inspected

- `AGENTS.md`
- `docs/architecture/android-vpn-transparent-routing.md`
- `docs/architecture/security-and-persistence.md`
- `.omx/plans/prd-todo-remediation.md`
- `.omx/plans/test-spec-todo-remediation.md`
