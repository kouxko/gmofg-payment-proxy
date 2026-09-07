# Android lease and native-generation contracts

- Task: `TASK-20260905-001`
- Executed: `2026-09-05 14:45:41 +08:00`
- Scope: desktop ADB control-lease heartbeat isolation and Android Companion native failure generation isolation.

## Preconditions and environment

- Repository worktree contained concurrent changes; validation used the current files without reverting other agents.
- Rust commands used the root manifest `src-tauri/Cargo.toml`.
- Companion commands used Android Studio JBR at `/Applications/Android Studio.app/Contents/jbr/Contents/Home` and the existing Gradle 9.4.1 distribution. Versions are recorded in `toolchain.log`.
- `device-availability.log` records `/Users/codin/Library/Android/sdk/platform-tools/adb devices -l`; no Android device was attached, so installation and real-device lifecycle acceptance were not run.

## RED reproduction

1. `red-heartbeat-gate.log`: temporarily restored the former same-serial `device_operations` gate acquisition inside the real `AndroidAdbAdapter::renew_control_lease_target` path, then ran:

   ```text
   cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure same_serial_business_gate_does_not_block_real_heartbeat_path
   ```

   Expected and actual: the real heartbeat path timed out while the same adapter's same-serial business gate was held (`Elapsed(())`), 0 passed / 1 failed. The temporary old code was removed immediately afterward.

2. `red-native-generation-race.log`: temporarily restored the former split `faultedIfCurrent` then teardown sequence, then ran:

   ```text
   JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' /Users/codin/.gradle/wrapper/dists/gradle-9.4.1-bin/arn2x92ynaizyzdaamcbpbhtj/gradle-9.4.1/bin/gradle :app:testDebugUnitTest --tests com.interceptproxy.vpn.NativeDataPlaneFailureCoordinatorTest.replacementStartCannotEnterBetweenCurrentFailureCheckAndTeardown
   ```

   Expected and actual: replacement start completed while old-generation teardown was blocked, failing the assertion at line 73, 0 passed / 1 failed. The atomic implementation was restored immediately afterward.

## GREEN verification

- `green-heartbeat.log`: two real-adapter tests passed. A heartbeat completes while the actual same-serial business gate is held, and a replaced epoch is rejected before transport.
- `green-forward-port-isolation.log`: concurrent control requests reserve distinct loopback ports.
- `green-forward-cancel-cleanup.log`: a cancelled stalled request removes exactly its owned serial and local port while another request progresses. Heartbeat still creates a temporary ADB forward; this proves isolation and cleanup of that temporary resource, not absence of forward activity.
- `green-companion-targeted.log`: `NativeDataPlaneFailureCoordinatorTest` and `ControlLeaseWatchdogTest` passed.
- `green-companion-full.log`: full Companion debug JVM suite passed; the generated XML contained 32 tests, 0 failures, 0 errors, 0 skipped.
- `green-companion-release-build.log`: `scripts/build-android-companion.sh` passed lint, JVM tests, release APK assembly, both probe APK assemblies, signing, package, alignment, and four-ABI checks. The APK contains `arm64-v8a`, `armeabi-v7a`, `x86`, and `x86_64` native libraries.
- `jni-libraries-sha256.log`: hashes of the four generated `app/src/main/jniLibs/*/libintercept_proxy_android_engine.so` inputs packaged into the APK.
- `apk-sha256.log`: release build output, staged desktop resource, and archived evidence APK are byte-identical with SHA-256 `be5b6247035bd0cc7ed4dd8b7aaaadcac1f8cb06e779d8e5e20cd1294415cce1`.
- `resources/android-companion-release.apk`: immutable APK snapshot actually verified by the release gate.
- `inputs/` and `test-source-sha256.log`: exact source snapshots and hashes for the three task-specific JVM/Rust regression test files used by this run.
- `green-static.log`: metadata JSON parsing, source snapshot equality, targeted Rust formatting, and tracked diff whitespace checks passed.

The actual Companion GREEN commands used the following existing local tools:

```text
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' /Users/codin/.gradle/wrapper/dists/gradle-9.4.1-bin/arn2x92ynaizyzdaamcbpbhtj/gradle-9.4.1/bin/gradle :app:testDebugUnitTest --tests com.interceptproxy.vpn.NativeDataPlaneFailureCoordinatorTest --tests com.interceptproxy.vpn.ControlLeaseWatchdogTest
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' /Users/codin/.gradle/wrapper/dists/gradle-9.4.1-bin/arn2x92ynaizyzdaamcbpbhtj/gradle-9.4.1/bin/gradle :app:testDebugUnitTest
PATH='/Users/codin/.gradle/wrapper/dists/gradle-9.4.1-bin/arn2x92ynaizyzdaamcbpbhtj/gradle-9.4.1/bin:/usr/bin:/bin:/usr/sbin:/sbin' JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' ./scripts/build-android-companion.sh
```

## Expected behavior and result

- Heartbeat is an explicit exception to the same-serial business operation gate. It snapshots and revalidates the current owner/runtime epoch before sending and does not mutate persistent owner, reverse, endpoint, or forwarding state. Its protocol request owns a separate temporary ADB forward, reserved by unique local port and cleaned by exact serial plus port.
- A native failure callback carries its originating generation. Only the current desired generation can atomically publish `faulted` and tear down the TUN/data plane. A stale generation performs no teardown and emits no native-failure error log.
- Current-generation native failure remains immediate fail-open.

The build also regenerated ignored working files under `android-companion/app/src/main/jniLibs/`, `android-companion/app/build/`, the two probe build directories, and `src-tauri/resources/android-companion.apk`. The staged resource is byte-identical to the archived APK; these generated paths are not source diffs.

## Result and unexecuted validation

- Overall result: `PASS_WITH_DEVICE_NOT_RUN`.
- `NOT_RUN`: real-device install, VPN start/stop, control-loss timing, and Android 7 hardware acceptance. These checks are applicable but could not run because `adb devices -l` reported no attached device.
- `NOT_RUN`: CI was not authorized or invoked.
- `N/A`: external payment/transaction network validation is outside this task.

## 父用例与变化

- 父任务：`TASK-20260828-004`；父用例：`ANDROID-CONTROL-LEASE-001`。
- 父证据：[README](../../../2026-08-28/TASK-20260828-004/ANDROID-CONTROL-LEASE-001/README.md)。
- 复用默认5秒、关闭开关、epoch/generation隔离；本次增加真实业务gate占用与native代次清理竞态。
