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
  "src/features/rules/rules-workspace-shell.tsx",
  "src/features/rules/rule-definition-list.tsx",
  "src/features/rules/rule-definition-editor.tsx",
  "src/features/rules/rule-creation-editor.tsx",
  "src/features/rules/rule-definition-model.ts",
  "src/features/rules/rule-single-pair-editor.tsx",
  "src/features/capture/exchange-observation-detail.tsx",
  "src/features/protocol-packages/protocol-package-detail.tsx",
  "src-tauri/crates/application/src/models/unified_rule.rs",
  "src-tauri/crates/application/src/facade/unified_rule_editor/document_factory.rs",
  "src-tauri/crates/application/src/facade/unified_rule_editor.rs",
  "src-tauri/crates/application/src/facade/rule_capabilities.rs",
  "src-tauri/crates/application/src/facade/protocol_package_portability.rs",
  "src-tauri/crates/application/src/facade.rs",
  "src-tauri/crates/domain/src/workspace.rs",
  "src-tauri/crates/domain/src/rule/matching.rs",
  "src-tauri/crates/proxy/src/http/contracts.rs",
  "src-tauri/crates/exchange/src/observation.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs",
  "src-tauri/crates/domain/src/unified_rule_execution.rs",
  "src-tauri/crates/domain/src/unified_rule_execution/program.rs",
  "src-tauri/crates/domain/src/unified_rule_execution/mutation.rs",
  "src-tauri/crates/proxy/src/socket_relay/processing.rs",
  "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs",
  "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor/evaluation.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/programs.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/plain_json.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/document_rules.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/runtime_rule_bundle.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/port.rs",
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
    ["src-tauri/crates/application/src/facade/protocol_package_portability.rs", "workspace.document_runtime_rules()"],
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

