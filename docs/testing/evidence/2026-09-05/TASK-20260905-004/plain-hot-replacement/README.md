# Plain HTTP keep-alive rule hot replacement

- Task: `TASK-20260905-004`
- Case: `plain-hot-replacement`
- RED executed: `2026-09-06 00:04:03 +08:00`
- GREEN executed: `2026-09-06 01:14:47 +08:00`
- Result: `PASS`

## Purpose and input

One existing Plain HTTP connection crosses three rule generations: no Document rule and non-JSON pass-through; a newly added Document rule changes `{"value":"old"}` to `{"value":"created"}`; removal restores non-JSON pass-through. See `inputs/scenario.json`.

## RED

`cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure existing_plain_http_connection_switches_document_capability_after_hot_replacement -- --nocapture`

Before the repair, generation 2 failed with `CONFIG_INVALID` because a capability created without Document rules did not acquire the new joint Document transaction. Exit 101, 0 passed, 1 failed. See `outputs/red.log`.

## GREEN

The same capability-level command now reports 1 passed, 0 failed. See `outputs/capability-green.txt`.

The localhost integration command is:

`cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure live_plain_keep_alive_connection_observes_document_rule_add_and_remove -- --nocapture`

It uses one client `TcpStream` for all three requests and the running Listener/Fixed Server assembly. The origin accepts one physical upstream connection per request, matching current connector ownership. It reports 1 passed, 0 failed and prints every sent, origin-received and client-received body as exact bytes. Reads, origin accepts and origin task join have two-second test timeouts. See `outputs/live-tcp-green.txt`.

The TCP test uses a focused `LiveHotPolicy` to prove real TCP plus listener snapshot/capability replacement. The production actor Rules/Encode transaction and rollback are separately proven by `joint-write` / `production_snapshot_uses_shared_provider_for_both_directions_and_joint_encode`.

## N/A / not run

- External package, TLS, real payment service, UI, screenshot and device: N/A.
- Full workspace test, strict clippy and architecture guards: delegated to the root agent.
- CI, commit, push and publication: outside this assigned slice.
