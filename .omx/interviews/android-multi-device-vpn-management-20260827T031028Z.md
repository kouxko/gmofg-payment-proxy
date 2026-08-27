# Deep interview: Android multi-device VPN management

- Profile: quick
- Context: brownfield
- Context snapshot: `.omx/context/android-multi-device-vpn-management-20260827T031028Z.md`
- Final ambiguity: 0.08
- Threshold: 0.30
- Prompt-safe initial-context summary: not_needed

## Clarified intent

Replace the current single-runtime-owner restriction with bounded multi-device VPN operation and complete per-device management. The user needs multiple payment terminals to remain valid concurrent runtimes, not a takeover workflow where starting one device invalidates another.

## Interview rounds

1. Runtime model: user selected **independent profile per device**.
2. Management scope: user selected **complete management** for every device.
3. Capacity pressure: user selected **at most 8 concurrently running VPN devices**.
4. Disconnect lifecycle: user selected **retain disconnected runtime records and wait for the same serial to reconnect**.
5. Upgrade boundary: user explicitly selected **do not consider upgrade**; no singleton-owner migration or old-database data-preservation path is required.

## Pressure-pass findings

- The earlier single-owner “explicit takeover” framing was challenged by the broader concurrency requirement and superseded.
- Disconnect is not equivalent to stop: a disconnected device remains an independently managed runtime record and must not block or mutate other devices.
- Bounded capacity is explicit, allowing deterministic polling, persistence, and cleanup rather than an unbounded fleet-management system.
- The existing development-database rebuild contract remains authoritative; this task validates the fresh multi-owner schema and runtime restart, not upgrade migration.

## Binding boundaries

- Each device independently owns its profile, runtime endpoints, status, reverse mappings, errors, and lifecycle operations.
- Full management includes install/update, consent, package inventory, profile start/apply, status, stop, emergency restore, and endpoint visibility per device.
- Maximum concurrent runtime records: 8.
- Disconnected records remain visible and wait for same-serial reconnect.
- No global takeover, no automatic offline-record deletion, no shared-profile batch rollout, and no unbounded device fleet in this task.
- No compatibility migration or preservation of an existing singleton owner across a schema-version change.
