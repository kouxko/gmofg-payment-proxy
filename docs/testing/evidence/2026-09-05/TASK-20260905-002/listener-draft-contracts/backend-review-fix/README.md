# TASK-20260905-002 backend review fix

## Purpose

Correct the independently reviewed P2: `ServerTls` accepted a Socket `LocalResponder` Draft and silently returned success. The approved Draft IPC contract requires a typed not-applicable error because LocalResponder has no Server upstream.

## Inputs and expected result

`inputs/server-tls-local-responder.json` contains both real boolean intent inputs. `inputs/expected.json` requires `LISTENER_DRAFT_TRANSITION_NOT_APPLICABLE` for both and preserves all Relay App/Server TLS combinations.

## TDD evidence

The authoritative RED run is `outputs/red-server-tls-local-responder-both-inputs.log`. It executes both inputs before comparing and records:

- actual: `[Ok("ok"), Ok("ok")]`
- expected: two `LISTENER_DRAFT_TRANSITION_NOT_APPLICABLE` errors

The earlier `outputs/red-server-tls-local-responder.log` is retained as the first genuine RED attempt; it stopped at the first assertion, so the later RED log is the evidence for both inputs.

The minimal fix moves the Relay topology gate into the `ServerTls` intent branch and changes the private helper to accept only `SocketRelayTopologyDraft`. This removes the helper's silent LocalResponder branch without changing the public signature or Relay TLS composition.

## Verification

- Focused new regression: 1 passed, 0 failed — `outputs/green-server-tls-local-responder.log`
- Complete Draft requirement suite: 13 passed, 0 failed — `outputs/green-listener-draft-suite.log`
- Existing strict listener validation: 2 passed, 0 failed — `outputs/green-existing-listener-validation.log`
- Existing three-command binding test: 1 passed, 0 failed — `outputs/green-command-binding.log`
- Application Clippy with `-D warnings`: PASS — `outputs/clippy-application.log`
- Rust fmt, scoped diff check, and 500-line gate: PASS — `outputs/static-checks.log`

`outputs/actual.json` and `outputs/comparison.json` give the structured actual/expected comparison. `replay/run.sh` contains the repeatable commands.

## Source-state mapping

`metadata.json` maps both original repository paths to before and after snapshots and exact SHA-256 values. Both `SHA256SUMS.txt` manifests explicitly exclude themselves; no invalid self-hash entry is present.

The prior 969-test repository result remains only the recorded pre-fix baseline. This review fix did not rerun or relabel it. Bindings were not regenerated because the public Rust/TypeScript contract did not change.

## N/A

Frontend tests, protocol frames, TCP chunks, real network/TLS, certificate material, lease operations, screenshots, Android devices, CI, push, and commit are N/A for this two-file application intent-boundary fix.
