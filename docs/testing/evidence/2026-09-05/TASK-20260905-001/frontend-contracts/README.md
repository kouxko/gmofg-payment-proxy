# Frontend contracts

- Task: `TASK-20260905-001`
- Executed: `2026-09-05 14:34:02 +08:00` through `2026-09-05 14:51:42 +08:00`
- Source state: `c7a70d3ef63719ae3bb7f9b3e6dca8db995c6525` plus the task worktree changes
- Environment: macOS arm64, Deno 2.9.6, TypeScript 6.0.3, Vitest 4.1.11, jsdom

## Purpose

Prove three frontend concurrency contracts:

1. A Listener draft is rebased onto the newest authoritative Workspace per Listener. Changes to another Listener refresh normally. A concurrent external change to the same dirty Listener preserves the draft, displays a conflict, and blocks save/start/delete.
2. An unrelated event does not invalidate an in-flight app bootstrap request. An event that changes bootstrap state invalidates the old request and always queues a replacement snapshot, including `workspace_changed`.
3. A same-Listener conflict has an explicit per-Listener resolution action. It replaces the conflicted Listener with the latest authoritative value, or removes it after an external deletion, while preserving every other Listener draft and reachable draft certificate.

## TDD execution

RED command:

```text
deno task test src/features/listeners/listener-workspace-draft.test.ts src/features/shell/bootstrap-context.test.tsx
```

The five new assertions failed against the old implementation: `reconcileListenerDraft` did not exist, an `exchange_observation_changed` event discarded the in-flight result, and `workspace_changed` did not start a replacement request. See `red.log`.

GREEN focused command:

```text
deno task test src/features/listeners/listener-workspace-draft.test.ts src/features/listeners/listeners-view.external-refresh.test.tsx src/features/shell/bootstrap-context.test.tsx
```

Result: 3 files and 11 tests passed.

Expanded command:

```text
deno task test src/features/listeners src/features/shell/bootstrap-context.test.tsx src/features/shell/workspace-query-invalidation.test.tsx
```

Result: 19 files and 214 tests passed. This caught and verified the repair for preserving the local Listener order when the authoritative response used another order, which keeps the selected Listener and imported certificate detail stable.

## Conflict-resolution follow-up

The second RED run used:

```text
deno task test src/features/listeners/listeners-view.external-refresh.test.tsx
```

Result before the follow-up implementation: 3 failed and 1 passed. The conflict had no per-Listener discard action, externally deleted Listener A could not be explicitly accepted, and successful validation still described the draft as save/start ready. See `red-v2.log`.

After adding the explicit action, the focused component test passed 4 of 4 tests. The expanded affected surface then passed 19 files and 216 tests. The component cases prove that resolving Listener A preserves Listener B's local name and imported CA lease, and that accepting an external deletion removes A without restoring it. See `green-after-conflict-resolution.log`.

Full frontend command:

```text
deno task test
```

Result: 67 files and 573 tests passed before the conflict-resolution follow-up. The final follow-up was validated by the 216 affected tests, typecheck/lint/boundary checks and final production build; a second full frontend suite was not run.

Static checks:

```text
deno task typecheck
deno task lint
deno task scan:frontend-boundaries
git diff --check -- src/features/listeners src/features/shell
```

All passed.

`deno task scan:source-size` still reports the seven task-baseline oversized files. The modified production view is 496 lines and the new conflict component test is 203 lines, so this task did not expand that known gate.

## Result

PASS. Inputs and expected decisions are recorded in `inputs/scenarios.json`; actual results are recorded in `outputs/results.json` and `green.log`.

## N/A

- Network, protocol frames, external service, Android device and screenshots: N/A because these are deterministic React state and query coordination tests.
- CI: N/A; repository policy forbids triggering CI without separate authorization.

## 父用例与变化

- 父任务：`TASK-20260828-006`；父用例：`WORKSPACE-LIVE-REFRESH-001`。
- 父证据：[README](../../../2026-08-28/TASK-20260828-006/WORKSPACE-LIVE-REFRESH-001/README.md)。
- 复用权威事件刷新和本地草稿保留合同；新增逐Listener三方合并、显式冲突解决与在途bootstrap事件分类。
