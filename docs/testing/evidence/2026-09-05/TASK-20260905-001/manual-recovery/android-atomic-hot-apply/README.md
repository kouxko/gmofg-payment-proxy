# Android atomic active hot apply

- Task: `TASK-20260905-001`, architecture review item `#3` TOCTOU follow-up.
- Parent case: `docs/testing/evidence/2026-09-05/TASK-20260905-001/manual-recovery/android/`.
- Purpose: prevent a cached desktop Active/runtime snapshot from restarting a protected VPN after its lease has expired and Android has stopped it.

## Contract

Protected automatic LAN hot update uses the v2 control operation `apply_active`. Existing `start` and `apply` remain explicit operations without an active-lease prerequisite; automatic unprotected apply also remains `apply`. No payload flag, schema version, profile field, storage field, fallback, or retry was added. An older Companion does not recognize `apply_active` and therefore rejects it through the existing unknown-operation gate.

`apply_active` requires the supplied owner epoch to match an unexpired watchdog lease and requires that lease generation to be the registry's current desired-running `running` generation. Registry validation and `beginStart` execute in one synchronized registry operation while the coordinator lock remains held. Rejection returns the existing `ANDROID_CONTROL_LEASE_STALE` response and does not enqueue, advance generation, or rebuild the lease.

## TDD and verification

- `outputs/red-atomic-hot-apply.log`: the old coordinator lacked the active-lease start prerequisite; all five new call sites failed compilation.
- `outputs/red-apply-active-operation.log`: `apply_active` was rejected by the old v2 supported-operation list.
- `outputs/green-atomic-and-protocol.log`: recovery/atomic admission and protocol tests passed.
- `outputs/green-final2-all-jvm.log` and `outputs/junit/`: all JVM tests passed, 45/45, including explicit unknown-operation rejection.
- `outputs/androidtest-compile.log`: Android instrumentation APK compiled successfully.
- `outputs/device-availability.log`: no device was attached; instrumentation execution is `NOT_RUN`.
- `outputs/release-build-and-stage.log`: lint, JVM tests, release build, fixed certificate, alignment, four JNI ABIs, and staging passed.
- `resources/apk/android-companion-release.apk`: actual final APK from this case.

An Android LocalSocket frame was not executed because no device was attached. The Android v2 operation whitelist was exercised through its pure JVM envelope boundary; the desktop case holds the cross-process encoded-frame roundtrip. Screenshots are N/A because no UI behavior changed. Input and expected fields are in `inputs/contract-scenarios.json`; actual results are in `outputs/actual-results.json`; commands are in `replay/commands.sh`.
