import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = process.argv[2] ? resolve(process.argv[2]) : resolve(import.meta.dirname, "..");
const sources = new Map([
  ["rules-view", "src/features/rules/rules-view.tsx"],
  ["rule-editor", "src/features/rules/rule-definition-editor.tsx"],
  ["rule-document-fields", "src/features/rules/rule-document-fields.ts"],
  ["rule-model", "src/features/rules/rule-definition-model.ts"],
  ["tree-editors", "src/features/rules/rule-tree-editors.tsx"],
  ["capture-detail", "src/features/capture/exchange-observation-detail.tsx"],
  ["package-detail", "src/features/protocol-packages/protocol-package-detail.tsx"],
  ["application-rule", "src-tauri/crates/application/src/models/unified_rule.rs"],
  ["application-factory", "src-tauri/crates/application/src/facade/unified_rule_editor/document_factory.rs"],
  ["exchange-observation", "src-tauri/crates/exchange/src/observation.rs"],
  ["joint-runtime", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs"],
  ["domain-rule-engine", "src-tauri/crates/domain/src/rule/engine.rs"],
  ["runtime-contract", "src-tauri/crates/proxy/src/socket_relay/processing.rs"],
  ["runtime-actor", "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs"],
  ["http-rule-runtime", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/programs.rs"],
  ["socket-rule-runtime", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/document_rules.rs"],
  ["tauri-observation", "src-tauri/src/runtime_logs/exchange_ui_layer.rs"],
  ["generated", "src/generated/rust-types.ts"],
].map(([name, relative]) => [name, readFileSync(resolve(root, relative), "utf8")]));

const requirements = [
  ["rules-view", "<Modal isOpen={editorOpen}", "unified rule modal"],
  ["rules-view", "aria-label=\"关闭规则编辑器\"", "modal keyboard close control"],
  ["rule-editor", "<DocumentMetadataTree", "recursive Document metadata consumer"],
  ["rule-editor", "<ConditionTreeEditor", "recursive AND/OR condition editor"],
  ["rule-editor", "<OrderedActionList", "ordered unified action editor"],
  ["tree-editors", "role=\"tree\"", "accessible metadata tree"],
  ["tree-editors", "Array index", "concrete array-index presentation"],
  ["tree-editors", "Array items", "Schema array items template presentation"],
  ["tree-editors", "AND 条件组", "AND group presentation"],
  ["tree-editors", "OR 条件组", "OR group presentation"],
  ["tree-editors", "下移动作", "action reorder control"],
  ["capture-detail", "call.stable_code", "stable failure code"],
  ["capture-detail", "call.method", "failed package method"],
  ["capture-detail", "Original Decode Document", "received Document evidence"],
  ["capture-detail", "Rule processing changes", "typed per-rule process evidence"],
  ["capture-detail", "Final working Document", "typed final working Document evidence"],
  ["capture-detail", "Encode result", "typed Encode evidence"],
  ["capture-detail", "Encode / Sent result", "Encode and sent result evidence"],
  ["rule-editor", "创建规则本地元数据条件", "Schema-free local metadata creation"],
  ["rule-editor", "commands.ruleDefinitionDocumentConditionDraft", "Rust-owned local condition factory"],
  ["rule-editor", "commands.ruleDefinitionDocumentActionDraft", "Rust-owned local action factory"],
  ["application-rule", "local_document_type_capabilities", "Rust-owned schema-free capability catalog"],
  ["application-rule", "RuleLocalDocumentValueType", "all Document value types"],
  ["application-factory", "condition_draft", "typed condition leaf factory"],
  ["application-factory", "action_draft", "typed action leaf factory"],
  ["application-factory", "value_type: domain_value_type(value_type)", "typed Clear factory metadata"],
  ["rule-document-fields", "capabilities.get(type)", "Rust-owned capability lookup for existing local fields"],
  ["rule-document-fields", "condition.predicate.type === \"null_equal\" ? \"null\"", "existing null local field support"],
  ["rule-document-fields", "action.type === \"clear\" ? action.value_type", "typed Clear metadata leaf"],
  ["rule-model", "localTypes.find((item) => item.value_type === valueType)", "stage action capability lookup by value type"],
  ["rule-model", "capability?.actions.includes(mutation.type)", "Rust-owned stage action compatibility"],
  ["rule-model", "if (mutation.type === \"clear\") return mutation.value_type;", "typed Clear stage compatibility"],
  ["exchange-observation", "RuleProcessingAccumulator", "bounded process evidence accumulator"],
  ["exchange-observation", "MAX_OBSERVATION_TEXT_BYTES.saturating_sub", "shared observation serialization budget"],
  ["exchange-observation", "changes_truncated", "typed process evidence truncation"],
  ["exchange-observation", "event = \"processed\"", "Exchange processed event"],
  ["exchange-observation", "observe_context::<P, D>(\"encoded\"", "Exchange encoded event"],
  ["joint-runtime", "RuleProcessingChange", "runtime per-rule changes"],
  ["joint-runtime", "nth_attempt", "actor-owned Nth attempt consumer"],
  ["domain-rule-engine", "evaluate_with_condition_gate_in_order", "actor-owned unified condition gate"],
  ["domain-rule-engine", "RuleConditionEvaluation::NotOwned", "ordinary rule fallback to actor matching"],
  ["runtime-contract", "JointRuleConditionEvaluation", "shared typed joint condition evaluation"],
  ["runtime-contract", "UnifiedOwned(JointConditionEvaluation)", "explicit unified-owned gate result"],
  ["runtime-contract", "NotOwned", "explicit ordinary-rule gate result"],
  ["runtime-contract", "nth_attempt: u64", "shared actor-owned Nth attempt contract"],
  ["runtime-actor", "evaluate_with_condition_gate_in_order", "active actor typed condition gate"],
  ["runtime-actor", "gate(rule.id.as_uuid(), nth_attempt)", "Socket Nth attempt forwarding"],
  ["joint-runtime", "JointRuleConditionEvaluation::NotOwned", "joint ownership miss result"],
  ["http-rule-runtime", "UnifiedRuleProgram", "HTTP unified runtime program"],
  ["http-rule-runtime", "workspace.rule_definitions", "HTTP authoritative unified rules"],
  ["socket-rule-runtime", "UnifiedRuleProgram", "Socket unified runtime program"],
  ["socket-rule-runtime", "workspace.rule_definitions", "Socket authoritative unified rules"],
  ["tauri-observation", "\"processed\" =>", "Tauri processed parser"],
  ["generated", "event: \"processed\"", "generated processed event"],
  ["generated", "{ type: \"clear\"; path: JsonPointer; value_type: DocumentValueType }", "generated typed Clear wire"],
  ["rules-view", "editorGeneration.current !== generation", "stale editor request generation guard"],
  ["package-detail", "external.recent_error.code", "package lifecycle stable code"],
  ["package-detail", "重启本地软件包", "local package restart control"],
];

const failures = [];
for (const [owner, token, label] of requirements) {
  if (!sources.get(owner).includes(token)) failures.push(`${owner}: missing ${label}`);
}
if (sources.get("rules-view").includes("RulesWorkspaceShell")) {
  failures.push("rules-view: fixed side editor remains");
}
if (sources.get("joint-runtime").includes("document.clone()")) {
  failures.push("joint-runtime: per-rule full Document clone remains");
}
if (sources.get("joint-runtime").includes("&mut self.document, 1,")) {
  failures.push("joint-runtime: hard-coded Nth attempt remains");
}
for (const owner of ["http-rule-runtime", "socket-rule-runtime"]) {
  for (const legacy of ["workspace.document_runtime_rules()", ".document_runtime_rules()?"]) {
    if (sources.get(owner).includes(legacy)) {
      failures.push(`${owner}: legacy Document runtime projection remains: ${legacy}`);
    }
  }
}
for (const invented of ['?? ["equals"]', '?? ["set_field", "clear_field"]']) {
  if (sources.get("rule-document-fields").includes(invented)) {
    failures.push(`rule-document-fields: invented fallback remains: ${invented}`);
  }
}
if (sources.get("rule-model").includes('mutation.type === "set" ? "set_field"')) {
  failures.push("rule-model: legacy set/clear action mapping remains");
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("phase15 complete UI contract: PASS");
