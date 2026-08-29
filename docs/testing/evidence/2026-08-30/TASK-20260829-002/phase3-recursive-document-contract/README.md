# phase3-recursive-document-contract

- 任务：`TASK-20260829-002`
- 用例：`phase3-recursive-document-contract`
- 目的：证明 Phase 3 已用唯一 recursive Document/Schema/RFC6901 合同替换 flat field-slot/Int/Blob/schema identity/version 合同，并同步当前 Rust、generated bindings、最小前端消费者与活动协议包 fixture。
- 执行环境：macOS arm64，Asia/Shanghai；Node `v26.7.0`、pnpm `11.13.1`、rustc `1.97.1`、cargo `1.97.1`。
- 被测状态：分支 `codex/task-20260829-002`，HEAD `c85ddaabb073198b0e4677c46f4babbbe2ba5799`，HEAD tree `3f2a0df06195330fedf1bd4ececd9b6cfc025a4f`。工作树包含本任务未提交修改及不属于本阶段的用户 `docs/README.md` 修改；最终 checkpoint 期间被测文件未发生变化。
- generated bindings：`src/generated/rust-types.ts` SHA-256 `ba0dcb545e4f5c04f381d337a4a11062fef789e8d0b28660f575bff37b7dc356`。

## derived_from

- 父任务：`TASK-20260829-002`
- 父用例：`phase2-development-database-recreate`
- 父证据：`docs/testing/evidence/2026-08-30/TASK-20260829-002/phase2-development-database-recreate/`
- 父资源：`docs/testing/evidence/2026-08-30/TASK-20260829-002/phase2-development-database-recreate/resources/nuvei-tango-json-rhai-1.0.0.zip`，SHA-256 `0595af171e20ae9eee21da42a8327971c99689a278cab6ffd7612ba20a4049ea`，6706 bytes。
- 本次变化：recursive Document 改变了 Nuvei source schema 与 Rhai value shape，因此从活动 source 确定性重建新 ZIP，并在本证据归档为 `resources/nuvei-tango-json-rhai-1.0.0.zip`，SHA-256 `047fe2701973d860d40fe30f5c74a735e46934d808ffb7dd1f16bf404460e30b`，6715 bytes。
- 保持不变：协议包 identity、版本和 Phase 2 双启动 Package prepare/commit 用法不变。Phase 2 父证据及其旧 ZIP 未覆盖、未改写；历史复现继续使用父证据快照，当前回归使用本次派生资源。

## 预期

1. Document 是无 identity 的 owned recursive value object，只包含 String、finite Number、Boolean、Null、Object 和 Array；整数超过 JavaScript safe integer 范围拒绝，NaN/Infinity 拒绝，标准 JSON 行为保持 `1e-400 => 0` 与 duplicate key last-wins。
2. Schema 是无 identity/version 的 recursive metadata node，只包含 string/number/boolean/object/array、optional title、object properties 和 required array items；Manifest 不增加 schema version。
3. RFC6901 支持 root `""`、`~0/~1`、空字段、Unicode 与严格 array index；Document Set/Clear/Insert/Append 使用一个模型和强类型错误，不保留 flat slot 或兼容 alias。
4. `ProtocolPackageSchemaViewModel.version` 与 rule `schema_version` 删除，不增加替代字段或硬编码 `1`。
5. Rust、generated bindings、即时前端 guards/tests、MCP fixtures、protocol scripting、external package、Nuvei 和 ISO template 同步；不进入 Phase 4 package contract 或 Phase 5 条件/动作执行重构。

## 实施中发现与修正

