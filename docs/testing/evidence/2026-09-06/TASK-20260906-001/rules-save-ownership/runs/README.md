# Rules UI race fix lane

- Scope: `src/features/rules/**` only.
- RED started: `2026-09-06 10:51:27 +0800`.
- RED command: `deno task test src/features/rules/rules-view.test.tsx --reporter=verbose`.
- RED result: 7 passed, 2 failed. The failures reproduced late save overwriting a newer selection and two creation saves admitted while draft materialization was pending.
- GREEN started: `2026-09-06 10:55:31 +0800`.
- GREEN command: `deno task test src/features/rules/rules-view.test.tsx src/features/rules/rule-http-action-parameters.test.ts src/features/rules/rule-definition-editor.test.tsx src/features/rules/rule-definition-model.test.ts --reporter=verbose`.
- GREEN result: 4 files passed, 41 tests passed.
- Static checks: `deno task typecheck` and `deno task lint --max-warnings 0` passed.
- Source-size integration check: failed only because `src-tauri/crates/exchange/src/observation.rs` was concurrently at 501 lines outside this lane. All five files changed by this lane are below 500 lines.
- RED and GREEN `resources/` contain exact source/config snapshots used by each run. Command outputs and timestamps are stored alongside them.
