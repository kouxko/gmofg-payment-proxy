# Backend contracts: Bootstrap cursor and raw HTTP hop headers

- Task: `TASK-20260905-001`
- Case: `backend-contracts`
- Executed: `2026-09-05 14:38:06 +08:00`
- Result: `PASS`

## Purpose

Verify two independent contracts:

1. `app_bootstrap` returns the event cursor captured before snapshot reads, so an event published while an awaited snapshot port is blocked is replayed by the following subscription.
2. Fixed-upstream HTTP removes standard hop-by-hop fields, `Proxy-Connection`, and every field named by all `Connection` header lines while preserving unrelated raw header name, order, duplicate values, optional whitespace, body, and response behavior.

## RED evidence

- `red-bootstrap.log`: the old implementation returned cursor `1` instead of the pre-snapshot cursor `0`.
- `red-http-unit.log`: the old implementation retained `X-Private-Hop`, `x-second-hop`, and `Proxy-Connection`.
- `red-http-integration.log`: the TCP upstream received a forbidden hop-by-hop field.

The initial accidental zero-test filtered command was overwritten by the actual failing Bootstrap run; the retained log contains one executed and failed test.

## GREEN evidence

- `green-bootstrap.log`: deterministic blocked-snapshot test passed.
- `green-http-unit.log`: exact normalized raw head comparison passed.
- `green-http-integration.log`: real TCP upstream received no declared hop headers and retained binary, duplicated custom headers.
- `green-fault-semantics.log`: all four established fault-stage contracts passed.
- `green-chunked-response.log`: all three chunked response regressions selected by `chunked` passed.
- `green-application-events.log`: complete application event requirement module passed, 8/8.
- `green-message-tests.log`: complete raw Message unit module passed, 11/11.
- `green-upstream-http.log`: complete fixed-upstream HTTP integration suite passed, 7/7.
- `green-cargo-check.log` and `green-clippy.log`: both affected crates typechecked and passed Clippy with warnings denied.
- `green-rustfmt-check.log` and `green-diff-check.log`: affected files were formatted and contained no whitespace errors.

## Inputs, expected, and actual

- `inputs/bootstrap-timeline.json` records the deterministic event sequence.
- `inputs/raw-request-head.txt` is the readable view of the hop-by-hop request input; the test source identified by `resources/source-sha256.txt` contains the authoritative CRLF byte literal. The integration test additionally carries binary values `first 80` and `middle ff` in `X-Trace`/`x-Other`.
- `expected/bootstrap-result.json` and `expected/normalized-request-head.txt` define the expected cursor/replay and wire head.
- `outputs/bootstrap-result.json` and `outputs/normalized-request-head.txt` record the passing actual values.
- `resources/source-sha256.txt` identifies the exact source and test resources used.
- `replay/commands.md` contains the commands.

## N/A

- TLS/mTLS: N/A; the changed Header contract is tested at the plaintext fixed-upstream wire boundary and does not change TLS establishment.
- Payment business response: N/A; the cases validate event ordering and HTTP framing/header semantics, not a payment payload.
- Remote service and Android device: N/A.
- CI/push/release: NOT_RUN; outside task authorization.

## 父用例与变化

- 父任务：`TASK-20260904-005`；父用例：`HTTP-CHUNKED-RESPONSE-001`。
- 父证据：[README](../../../2026-09-04/TASK-20260904-005/HTTP-CHUNKED-RESPONSE-001/README.md)。
- 复用chunked响应、404和故障注入回归；本次仅修正请求逐跳Header和bootstrap事件游标。
