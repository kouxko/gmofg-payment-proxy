# Archive source discovery boundary

The historical resources are immutable snapshots, not active project source. Before the fix, TypeScript and ESLint both discovered the archived Listener source. The narrow `docs/testing/evidence/**/resources/**` exclusion now removes only these resources from automatic discovery.

Run from the repository root:

```sh
deno run -A docs/testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/integrated/config-boundary/check.mjs
```

`red.log` records the original failure (exit 1); `green.log` records four passing comparisons (exit 0). Current Listener implementation, its active test, and vitest.config.ts remain included and lintable. The checker reads the real TypeScript configuration and ESLint API; it does not filter compiler diagnostics. Before/after configs are saved as text snapshots. Full typecheck/lint/build are separate integration gates after frontend freeze.

Protocol, network, screenshots and devices: N/A, this verifies source discovery configuration only.
