# Rules editor-session ownership follow-up

- Scope: same-identity editor replacements in `RulesView`.
- `red/` preserves the first realistic run, including an initially invalid priority-input setup for repeated New and the valid failing capture A-to-B case.
- `red-final/` is the authoritative RED run after correcting the test setup. Both real `RulesView` paths fail: repeated New leaves the old creation tab/session mounted, and capture draft B with the same nullable rule/listener/stage identity lets draft A submit.
- Production change: one explicit editor-session counter has a state value for React keys and a ref value for async ownership checks. Capture replacement, selection, New, and Cancel all start a new session.
- `green/` contains exact final test and production source snapshots, command output, and timestamps.
- GREEN result: 5 rule test files and 45 tests passed.
- Static validation: `deno task typecheck`, `deno task lint --max-warnings 0`, `deno task scan:source-size`, and `git diff --check -- src/features/rules` passed.
- Existing cancellation, duplicate-save, late-writeback, retry, and backend-failure-notification behavior remains covered.
