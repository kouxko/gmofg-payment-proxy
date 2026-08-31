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
  ["application-editor", "src-tauri/crates/application/src/facade/unified_rule_editor.rs"],
  ["application-capabilities", "src-tauri/crates/application/src/facade/rule_capabilities.rs"],
  ["application-portability", "src-tauri/crates/application/src/facade/protocol_package_portability.rs"],
  ["application-facade", "src-tauri/crates/application/src/facade.rs"],
  ["domain-workspace", "src-tauri/crates/domain/src/workspace.rs"],
  ["domain-matching", "src-tauri/crates/domain/src/rule/matching.rs"],
  ["http-metadata", "src-tauri/crates/proxy/src/http/contracts.rs"],
  ["exchange-observation", "src-tauri/crates/exchange/src/observation.rs"],
  ["joint-runtime", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs"],
  ["unified-execution", "src-tauri/crates/domain/src/unified_rule_execution.rs"],
  ["unified-mutation", "src-tauri/crates/domain/src/unified_rule_execution/mutation.rs"],
  ["runtime-contract", "src-tauri/crates/proxy/src/socket_relay/processing.rs"],
  ["runtime-actor", "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs"],
  ["runtime-evaluation", "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor/evaluation.rs"],
  ["http-rule-runtime", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/programs.rs"],
  ["socket-rule-runtime", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/document_rules.rs"],
  ["runtime-rule-bundle", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/runtime_rule_bundle.rs"],
  ["listener-runtime-port", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/port.rs"],
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
  ["rule-editor", "commands.ruleDefinitionHttpConditionDraft", "Rust-owned HTTP condition factory"],
  ["rule-editor", "commands.ruleDefinitionNthHitConditionDraft", "Rust-owned Nth condition factory"],
  ["rule-editor", "stage.http.match_fields", "Rust-owned HTTP field capabilities"],
  ["application-editor", "rule_definition_http_condition_draft", "Rust-owned HTTP condition contract"],
  ["application-editor", "document_schema_field_capabilities", "Rust-owned Document schema capability projection"],
  ["application-capabilities", "match_fields", "Rust-owned HTTP match field catalog"],
  ["application-capabilities", "RuleMatchSelectorKind::HeaderNamePointer", "Header selector capability"],
  ["application-portability", "workspace.rule_definitions", "authoritative portable rule collection"],
  ["application-portability", "condition.validate_document_schema", "typed portable condition schema validation"],
  ["application-portability", "validate_unified_actions_schema", "typed portable action schema validation"],
  ["runtime-rule-bundle", "enum RuntimeRuleBundleBaseline", "typed stopped/running baseline owner"],
  ["runtime-rule-bundle", "Running(uuid::Uuid)", "run-token baseline identity"],
  ["listener-runtime-port", "if current != baseline", "strict runtime baseline compare before persistence"],
  ["listener-runtime-port", "RuntimeRuleBundleBaseline::Running(running.run_token)", "current run-token capture"],
  ["domain-matching", "RequestTarget 匹配缺少关联请求元数据", "fail-closed associated request metadata"],
  ["domain-matching", "wildcard_matches", "domain-owned wildcard matcher"],
  ["http-metadata", "HttpRequestMetadata", "transaction-owned request metadata"],
  ["http-metadata", "uri.path_and_query()", "raw path and query extraction"],
  ["generated", 'export type RuleMatchFieldKind = "terminal_ip" | "certificate_fingerprint" | "method" | "request_target" | "header";', "generated HTTP fields"],
  ["generated", 'export type RuleMatchOperatorKind = "equals" | "contains" | "starts_with" | "ends_with" | "wildcard";', "generated HTTP operators"],
  ["application-rule", "local_document_type_capabilities", "Rust-owned schema-free capability catalog"],
  ["application-rule", "RuleDocumentActionCapability", "typed Document action capability"],
  ["application-rule", "document_schema_field_capabilities", "Rust-owned schema field capability catalog"],
  ["application-rule", "target_value_type", "Document action target type"],
  ["application-rule", "operand_value_type", "Document action operand type"],
  ["application-rule", "RuleLocalDocumentValueType", "all Document value types"],
  ["application-factory", "condition_draft", "typed condition leaf factory"],
  ["application-factory", "action_draft", "typed action leaf factory"],
  ["application-factory", "value_type: domain_value_type(value_type)", "typed Clear factory metadata"],
  ["rule-document-fields", "capabilities.get(type)", "Rust-owned capability lookup for existing local fields"],
  ["rule-document-fields", "condition.predicate.type === \"null_equal\" ? \"null\"", "existing null local field support"],
  ["rule-document-fields", "action.type === \"clear\" ? action.value_type", "typed Clear metadata leaf"],
  ["rule-model", "descriptor.kind !== mutation.type", "stage action capability lookup by action kind"],
  ["rule-model", "descriptor.target_value_type", "Document action target compatibility"],
  ["rule-model", "descriptor.operand_value_type", "Document action operand compatibility"],
  ["rule-editor", "action.operand_value_type ?? action.target_value_type", "Rust-owned Document action input type"],
  ["generated", "export type RuleDocumentActionCapability", "generated Document action capability"],
  ["generated", "document_fields: RuleDocumentSchemaFieldCapability[]", "generated schema field capability catalog"],
  ["exchange-observation", "RuleProcessingAccumulator", "bounded process evidence accumulator"],
  ["exchange-observation", "MAX_OBSERVATION_TEXT_BYTES.saturating_sub", "shared observation serialization budget"],
  ["exchange-observation", "changes_truncated", "typed process evidence truncation"],
  ["exchange-observation", "event = \"processed\"", "Exchange processed event"],
  ["exchange-observation", "observe_context::<P, D>(\"encoded\"", "Exchange encoded event"],
  ["joint-runtime", "RuleProcessingChange", "runtime per-rule changes"],
  ["joint-runtime", "nth_attempt", "actor-owned Nth attempt consumer"],
  ["unified-execution", "schema.resolve_match_path(path)", "wildcard schema path validation"],
  ["unified-mutation", "DocumentMutation::Clear { path, value_type }", "typed Clear schema validation"],
  ["unified-mutation", "Some((items.as_ref(), value.value_type()))", "array item operand schema validation"],
  ["rule-editor", "selectedSchemaField?.predicates", "schema-owned predicate capability"],
  ["rule-editor", "selectedSchemaField?.actions", "schema-owned action capability"],
  ["rule-model", "return unreachableContract(field);", "exhaustive HTTP field mapping"],
  ["rule-model", "return unreachableContract(operator);", "exhaustive HTTP operator mapping"],
  ["rule-model", 'if ("UpstreamConnectTimeout" in action)', "exhaustive terminal action mapping"],
  ["runtime-contract", "JointRuleConditionEvaluation", "shared typed joint condition evaluation"],
  ["runtime-contract", "UnifiedOwned(JointConditionEvaluation)", "explicit unified-owned gate result"],
  ["runtime-contract", "NotOwned", "explicit ordinary-rule gate result"],
  ["runtime-contract", "nth_attempt: u64", "shared actor-owned Nth attempt contract"],
  ["runtime-actor", "let checkpoint = current.clone()", "actor lifecycle checkpoint"],
  ["runtime-actor", "current.counters.retain", "actor hot replace counter ownership"],
  ["runtime-actor", "commit_runtime_deltas", "actor lifecycle commit owner"],
  ["runtime-evaluation", "rules: &[RuleDefinition]", "active RuleDefinition evaluation owner"],
  ["runtime-evaluation", "joint.gate(rule.rule_id().as_uuid(), nth_attempt)?", "Socket typed Nth attempt forwarding"],
  ["runtime-evaluation", "current.counters.insert(key, nth_attempt)", "actor-owned Nth counter"],
  ["runtime-evaluation", "rule.lifecycle_delta_for_successful_match", "typed lifecycle delta"],
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
for (const legacy of ["workspace.document_runtime_rules()", ".document_runtime_rules()?"]) {
  if (sources.get("application-portability").includes(legacy)) {
    failures.push(`application-portability: legacy Document runtime projection remains: ${legacy}`);
  }
}
for (const [owner, forbidden] of [
  ["application-facade", /protocol_rule_(?:list|save|toggle|delete|capabilities)/],
  ["domain-workspace", /(?:replace_)?document_runtime_rules\s*\(/],
]) {
  if (forbidden.test(sources.get(owner))) {
    failures.push(`${owner}: removed legacy ProtocolDocumentRule production path remains`);
  }
}
for (const owner of ["runtime-actor", "runtime-evaluation"]) {
  if (/\bRuleEngine\b|\bevaluate_with_condition_gate_in_order\b|\bRuleConditionEvaluation\b|\bruntime_rules\s*\(|\breplace_runtime_rule_lifecycle\b/u.test(sources.get(owner))) {
    failures.push(`${owner}: retired runtime owner re-entry`);
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
for (const fallback of ['\n  return "header";', '\n  return "wildcard";', "Object.keys(action)[0]"]) {
  if (sources.get("rule-model").includes(fallback)) {
    failures.push(`rule-model: non-exhaustive variant fallback remains: ${fallback}`);
  }
}
for (const [owner, source] of sources) {
  for (const legacy of [
    "PathOrRequestType",
    "MatchField::JsonPath",
    "MatchOperator::Regex",
    "ruleDefinitionConditionDraft",
    "match_field_kinds",
  ]) {
    if (source.includes(legacy)) failures.push(`${owner}: removed compatibility contract remains: ${legacy}`);
  }
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("phase15 complete UI contract: PASS");
