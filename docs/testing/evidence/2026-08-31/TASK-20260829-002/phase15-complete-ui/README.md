# phase15-complete-ui

- 任务：`TASK-20260829-002`
- 用例：`phase15-complete-ui`
- 父用例：`phase14-final-schema100`
- 目的：完成统一规则全屏 modal、递归 metadata/AND-OR 条件树、有序统一 actions、package lifecycle 与 Capture/Session stable error 展示；只消费既有后端合同，不增加默认、兼容、重试或 fallback。
- 被测状态：分支 `codex/task-20260829-002`，基线 HEAD `938eb591d8d2701b32d491cac4558e1cfdb1cda9`；工作树含本阶段修改及不属于本阶段的用户 `docs/README.md` 修改，后者未编辑、未纳入本阶段差异。
- 执行环境：macOS arm64，Asia/Shanghai；Vitest/jsdom、TypeScript、ESLint、Node checker 与 Rust workspace。

## RED

1. focused Vitest：exit 1，25 tests 中 21 PASS / 4 FAIL；分别缺少规则 modal、递归 metadata/AND-OR UI、有序 action reorder 和 Capture stable error code/method。
2. Phase15 checker test：exit 1；当前源码检查因 checker module 尚不存在而失败。

## GREEN / 实际结果

| 检查 | 结果 |
| --- | --- |
| Phase15 checker mutation + current source | PASS；Node 5/5，锁定 modal/tree/action owner、package lifecycle、stable error 展示、HTTP/Socket unified runtime owner 与 actor-owned Nth attempt，不允许恢复扁平/分离 owner、legacy Document runtime projection 或 joint hard-coded attempt |
| Phase15 focused | PASS；初始 4 files / 29 tests；第五次 repair 后 affected 5 files / 45 tests |
| TypeScript / lint | PASS；`pnpm typecheck`、`pnpm lint` |
| 静态边界 | PASS；architecture、source-size、`git diff --check` |
| Rust affected | PASS；application typed capability/factory、Tauri process parser targeted 各 1/1；4 packages `cargo check`；4 packages all-target/all-feature strict Clippy |
| 历史全仓 checkpoint session16797 | PASS，exit 0；Phase1、bindings fresh/deterministic、architecture、source-size、lint、typecheck、前端 65 files / 550 tests、Rust fmt、workspace strict Clippy、workspace all-target/all-feature tests 全部通过；该 session 早于后续八轮 repair，不能代表当前全仓 checkpoint |

Review repair 的结构化/终端摘要见 `outputs/review-repair-red.txt`、`outputs/review-repair-green.txt`；基线、worktree delta 与关键生产源码 SHA-256 见 `outputs/source-hashes.txt`。

## 合同覆盖