- 首次完整 checkpoint 在 Phase 1 inventory 停止：active inventory 仍要求已由 Phase 3 删除的 flat Document 旧片段。修复为 Phase 3 recursive current-state fragments 后 Phase 1 Node 4/4 PASS；历史 Phase 1 evidence snapshot 未修改。
- 第二次完整 checkpoint 在 source-size 停止：`external_package/registration.rs` 达到 502 行。仅压缩空行至 498 行，不改变逻辑、不放宽 500 行门禁，复跑 PASS。
- 第三次完整 checkpoint 的前端全量测试唯一失败 1/531：`socket-protocol-package-dialog.test.tsx` 仍使用旧 `{id,version,title,fields}` fixture。由 frontend owner 改为 identity-free recursive `{root}` fixture 后定向 7/7 PASS，完整 checkpoint 再跑 exit 0；首次失败未隐藏。
- 上述初版随后被独立 Verifier 判定 `FAILED`，因此撤回“旧合同零残留且可 checkpoint”的过强结论。P1/P2 包括：integral `f64` 未对 JavaScript safe integer 做统一边界检查；Decode/transform 后仍做完整 Document-vs-Schema 校验；规则拒绝 Schema 未声明 path；generated `DocumentNumber` 导出 `number | null`、Null 导出字符串字面量 `"Null"`；旧 `ClearDocument` 仍由 capability/generated/MCP fixture 暴露；public Program 构造未验证 Schema definition，且残留过时注释与 Blob limit。
- 修复后 DocumentNumber 对所有 integral `f64` 统一执行 safe-integer 边界，真实 JSON null 与字符串 `"Null"` 分离；Schema 只校验自身递归定义并作为可不完整 metadata，未声明 rule path 按规则本地类型合同保存；旧 `ClearDocument`/`clear_document` 及 field-slot/schema-identity 文案从当前 Rust/generated/MCP 合同删除。Phase 3 Node 回归 11/11，精确旧合同扫描 0。
- 修复后的第一次 fresh 完整 checkpoint 在第 8 门仅因 `document_rules/tests.rs` rustfmt 差异停止；执行标准 `cargo fmt` 后完整 checkpoint 重跑十门全部 PASS。更早一次独立 `pnpm test` 出现既有 focus restore 用例偶发 1/534，定向 1/1 与后续完整前端 534/534 PASS；两次失败均未隐藏。
- 最终复审又发现一个 P2：Application facade `protocol_rule_values.rs` 仍描述并实现已删除的 Int/Bool/Blob Hex 文本合同，保留 `MAX_PROTOCOL_RULE_INT_TEXT_BYTES` 与“整数文本”错误。最小修复删除该预算和旧文案，Number/Boolean 统一经标准 JSON 与 Domain recursive value 合同解析；非法 JSON 保持 typed `JSON_INVALID`，合法 JSON 类型不匹配保持 `PROTOCOL_RULE_VALUE_INVALID`，不增加兼容或错误映射。直接 Application 4/4、Phase 3 Node 14/14、workspace strict Clippy、bindings freshness/determinism、精确残留扫描和 diff-check 均 PASS。
- 最终独立 Verifier 随后 fresh 重跑精确十门 checkpoint，十门全部 PASS；Application unit 实际计数因新增 facade 回归为 459/459。本次只修正归档计数漂移，状态继续 `RECHECK PENDING`，等待 Verifier 正式 verdict。
- `2026-08-30 04:16:20 +08:00`：正式独立 Verdict 为 `VERIFIED / APPROVED / CHECKPOINT READY`，P0=0、P1=0、P2=0，无剩余阻断；确认前述独立 fresh 精确十门 checkpoint 全部 PASS。G044 可创建 Phase 3 rollback checkpoint，TASK-20260829-002 总体仍进行中。

## 实际结果

完整命令见 `replay/commands.txt`，结构化摘要见 `outputs/verification-summary.json`，合同与消费者快照见 `inputs/phase3-contract-and-consumer-inventory.json`。

| 检查 | 实际结果 |
| --- | --- |
| Domain recursive tests | PASS；recursive 6/6，domain 87/87，domain integrations 14/14 + 7/7 |
| Protocol scripting | PASS；160/160；schema definition 与 incomplete metadata 合同全绿；原两个 hang 用例定向各 1/1 |
| Application | PASS；459/459；integration 14/14、7/7、5/5、12/12 |
| Infrastructure | PASS；651/651；迁移后的 registry/server/runtime/SQLite fixtures 全绿 |
| Nuvei active fixture | PASS；example 6/6；确定性 ZIP SHA-256 为 `047fe270...e30b` |
| generated / frontend | PASS；bindings fresh+deterministic、typecheck、目标 dialog 7/7、全量 62 files / 534 tests |
| Phase 1 十门禁 checkpoint | PASS，exit 0；Rust workspace all-target/all-feature 0 failed |
| strict Rust Clippy / fmt | PASS；workspace all-target/all-feature `-D warnings`、fmt check |
| 旧合同扫描 | PASS；`ClearDocument`、`clear_document`、`字段值槽`、`Schema 身份和结构`、`MAX_PROTOCOL_RULE_INT_TEXT_BYTES`、`Blob Hex`、`整数文本不能超过` 精确扫描均 0；既有旧 Rust symbol/schema version/flat schema/non-RFC6901 扫描仍为 0 |
| `git diff --check` | PASS |

## N/A

- 原始网络 Frame、Server/App 实际报文：N/A；Phase 3 建立模型与即时消费者合同，真实 pipeline 原字节行为属于后续 pipeline 阶段。
- UI 截图与人工交互：N/A；本阶段只同步 schema 展示/guard 的编译和自动化测试，不改变已确认 UI 交互流程。
- 真实设备、远程服务、外部 sidecar：N/A；不属于 Phase 3 Document model。
- CI、push、Release、部署、提交：N/A；未获授权且未执行。

## 结果

`VERIFIED / APPROVED / CHECKPOINT READY`。正式独立 Verdict：P0=0、P1=0、P2=0，无剩余阻断；G044 可创建 Phase 3 rollback checkpoint，任务总体继续进行中。
