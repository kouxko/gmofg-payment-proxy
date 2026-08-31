import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const checker = resolve(import.meta.dirname, "check-task-20260829-002-phase15-ui.mjs");
const repo = resolve(import.meta.dirname, "..");
const files = [
  "src/features/rules/rules-view.tsx",
  "src/features/rules/rule-definition-editor.tsx",
  "src/features/rules/rule-document-fields.ts",
  "src/features/rules/rule-definition-model.ts",
  "src/features/rules/rule-tree-editors.tsx",
  "src/features/capture/exchange-observation-detail.tsx",
  "src/features/protocol-packages/protocol-package-detail.tsx",
  "src-tauri/crates/application/src/models/unified_rule.rs",
  "src-tauri/crates/application/src/facade/unified_rule_editor/document_factory.rs",
  "src-tauri/crates/exchange/src/observation.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs",
  "src-tauri/crates/domain/src/rule/engine.rs",
  "src-tauri/crates/proxy/src/socket_relay/processing.rs",
  "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/programs.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/document_rules.rs",
  "src-tauri/src/runtime_logs/exchange_ui_layer.rs",
  "src/generated/rust-types.ts",
];

function fixture(mutator) {
  const root = mkdtempSync(join(tmpdir(), "phase15-ui-"));
  for (const relative of files) cpSync(join(repo, relative), join(root, relative), { recursive: true });
  mutator?.(root);
  return root;
}

function run(root) {
  return spawnSync(process.execPath, [checker, root], { cwd: root, encoding: "utf8" });
}

test("current source satisfies the complete Phase15 UI contract", () => {
  const result = run(repo);
  assert.equal(result.status, 0, result.stderr);
});

test("checker rejects legacy Document runtime projection re-entry", () => {
  for (const [relative, token] of [
    ["src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/programs.rs", "workspace.document_runtime_rules()"],
    ["src-tauri/crates/infrastructure/src/adapters/listener_runtime/document_rules.rs", ".document_runtime_rules()?"],
  ]) {
    const root = fixture((directory) => {
      const path = join(directory, relative);
      writeFileSync(path, `${readFileSync(path, "utf8")}\n// ${token}\n`);
    });
    try {
      assert.notEqual(run(root).status, 0, token);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects loss of actor-owned unified Nth contract", () => {
  for (const [relative, token] of [
    ["src-tauri/crates/domain/src/rule/engine.rs", "evaluate_with_condition_gate_in_order"],
    ["src-tauri/crates/domain/src/rule/engine.rs", "RuleConditionEvaluation::NotOwned"],
    ["src-tauri/crates/proxy/src/socket_relay/processing.rs", "JointRuleConditionEvaluation"],
    ["src-tauri/crates/proxy/src/socket_relay/processing.rs", "UnifiedOwned(JointConditionEvaluation)"],
    ["src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs", "JointRuleConditionEvaluation::NotOwned"],
    ["src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs", "gate(rule.id.as_uuid(), nth_attempt)"],
  ]) {
    const root = fixture((directory) => {
      const path = join(directory, relative);
      writeFileSync(path, readFileSync(path, "utf8").replaceAll(token, "REMOVED_NTH_OWNER"));
    });
    try {
      assert.notEqual(run(root).status, 0, token);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects removal of each recursive editor and modal owner", () => {
  for (const [relative, token] of [
    ["src/features/rules/rules-view.tsx", "<Modal isOpen={editorOpen}"],
    ["src/features/rules/rule-definition-editor.tsx", "<ConditionTreeEditor"],
    ["src/features/rules/rule-definition-editor.tsx", "<OrderedActionList"],
    ["src/features/rules/rule-tree-editors.tsx", "role=\"tree\""],
    ["src/features/rules/rule-tree-editors.tsx", "Array items"],
    ["src/features/rules/rule-definition-editor.tsx", "创建规则本地元数据条件"],
    ["src/features/rules/rule-definition-editor.tsx", "commands.ruleDefinitionDocumentConditionDraft"],
    ["src/features/rules/rule-definition-editor.tsx", "commands.ruleDefinitionDocumentActionDraft"],
    ["src-tauri/crates/application/src/models/unified_rule.rs", "local_document_type_capabilities"],
    ["src-tauri/crates/application/src/facade/unified_rule_editor/document_factory.rs", "condition_draft"],
    ["src-tauri/crates/application/src/facade/unified_rule_editor/document_factory.rs", "action_draft"],
    ["src-tauri/crates/application/src/facade/unified_rule_editor/document_factory.rs", "value_type: domain_value_type(value_type)"],
    ["src/features/rules/rule-document-fields.ts", "capabilities.get(type)"],
    ["src/features/rules/rule-document-fields.ts", "condition.predicate.type === \"null_equal\" ? \"null\""],
    ["src/features/rules/rule-document-fields.ts", "action.type === \"clear\" ? action.value_type"],
    ["src/features/rules/rule-definition-model.ts", "localTypes.find((item) => item.value_type === valueType)"],
    ["src/features/rules/rule-definition-model.ts", "capability?.actions.includes(mutation.type)"],
    ["src/features/rules/rule-definition-model.ts", "if (mutation.type === \"clear\") return mutation.value_type;"],
    ["src/generated/rust-types.ts", "{ type: \"clear\"; path: JsonPointer; value_type: DocumentValueType }"],
    ["src/features/rules/rules-view.tsx", "editorGeneration.current !== generation"],
  ]) {
    const root = fixture((directory) => {
      const path = join(directory, relative);
      writeFileSync(path, readFileSync(path, "utf8").replaceAll(token, "REMOVED_PHASE15_OWNER"));
    });
    try {
      assert.notEqual(run(root).status, 0, token);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects removal of stable failure and package lifecycle evidence", () => {
  for (const [relative, token] of [
    ["src/features/capture/exchange-observation-detail.tsx", "call.stable_code"],
    ["src/features/capture/exchange-observation-detail.tsx", "call.method"],
    ["src/features/capture/exchange-observation-detail.tsx", "Original Decode Document"],
    ["src/features/capture/exchange-observation-detail.tsx", "Rule processing changes"],
    ["src/features/capture/exchange-observation-detail.tsx", "Final working Document"],
    ["src/features/capture/exchange-observation-detail.tsx", "Encode result"],
    ["src/features/capture/exchange-observation-detail.tsx", "Encode / Sent result"],
    ["src-tauri/crates/exchange/src/observation.rs", "event = \"processed\""],
    ["src-tauri/crates/exchange/src/observation.rs", "RuleProcessingAccumulator"],
    ["src-tauri/crates/exchange/src/observation.rs", "MAX_OBSERVATION_TEXT_BYTES.saturating_sub"],
    ["src-tauri/crates/exchange/src/observation.rs", "changes_truncated"],
    ["src-tauri/crates/exchange/src/observation.rs", "observe_context::<P, D>(\"encoded\""],
    ["src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs", "RuleProcessingChange"],
    ["src-tauri/src/runtime_logs/exchange_ui_layer.rs", "\"processed\" =>"],
    ["src/generated/rust-types.ts", "event: \"processed\""],
    ["src/features/protocol-packages/protocol-package-detail.tsx", "external.recent_error.code"],
    ["src/features/protocol-packages/protocol-package-detail.tsx", "重启本地软件包"],
  ]) {
    const root = fixture((directory) => {
      const path = join(directory, relative);
      writeFileSync(path, readFileSync(path, "utf8").replaceAll(token, "REMOVED_PHASE15_EVIDENCE"));
    });
    try {
      assert.notEqual(run(root).status, 0, token);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});
