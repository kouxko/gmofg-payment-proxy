# Listener Draft 集成验证

TASK-20260905-002；当前Rust三库430+70+469=969/969 PASS，见rust-libraries.log。该命令在B1源码冻结、F1仅编辑前端时执行；对应Rust修改源快照见[后端证据](../backend/README.md)。

```sh
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application -p intercept-proxy-domain -p intercept-proxy-infrastructure --lib
```

前端最终测试/构建及独立整体审查尚待F1实现结束；不表示整任务完成。完整状态和命令见metadata.json。


## Compatibility and archived source checks

- `compatibility-ast.json` is produced by `deno run -A docs/testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/integrated/check-compatibility.mjs`. It parses generated commands using TypeScript AST and compares exact declarations against baseline c7a70d3e: 111 old commands unchanged, only the three approved Draft commands added. Protected domain, Display 1 MiB, package-runtime and SQLite paths remain unchanged. This is static compatibility evidence, not native IPC/runtime acceptance.
- `backend-snapshot-verification.json` is produced by `python3 docs/testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/integrated/check-backend-snapshots.py`. All 12 actual B1 source snapshots match their original digests and current repository origins. The original SHA256SUMS.txt incorrectly included its own empty-file digest; the original is preserved and this independent inventory replaces that invalid self-entry for integrity verification. No archived source was modified.
- [Configuration boundary RED/GREEN](config-boundary/README.md) proves the narrow resources exclusion using real TypeScript configuration parsing and ESLint APIs. Current implementation, active test and Vitest config remain checked.
- `architecture-docs.log` and `rust-fmt.log`: exit 0. Frontend remains in progress; these checks do not replace final frontend validation or independent overall review.


## Final source freeze and integrated checks

B1 and F1 both reached terminal status and the leader collected their results before integration. Final Next production build passed (`next-build.log`), including TypeScript and all 13 static pages. The full architecture suite passed (`architecture-final.log`). `source-size-final.log` is an expected **FAIL**, with six existing unrelated files still above 500 lines; no new oversized file was introduced, and the edited Listener test was split below the limit. This is not a globally green source-size gate.

F1 recorded 536 full frontend tests at its formal execution state, followed by 72 affected regressions after lifecycle/test extraction changes and 39 after the final control assertion. Typecheck and lint passed. See [F1 execution windows](../frontend/README.md); those windows are not collapsed into a fabricated single run. The final 37 F1 snapshots were checked against their digests and byte-compared to current source after the production build (`frontend-snapshot-verification.json`). Rust source remained frozen since B1; its 969-library-test result is reused without redundant rerun.

Independent overall review was started only after both implementation lanes were terminal and these integrated results had been read. Review is pending; this case is not yet fully accepted.


## Final acceptance

Independent round 2 is **APPROVE**: the only P2 was corrected and no P0/P1/in-scope P2 remains. See [review report](independent-review.md) and [repair RED/GREEN](../backend-review-fix/README.md). Final repair verification: Draft 13/13, existing validation 2/2, command binding 1/1, Application strict Clippy/fmt/diff PASS. The original 969-library run remains the pre-repair baseline; no fictitious full rerun is claimed. The frontend/public interface did not change during this repair, so their passing checks/build remain applicable.

Run `python3 docs/testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/integrated/check-final-source-state.py` for the final 58-resource integrity comparison. It uses the original backend mapping with precisely two explicit review-fix snapshots, verifies the before/after chain, then checks the final frontend and documentation/configuration resources. The earlier `check-backend-snapshots.py` is retained as a historical check of the pre-repair B1 state; use the final checker on the completed implementation. The actual document/tool inputs are listed in `integration-resources.json`.

Result: PASS for the accepted task scope, with **six pre-existing unrelated source-size failures and native desktop UI NOT_RUN**. No device installation, CI, push, release or commit was executed. Task001 #3 recovery behavior remains outside this task and awaits a product choice.
