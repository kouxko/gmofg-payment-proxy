# Android emulator proxy gate

This test-only gate sends the selected Android app to an intentionally unreachable original
server address on independent DLL and Transaction ports. The app never receives a desktop proxy
address. Its VpnService profile maps each original destination/port to a different Workspace
Listener; temporary `adb reverse` endpoints then reach the two host-side listeners, which forward
to different local Shift-JIS upstream fixture ports. The Rust runner builds the normal
`ApplicationHost` with `InterceptProxyProfile`, creates and selects an empty Workspace, saves a
Reverse Listener plus body policies, assertions and rules, and validates every captured session.

The deterministic matrix covers:

1. unchanged Shift-JIS D48 bytes, ordered raw headers and five response assertions;
2. request Header and JSON field modification as observed by the upstream fixture;
3. response status, Header and Shift-JIS Body modification as observed by Android;
4. a terminal Mock response that does not contact upstream;
5. second-hit plus one-shot behavior across three requests, including persisted disable state;
6. a fixed 250 ms delay;
7. response truncation with the original declared `Content-Length`;
8. dropping a fully-read upstream response;
9. disconnecting before the upstream connection is created.
10. a separate Transaction listener/upstream preserving the same Shift-JIS D48 bytes and exposing
    its own session/listener identity.
11. protocol-equivalent DLL and Transaction requests from a selected Android app that still uses
    the original server address, with each TCP flow transparently mapped to its referenced Listener;
12. `destination_targets` affecting only the DLL weak-network scope while both proxy routes still
    succeed, proving that routing and impairment selection are independent;
13. byte-for-byte Shift-JIS D48 preservation before and during the VPN impairment, with live TUN,
    SOCKS and injected-delay counters embedded into the same report.

Request-stage scenarios match the request path. Response-stage rules deliberately match the
fixture's `$.scenario` JSON field: the current formal `PathOrRequestType` field evaluates the
response start-line target (the HTTP status token) at response stage, so it cannot represent the
original request path. The gate records this product boundary instead of injecting a test-only
request-path context.

Run:

```sh
test-support/emulator-proxy-gate/run.sh
```

Set `ANDROID_SERIAL` when more than one device is connected. The latest machine-readable Rust
capture report is written to `reports/latest.json`. The report includes the nested
`vpn_joint_probe` evidence for the direct listener baselines and transparently routed original
DLL/Transaction targets. The script rejects
physical devices and removes the previous report before each run, so a stale success cannot be
confused with the current run.

## Evidence boundary

The upstream is simulated locally and returns a test-owned D48 body. Passing this gate proves the
emulator, `adb reverse`, Workspace Listener, local HTTP upstream, the production Workspace rule
repository/pipeline, Shift-JIS decoding, response assertions, capture headers, mutation and terminal
fault paths, and a selected Android UID traversing VpnService plus transparent proxy routing work
together without changing the test app's server address. The
Android client is protocol-equivalent test traffic, not the production Payment APK. It does **not** replace an
A920MAX test against the real GMO-FG service and must never be cited as production D48 evidence.