- metadata tree 递归渲染 Object/Array，Array 只显示实际数值索引；Schema 节点只读，规则本地 metadata 节点可编辑，并显示每个节点的条件计数。
- Review repair 后 Schema Array 的 `items` 只作为只读模板显示；只有 Document leaf 明确携带具体 index 时才在 rule-local tree 显示 `Array index N`。Schema-free 空规则可显式输入 RFC6901 path、选择 type 和输入 JSON value，经 Rust typed parser 后创建首个 Document condition leaf；metadata 仍随 leaf 持久化，没有新增独立字段。
- condition tree 递归渲染 AND/OR，允许切换组合操作符、包裹和删除但不产生空组；仍直接编辑唯一 `ConditionTree`。
- actions 使用唯一有序列表，上移/下移/删除保持完整 payload；没有额外动作默认或兼容路径。
- Rules list 与 editor 统一在全屏 modal 中；package 状态继续显示既有 source/enabled/connection/error lifecycle 字段。
- Capture external-package failure 展示稳定 code、method、package、remote message 与有界 data，不把失败显示为成功。
- Capture/Session 直接渲染 typed `received.document`、逐规则 `processed.changes[].matched/operations`、`processed.final_document`、`processed.changes_truncated`、`encoded.context` 与 `sent.context`；Runtime 在规则链完成后发布有界 typed operation summary 与最终 working Document，Exchange 在 Encode 成功后发布 encoded context。规则选择使用 generation guard，关闭或新选择会使旧请求失效，旧结果不能重新打开 modal。
- 第三次 repair 后既有 rule-local field 也只按 `context.local_document_types[value_type]` 的 predicates/actions 渲染，包含 `null`，不再回填 UI 字面默认。逐规则观测改为有界 typed operation summary；直接复用 16MiB 观测上限并按真实 serialized bytes 计账，最终 Document 预留由发布时实际长度决定，超限以 `changes_truncated=true` 展示且不影响规则处理或 Encode。
- 第四次 repair 后 stage compatibility 同样按 path 的 schema 或 rule-local `value_type` 消费 Rust `local_document_types[].actions`，不再把 `set`/`clear` 映射为前端字面能力；undeclared local path 的 `insert`/`append` 由 Rust typed factory 创建后可保存，并保持完整 action payload 持久化。
- 第五次 repair 后 `DocumentMutation::Clear` 与旧 runtime 投影均显式携带 Rust `DocumentValueType`；Rust factory 保留用户选择的类型，generated wire 为 `{type:"clear",path,value_type}`。Schema-free 首个 Clear-only leaf 因而可从 action 自身恢复 rule-local metadata/capability 并保存；旧缺少 `value_type` 的 wire 不提供 alias 或 migration，按 strict serde fail-closed。
- 第六次 repair 后 HTTP 与 Socket listener compiler 直接从权威 `RuleDefinition` 构建 `UnifiedRuleProgram`；Document-bound actor skeleton 只保留生命周期和非 Document HTTP action，递归 `ConditionTree` 与 `UnifiedAction::Document` 由 joint working Document 顺序执行，不再经过 `document_runtime_rules()`、legacy condition 或 `ProtocolDocumentOperation` production projection。真实 HTTP actor 用例覆盖 Document OR、Set/Insert/Append、前序 working Document、成功 lifecycle commit 与 Encode 失败 rollback；真实 Socket joint 用例覆盖 Decode 后 OR、Set/Insert/Append 与 Encode 输入。过程证据新增 typed `insert`/`append` operation kind 并贯穿 Application/generated；checker mutation 阻止 legacy compiler 重新接入。
- 第七次 repair 后 Phase6 actor 继续作为 Nth counter、hit 与 one-shot lifecycle 的唯一 owner；HTTP 与 Socket joint 通过共享 typed gate 接收 actor 为当前 transaction 推导的 `nth_attempt`，返回 `matched/eligible_without_nth/contains_nth`。Encode 失败由既有 actor checkpoint 恢复 counter 与 lifecycle，成功后才单次提交；不恢复 legacy projection 或第二套 counter。真实 HTTP production actor 覆盖 NthHit(2) 首次 miss、第二次命中和 one-shot，真实 Socket relay 覆盖 NthHit(2)、Encode 失败不消费 attempt、retry 仍命中及双写阶段生命周期；checker mutation 拒绝 hard-coded attempt 重新接入。
- 第八次 repair 后 joint gate 明确返回 `UnifiedOwned(JointConditionEvaluation)` 或 `NotOwned`。只有存在 `UnifiedRuleProgram` 的 Document rule 使用 typed unified evaluation；同 listener/stage 中 `document=None` 的普通 HTTP rule 在 `NotOwned` 分支继续由 actor 的既有 HTTP 条件与 Nth counter 匹配，普通 action/hit/lifecycle 不再被 missing-program 的伪 `matched=true` 绕过。真实 external HTTP mixed 用例连续两次请求证明 ordinary false 始终不执行且 hit=0、ordinary true 正常执行且 hit=2、ordinary NthHit(2) 仅第二次执行且 hit=1；既有 Unified Nth/one-shot 与 Socket Encode rollback/retry 均保持通过。

## N/A / NOT_RUN

- 真实 HTTP/Socket Server/App 字节链：`NOT_RUN`；本次用 typed Rust event/parser 与 jsdom UI 回归验证跨层合同，真实外部链路需可用 listener/package 环境。
- macOS 真实 App 人工交互、截图、VoiceOver、系统权限弹窗：`NOT_RUN`，需要人工桌面环境；自动化覆盖 dialog/tree/list 语义、键盘可达按钮和 stable error 文本。
- Windows runner、installer、push、远程 CI、Release、部署：`NOT_RUN`，未授权或需要外部环境。
- Phase16 文档同步与 Phase17 reset 删除：`NOT_RUN`，未提前执行。

## 复测

```bash
pnpm test:task-20260829-002:phase15
pnpm typecheck
pnpm lint
pnpm scan:architecture
pnpm scan:source-size
git diff --check
pnpm check:task-20260829-002:checkpoint
```

## 结果

`VERIFIED / APPROVED / CODE CHECKPOINT READY / GLOBAL CHECKPOINT INCOMPLETE / FULL CHECKPOINT ARTIFACT PARTIAL`。最终 Reviewer `APPROVE`、Verifier `VERIFIED`，P0/P1/P2=`0/0/0`，`code_checkpoint_ready=true`。第八次 repair 后 targeted/affected/static fresh PASS；当前源码未重跑 full workspace，故 `global_checkpoint_complete=false`。历史 session16797 exit0 早于后续 repairs，仅保留历史门结果；其 raw stdout/stderr 未落盘且不可恢复，`full_checkpoint_artifact_complete=false`，不伪造补写。人工 macOS App/截图/VoiceOver、系统权限、真实外部链路与 Windows 仍为 `NOT_RUN`。
