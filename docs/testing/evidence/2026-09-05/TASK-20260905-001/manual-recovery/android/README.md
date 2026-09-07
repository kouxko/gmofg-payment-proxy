# Android protected saved recovery admission

- Task: `TASK-20260905-001`, architecture review item `#3`.
- Executed: 2026-09-05, Asia/Shanghai.
- Purpose: prove that protected saved recovery waits for an explicit desktop start/apply without creating a generation, lease, Service start, or TUN; preserve device-only recovery when the protection switch is disabled.
- Parent evidence: `docs/testing/evidence/2026-09-05/TASK-20260905-001/android-contracts/`.

## TDD result

`outputs/red-control-lease-recovery.log` is the real pre-production RED. The new test did not compile because the old coordinator had no `recover` admission and no `RecoveryAdmission` result. `inputs/ControlLeaseRecoveryTest.RED.kt` is the exact test input used for that RED.

The implementation then passed seven recovery/CAS tests: protected admission does not enqueue or mutate generation/lease; explicit desktop start still accepts its owner heartbeat and expires at the exact 5000 ms boundary; unprotected device-only recovery enqueues with no lease; enqueue rollback retains the previous generation/lease; stale cleanup cannot close an interleaved desktop generation; idle rejected recovery can clean Service resources without changing registry state.

## Verification

- `outputs/green-affected-jvm.log`: affected recovery, watchdog, registry and native failure tests passed, 25/25.
- `outputs/green-all-jvm.log` and `outputs/junit/`: all JVM tests passed, 39/39, zero failures/errors/skips.
- `outputs/androidtest-final-compile.log`: the two tests in `SavedRecoveryAdmissionInstrumentedTest` compiled into the Android test APK.
- `outputs/device-availability.log`: no device was attached, so instrumentation execution is `NOT_RUN`; compilation is not reported as device execution.
- `outputs/release-build-and-stage.log`: repository build gate passed `lintRelease`, all JVM tests, release assembly, fixed certificate validation, APK alignment, all four JNI ABIs, and staging.
- `resources/apk/android-companion-release.apk`: the actual release APK produced and verified in this run. Its SHA-256 equals the staged desktop resource; see `outputs/apk-sha256.log`.
- UI screenshots and network frames: N/A. This is a Kotlin lifecycle/registry contract with no UI or network exchange.

`inputs/recovery-scenarios.json` records the input and expected outcomes. `outputs/actual-recovery-results.json` records the actual field-level result. `replay/commands.sh` contains the replay commands and fixed local tool paths used in this run.
