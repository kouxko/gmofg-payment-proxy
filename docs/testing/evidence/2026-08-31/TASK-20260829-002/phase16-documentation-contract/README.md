# Phase16 当前文档合同证据

## 结果

`VERIFIED / APPROVED / CODE CHECKPOINT READY`。用户已明确授权对 `docs/README.md` 做精确最小修改；现已保留
该文件的其他用户修改，仅把 ADR-002/ADR-007 标为由 ADR-009 取代、加入 ADR-009，并把 current
authority 指向 ADR-009、current architecture docs 与源码。targeted、MCP embedded resource、
architecture/link/static 门均通过；最终 Reviewer `APPROVE`、Verifier `VERIFIED`，P0/P1/P2=`0/0/0`，
`code_checkpoint_ready=true`。

## 被测状态

- baseline HEAD：`02e7ea478f8b4f83d8acf9139872b17322c1559f`
- 执行时间：`2026-08-31 16:05:16 +08:00`
- 用户取消 Phase16 hash 验收；本证据不生成或声明 worktree/source hash，以明确文件范围、命令、
  exit、测试计数、链接和状态作为复验依据。
- 测试期间被测 source/docs 已冻结；任务元数据仅在测试后持锁写入。

## 文件范围

- current authority 与 ADR：`docs/README.md`、`docs/architecture/{README.md,data-flow.md,modules.md,rules-and-protocol-packages.md,runtime-observability.md,security-and-persistence.md}`、`docs/architecture/decisions/{ADR-002-protocol-packages-http.md,ADR-007-exchange-pipeline-runtime-boundary.md,ADR-009-nested-document-javascript-package-runtime.md}`、`docs/architecture/exchange-pipeline-template/README.md`。
- 用户/package/MCP 文档：`docs/{user-operation-guide.md,onboarding-guide.md}`、`templates/socket-protocol/{AUTHORING.md,API.md}`、`docs/mcp/{app-integration-guide.md,diagnostic-architecture.md,external-package-integration-guide.md,tool-reference.md,validation-playbook.md}`。
- 测试与验证文档：`docs/testing/{evidence/TEMPLATE.md,quick-validations/TEMPLATE.md,release-validation-matrix.md}`。
- checker/embedded resource：`scripts/{check-architecture-docs.mjs,check-architecture-docs.test.mjs,check-task-20260829-002-phase16-docs.mjs,check-task-20260829-002-phase16-docs.test.mjs}`、`src-tauri/src/mcp/tests.rs`、`package.json`。
- 任务/evidence 元数据另由本次 5.4 锁事务同步；`docs/tasks/pending/2026-08-31/`及其相关 Node/Deno 文件不属于 Phase16。

## RED / GREEN

- RED：`node --test scripts/check-task-20260829-002-phase16-docs.test.mjs` 初次为 `0/3`，缺失
  ADR-009、正式 evidence template 及 current Document/package/MCP/matrix 合同，并检出 Rhai/TOML 等
  active 陈旧陈述。
- GREEN（session80057，串联命令 exit `0`）：`pnpm test:task-20260829-002:phase16` 为 Node `17/17`、Phase16 checker PASS、architecture
  docs PASS；六个 MCP embedded resource exact tests 各 `1/1` PASS。
- Static：`pnpm scan:architecture`、`pnpm scan:source-size`、`pnpm lint`、Rust fmt check、
  `git diff --check` 全部 PASS。
- Mutation 覆盖 working-state 前序可见、Boa host general-sandbox过度声明必败、current MCP read-tool
  count `36`漂移到`37`必败、local `Uint8Array` 与 public canonical padded
  Base64、无 id registration、stable `error.data.code`、remote enable/disable lifecycle、MCP 36+5、typed
  capture、matrix 禁词和历史 template 退出 authority。

## Full checkpoint

`N/A`。权威任务把全层验收和完整 checkpoint 分配给 NDR-JS-12；Phase16 只验证文档、链接、embedded
resource 和 static contract。没有机械运行或伪造 full workspace 结果。

## 需求变更与复测

- 用户授权：仅修改 `docs/README.md` ADR 索引/current authority 小节，保留全部其他用户修改。
- 用户取消 hash：删除此前不可完整复算的 scope hash 声明，不生成替代 hash 或 manifest；验收改用本页
  明确范围、session90686 命令 exit、计数、链接和状态。
- fresh 复测：`pnpm test:task-20260829-002:phase16 && pnpm scan:architecture && pnpm scan:source-size && pnpm lint && cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check && git diff --check`，session80057 exit `0`。
- 六个 MCP exact 的 stdout 在session80057中逐项显示 `1 passed; 0 failed`，未单独落盘；复跑入口由
  `package.json` 的 Phase16 aggregate 固定，不伪造 raw artifact。

## NOT_RUN

- full workspace checkpoint：`N/A`，属于 NDR-JS-12。
- 人工 UI、真实 App、外部网络、CI、push、Release：`NOT_RUN`。
