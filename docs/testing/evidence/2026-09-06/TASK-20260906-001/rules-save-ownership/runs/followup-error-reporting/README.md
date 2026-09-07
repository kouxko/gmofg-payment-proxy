# Rules follow-up: stale save error reporting and cancel behavior

- `red/` records the first UI-text assertion against the original guarded toast. It failed because the toast provider is not mounted by this component test.
- `red-assertion/` records the robust boundary assertion with the toast function mocked. Against the original guarded toast it failed with zero notification calls.
- `green/` is the preserved intermediate run requested by the task; it still contains the pre-correction failed result.
- `green-final/` is authoritative for this follow-up. It contains exact final source/config snapshots, timestamps, and command logs.
- Final behavior: stale save failures always notify the user; field errors remain scoped to the editor generation that initiated the save; the newer selection remains unchanged.
- Original Cancel behavior is restored. Cancelling during pre-commit materialization unmounts the editor and prevents the abandoned draft from reaching `onSave` after materialization completes.
- Final focused validation: 4 rule test files, 43 tests passed; `deno task typecheck` passed; `deno task lint --max-warnings 0` passed; `git diff --check -- src/features/rules` passed.
