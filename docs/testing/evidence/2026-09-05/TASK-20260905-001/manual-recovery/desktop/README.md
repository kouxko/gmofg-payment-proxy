# Desktop manual recovery regression

Task: TASK-20260905-001 / MANUAL-RECOVERY-001. Parent: [android-contracts](../../android-contracts/README.md).

User contract: protected stopped VPN waits for explicit desktop start/apply. Existing owner/epoch remains for CAS. Protection-disabled device-only recovery and valid live LAN coordination are preserved.

The regression uses the actual AndroidAdbAdapter, application control frame encoder/decoder and a loopback TCP fake Companion. ADB forward owns a per-request socket; exact port cleanup aborts the server. Desktop/device LAN inputs are 192.168.1.20 / 192.168.1.99, previous desktop endpoint is 192.168.1.10, original destination is 203.0.113.10:443, desktop listener port is 8080. UUIDs are created per run. No real ADB transport, Android TUN, external server or business transaction is claimed.

Cases cover seven owner/runtime combinations, stale serial and epoch, observed stopped owner, explicit apply producing a new epoch, healthy LAN update retaining the epoch, and expired lease rejection without automatic retry followed by successful manual apply. The new apply_active operation fails closed on old Companion; explicit apply remains unchanged.

Replay: from repository root run `bash docs/testing/evidence/2026-09-05/TASK-20260905-001/manual-recovery/desktop/replay/run.sh`. Source snapshots and repository mapping are in source-snapshots.json. Prerequisites: repository Rust toolchain, Cargo dependencies and Deno. All Rust sources were held stable for the final verification commands; Android writes run in a separate module.

Evidence includes genuine initial RED (missing runtime permits apply, stopped owner stays Active, protected hot apply misses lease), intermediate compiler/Clippy and operation-whitelist integration failures, then final results. Intermediate logs are preserved, not overwritten. Source-size gate retains the six pre-existing over-500-line files; no new violation is introduced.

Actual framed control request/response bytes from the focused final run are saved under inputs/ and outputs/frames/. Source resources are actual copies; historical parent resources remain unchanged. UI screenshots, real Android instrumentation execution, TUN traffic and upstream business response are N/A for this local Rust regression and remain NOT_RUN at task level.

Final: application 432/432, infrastructure 474/474, focused final 5/5, strict Clippy/fmt/architecture PASS. Final fake-error text alignment is covered by focused rerun. Ten control frames are archived with exact JSON bytes and length prefix in frames.json.