test("checker rejects loss of unified portability schema validation", () => {
  for (const token of [
    "workspace.rule_definitions",
    "validate_document_conditions_schema",
    "validate_unified_actions_schema",
  ]) {
    const root = fixture((directory) => {
      const path = join(directory, "src-tauri/crates/application/src/facade/protocol_package_portability.rs");
      writeFileSync(path, readFileSync(path, "utf8").replaceAll(token, "REMOVED_PORTABILITY_OWNER"));
    });
    try {
      assert.notEqual(run(root).status, 0, token);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects loss of listener runtime generation CAS", () => {
  for (const [relative, token] of [
    ["src-tauri/crates/infrastructure/src/adapters/listener_runtime/runtime_rule_bundle.rs", "enum RuntimeRuleBundleBaseline"],
    ["src-tauri/crates/infrastructure/src/adapters/listener_runtime/runtime_rule_bundle.rs", "Running(uuid::Uuid)"],
    ["src-tauri/crates/infrastructure/src/adapters/listener_runtime/port.rs", "if current != baseline"],
    ["src-tauri/crates/infrastructure/src/adapters/listener_runtime/port.rs", "RuntimeRuleBundleBaseline::Running(running.run_token)"],
  ]) {
    const root = fixture((directory) => {
      const path = join(directory, relative);
      writeFileSync(path, readFileSync(path, "utf8").replaceAll(token, "REMOVED_RUNTIME_GENERATION_CAS"));
    });
    try {
      assert.notEqual(run(root).status, 0, token);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects legacy ProtocolDocumentRule production paths", () => {
  for (const [relative, token] of [
    ["src-tauri/crates/application/src/facade.rs", "protocol_rule_save"],
    ["src-tauri/crates/domain/src/workspace.rs", "document_runtime_rules("],
    ["src-tauri/crates/domain/src/workspace.rs", "replace_document_runtime_rules("],
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

test("checker rejects loss of unified ownership and lifecycle contract", () => {
  for (const [relative, token] of [
    ["src-tauri/crates/proxy/src/socket_relay/processing.rs", "JointRuleConditionEvaluation"],
    ["src-tauri/crates/proxy/src/socket_relay/processing.rs", "UnifiedOwned(JointConditionEvaluation)"],
    ["src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs", "JointRuleConditionEvaluation::NotOwned"],
    ["src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs", "let checkpoint = current.clone()"],
    ["src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs", "commit_runtime_deltas"],
    ["src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor/evaluation.rs", "rules: &[RuleDefinition]"],
    ["src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor/evaluation.rs", "joint.gate(rule.rule_id().as_uuid())?"],
    ["src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor/evaluation.rs", "rule.lifecycle_delta_for_successful_match"],
  ]) {
    const root = fixture((directory) => {
      const path = join(directory, relative);
      writeFileSync(path, readFileSync(path, "utf8").replaceAll(token, "REMOVED_UNIFIED_OWNER"));
    });
    try {
      assert.notEqual(run(root).status, 0, token);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects retired runtime owner re-entry", () => {
  for (const [relative, token] of [
    ["src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs", "struct RuleEngine;"],
    ["src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor/evaluation.rs", "fn evaluate_with_condition_gate_in_order() {}"],
    ["src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs", "fn runtime_rules() {}"],
    ["src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor/evaluation.rs", "fn replace_runtime_rule_lifecycle() {}"],
  ]) {
    const root = fixture((directory) => {
      const path = join(directory, relative);
      writeFileSync(path, `${readFileSync(path, "utf8")}\n${token}\n`);
    });
    try {
      assert.notEqual(run(root).status, 0, token);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects restoring the intermediate rule creation step", () => {
  const root = fixture((directory) => {
    const path = join(directory, "src/features/rules/rule-creation-editor.tsx");
    writeFileSync(path, `${readFileSync(path, "utf8")}\n// 进入规则编辑器\n`);
  });
  try {
    assert.notEqual(run(root).status, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("checker rejects removal of each flat inline editor owner", () => {
  for (const [relative, token] of [
    ["src/features/rules/rules-view.tsx", "<RulesWorkspaceShell>"],
    ["src/features/rules/rules-view.tsx", "<RuleDefinitionEditor"],
    ["src/features/rules/rules-view.tsx", "<RuleCreationEditor"],
    ["src/features/rules/rules-workspace-shell.tsx", "grid-cols-[minmax(600px,1fr)_560px]"],
    ["src/features/rules/rule-definition-list.tsx", "上行与下行规则统一显示"],
    ["src/features/rules/rule-definition-list.tsx", "ruleDirectionLabel(rule.stage)"],
    ["src/features/rules/rule-definition-editor.tsx", "<RuleSinglePairEditor"],
    ["src/features/rules/rule-single-pair-editor.tsx", "async function materialize()"],
    ["src/features/rules/rule-single-pair-editor.tsx", "conditions: [condition]"],
    ["src/features/rules/rule-single-pair-editor.tsx", "actions: [action]"],
    ["src/features/rules/rule-single-pair-editor.tsx", "保存规则"],
    ["src/features/rules/rule-creation-editor.tsx", "onReadinessChange"],
    ["src-tauri/crates/domain/src/unified_rule_execution.rs", "conditions.len() != 1"],
    ["src-tauri/crates/domain/src/unified_rule_execution/program.rs", "actions.len() != 1"],
    ["src/features/rules/rule-single-pair-editor.tsx", "commands.ruleDefinitionDocumentConditionDraft"],
    ["src/features/rules/rule-single-pair-editor.tsx", "commands.ruleDefinitionDocumentActionDraft"],
    ["src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/programs.rs", "rule_definitions"],
    ["src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/plain_json.rs", "Document::parse_json"],
    ["src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/plain_json.rs", "JointDocumentEvaluation::new_plain_json"],
    ["src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/plain_json.rs", "BODY_DECODE_FAILED"],
    ["src-tauri/crates/application/src/models/unified_rule.rs", "local_document_type_capabilities"],
    ["src-tauri/crates/application/src/models/unified_rule.rs", "RuleDocumentActionCapability"],
    ["src-tauri/crates/application/src/models/unified_rule.rs", "document_schema_field_capabilities"],
    ["src-tauri/crates/application/src/models/unified_rule.rs", "target_value_type"],
    ["src-tauri/crates/application/src/models/unified_rule.rs", "operand_value_type"],
    ["src-tauri/crates/application/src/facade/unified_rule_editor/document_factory.rs", "condition_draft"],
    ["src-tauri/crates/application/src/facade/unified_rule_editor/document_factory.rs", "action_draft"],
    ["src-tauri/crates/application/src/facade/unified_rule_editor/document_factory.rs", "value_type: domain_value_type(value_type)"],
    ["src/features/rules/rule-definition-model.ts", "descriptor.kind !== mutation.type"],
    ["src/features/rules/rule-definition-model.ts", "descriptor.target_value_type"],
    ["src/features/rules/rule-definition-model.ts", "descriptor.operand_value_type"],
    ["src/generated/rust-types.ts", "export type RuleDocumentActionCapability"],
    ["src/generated/rust-types.ts", "document_fields: RuleDocumentSchemaFieldCapability[]"],
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

test("checker rejects loss or compatibility re-entry in the unified matching contract", () => {
  for (const [relative, from, to] of [
    ["src-tauri/crates/application/src/facade/rule_capabilities.rs", "match_fields", "REMOVED_MATCH_FIELDS"],
    ["src-tauri/crates/application/src/facade/unified_rule_editor.rs", "rule_definition_http_condition_draft", "REMOVED_HTTP_FACTORY"],
    ["src-tauri/crates/domain/src/rule/matching.rs", "RequestTarget 匹配缺少关联请求元数据", "REMOVED_FAIL_CLOSED_METADATA"],
    ["src-tauri/crates/proxy/src/http/contracts.rs", "uri.path_and_query()", "REMOVED_REQUEST_TARGET_OWNER"],
    ["src/features/rules/rule-definition-model.ts", "", "// PathOrRequestType compatibility\n"],
    ["src/features/rules/rule-definition-model.ts", "", "// MatchOperator::Regex compatibility\n"],
  ]) {
    const root = fixture((directory) => {
      const path = join(directory, relative);
      const source = readFileSync(path, "utf8");
      writeFileSync(path, from ? source.replaceAll(from, to) : `${source}\n${to}`);
    });
    try {
      assert.notEqual(run(root).status, 0, `${relative}: ${to}`);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects frontend variant defaults", () => {
  for (const [relative, from, to] of [
    ["src/features/rules/rule-definition-model.ts", "return unreachableContract(field);", 'return "header";'],
    ["src/features/rules/rule-definition-model.ts", "return unreachableContract(operator);", 'return "wildcard";'],
    ["src/features/rules/rule-definition-model.ts", 'if ("UpstreamConnectTimeout" in action)', "if (Object.keys(action)[0])"],
  ]) {
    const root = fixture((directory) => {
      const path = join(directory, relative);
      writeFileSync(path, readFileSync(path, "utf8").replaceAll(from, to));
    });
    try {
      assert.notEqual(run(root).status, 0, `${relative}: ${to}`);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects TLS rule wire re-entry", () => {
  for (const mutation of [
    (source) => source.replace(
      'export type RuleStage = "proxy_to_upstream" | "proxy_to_app";',
      'export type RuleStage = "proxy_to_upstream" | "proxy_to_app" | "tls_handshake";',
    ),
    (source) => `${source}\nexport type RemovedTlsRuleAction = "RejectTlsHandshake";\n`,
  ]) {
    const root = fixture((directory) => {
      const path = join(directory, "src/generated/rust-types.ts");
      writeFileSync(path, mutation(readFileSync(path, "utf8")));
    });
    try {
      assert.notEqual(run(root).status, 0);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects restoring stage-grouped rule lists", () => {
  const root = fixture((directory) => {
    const path = join(directory, "src/features/rules/rule-definition-list.tsx");
    writeFileSync(path, `${readFileSync(path, "utf8")}\n// rule-stage-heading\n`);
  });
  try {
    assert.notEqual(run(root).status, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("checker rejects schema fields falling back to schema-free capabilities", () => {
  for (const [from, to] of [
    ["selectedSchema?.predicates", "undefined"],
    ["selectedDocumentActions", "missingDocumentActions"],
  ]) {
    const root = fixture((directory) => {
      const path = join(directory, "src/features/rules/rule-single-pair-editor.tsx");
      writeFileSync(path, readFileSync(path, "utf8").replaceAll(from, to));
    });
    try {
      assert.notEqual(run(root).status, 0, `${from}: ${to}`);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});
