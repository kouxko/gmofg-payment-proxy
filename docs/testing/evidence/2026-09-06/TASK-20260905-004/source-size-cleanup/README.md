# Source-size responsibility cleanup

- Task: `TASK-20260905-004`
- Case: `source-size-cleanup`
- Executed: `2026-09-06 09:35:20 +08:00`
- Result: `PASS` for the four assigned files and their regressions

## Purpose

Split four legacy files at existing responsibility boundaries without changing public DTOs, serialization, environment configuration behavior, UI behavior, or test discovery counts.

## Responsibility moves

- Domain Listener model: moved Socket-specific DTOs and strict tagged-union deserialization into `workspace/listener_model/socket.rs`; the parent retains common and HTTP Listener models.
- Application certificate requirements: moved running-listener mutation guards into `listener_certificates/runtime_guards.rs`; the parent retains certificate persistence and TLS/connection cases.
- Environment configuration Listener: moved certificate/secret material reference projection into `listener/materials.rs`; schema DTOs, validation and network target projection remain in the parent.
- Android network view tests: moved shared mocks/default fixtures into `android-network-view.test-fixtures.ts` and layout/start/routing intent cases into `android-network-view.network-intent.test.tsx`; test assertions were moved unchanged.

## Line counts

| Original file | Before | After | Extracted file(s) |
| --- | ---: | ---: | --- |
| `domain/src/workspace/listener_model.rs` | 545 | 399 | `listener_model/socket.rs`: 158 |
| `application/src/requirements_tests/listener_certificates.rs` | 515 | 298 | `listener_certificates/runtime_guards.rs`: 220 |
| `application/src/environment_configuration/listener.rs` | 503 | 410 | `listener/materials.rs`: 103 |
| `src/features/android-network/android-network-view.test.tsx` | 515 | 415 | network intent: 260; fixtures: 223 |

## Commands and actual results

1. `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-domain workspace -- --nocapture`
   - Baseline and after: 29 unit tests plus 2 unified workspace contract tests passed. See `outputs/domain-workspace-baseline.txt` and `outputs/domain-workspace-after.txt`.
2. `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application --lib requirements_tests::listener_certificates -- --nocapture`
   - Baseline and after: 11 passed; moved runtime tests remain discovered under `listener_certificates::runtime_guards`. See the corresponding baseline/after outputs.
3. `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application environment_configuration -- --nocapture`
   - Baseline and after: 212 passed. See `outputs/application-environment-listener-baseline.txt` and `outputs/application-environment-listener-after.txt`.
4. `deno task test src/features/android-network/android-network-view.test.tsx [network-intent file after split]`
   - Baseline: 1 file, 18 tests passed. After: 2 files, the same 18 tests passed. See `outputs/android-network-view-baseline.txt` and `outputs/android-network-view-after.txt`.
5. `deno check` for the three Android network test files
   - PASS.
6. `deno task scan:source-size`
   - PASS: repository-wide source-size gate passed; all nine files in this slice are below 500 physical lines. See `outputs/source-size-scan.txt`.

## Resources

- `resources/before/` contains the four original files from the unchanged repository baseline.
- `resources/after/` contains the nine final source/test files used by the successful regressions.

## N/A

- Protocol frames, external services, TLS, UI screenshots and devices are N/A for a pure source responsibility split.
- Full workspace check and strict Clippy are owned by the root agent after all concurrent source changes freeze.
- CI, commit, push and publication were not requested for this slice.
