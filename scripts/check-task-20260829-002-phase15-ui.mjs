import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = process.argv[2] ? resolve(process.argv[2]) : resolve(import.meta.dirname, "..");
const sources = new Map([
  ["rules-view", "src/features/rules/rules-view.tsx"],
  ["rules-workspace", "src/features/rules/rules-workspace-shell.tsx"],
  ["rule-list", "src/features/rules/rule-definition-list.tsx"],
  ["rule-editor", "src/features/rules/rule-definition-editor.tsx"],
  ["rule-creation", "src/features/rules/rule-creation-editor.tsx"],
  ["rule-model", "src/features/rules/rule-definition-model.ts"],
  ["single-pair-editor", "src/features/rules/rule-single-pair-editor.tsx"],
  ["capture-detail", "src/features/capture/exchange-observation-detail.tsx"],
  ["package-detail", "src/features/protocol-packages/protocol-package-detail.tsx"],
  ["application-rule", "src-tauri/crates/application/src/models/unified_rule.rs"],
  ["application-factory", "src-tauri/crates/application/src/facade/unified_rule_editor/document_factory.rs"],
  ["application-editor", "src-tauri/crates/application/src/facade/unified_rule_editor.rs"],
  ["application-capabilities", "src-tauri/crates/application/src/facade/rule_capabilities.rs"],
  ["application-portability", "src-tauri/crates/application/src/facade/protocol_package_portability.rs"],
  ["application-facade", "src-tauri/crates/application/src/facade.rs"],
  ["domain-workspace", "src-tauri/crates/domain/src/workspace.rs"],
  ["domain-rule", "src-tauri/crates/domain/src/unified_rule.rs"],
  ["domain-matching", "src-tauri/crates/domain/src/rule/matching.rs"],
  ["http-metadata", "src-tauri/crates/proxy/src/http/contracts.rs"],
  ["exchange-observation", "src-tauri/crates/exchange/src/observation.rs"],
  ["joint-runtime", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs"],
  ["unified-execution", "src-tauri/crates/domain/src/unified_rule_execution.rs"],
  ["unified-program", "src-tauri/crates/domain/src/unified_rule_execution/program.rs"],
  ["unified-mutation", "src-tauri/crates/domain/src/unified_rule_execution/mutation.rs"],
  ["runtime-contract", "src-tauri/crates/proxy/src/socket_relay/processing.rs"],
  ["runtime-actor", "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs"],
  ["runtime-evaluation", "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor/evaluation.rs"],
  ["http-rule-runtime", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/programs.rs"],
  ["plain-json-runtime", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/plain_json.rs"],
  ["socket-rule-runtime", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/document_rules.rs"],
  ["runtime-rule-bundle", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/runtime_rule_bundle.rs"],
  ["listener-runtime-port", "src-tauri/crates/infrastructure/src/adapters/listener_runtime/port.rs"],
  ["tauri-observation", "src-tauri/src/runtime_logs/exchange_ui_layer.rs"],
  ["generated", "src/generated/rust-types.ts"],
].map(([name, relative]) => [name, readFileSync(resolve(root, relative), "utf8")]));

const requirements = [
  ["rules-view", "<RulesWorkspaceShell>", "inline split rules workspace"],
  ["rules-view", "<RuleDefinitionEditor", "inline rule editor"],
  ["rules-view", "<RuleCreationEditor", "inline rule creation editor"],
  ["rules-workspace", "grid-cols-[minmax(600px,1fr)_560px]", "fixed inline editor column"],
  ["rule-list", "上行与下行规则统一显示", "single combined direction list"],
  ["rule-list", "ruleDirectionLabel(rule.stage)", "per-card direction badge"],
  ["rule-editor", "<RuleSinglePairEditor", "single condition/action pair editor"],
  ["single-pair-editor", "async function materialize()", "save-time factory materialization owner"],
  ["single-pair-editor", "withSinglePair(props.input, condition, action)", "single update pair owner"],
  ["single-pair-editor", "newSaveInput(props.creation, condition, action)", "single creation pair owner"],
  ["single-pair-editor", "props.onSave(materialized)", "single materialized save owner"],
  ["single-pair-editor", "保存规则", "single final save action"],
  ["rule-creation", "const ready = context != null && structure != null", "continuous creation readiness owner"],
  ["capture-detail", "call.stable_code", "stable failure code"],
  ["capture-detail", "call.method", "failed package method"],
  ["capture-detail", "Original Decode Document", "received Document evidence"],
  ["capture-detail", "Rule processing changes", "typed per-rule process evidence"],
  ["capture-detail", "Final working Document", "typed final working Document evidence"],
  ["capture-detail", "Encode result", "typed Encode evidence"],
  ["capture-detail", "Encode / Sent result", "Encode and sent result evidence"],
  ["single-pair-editor", "const manualAriaLabel = `手动 Document ${props.pathKind}路径`;", "schema-free manual path input"],
  ["single-pair-editor", "commands.ruleDefinitionDocumentConditionDraft", "Rust-owned local condition factory"],
  ["single-pair-editor", "commands.ruleDefinitionDocumentActionDraft", "Rust-owned local action factory"],
  ["single-pair-editor", "commands.ruleDefinitionHttpConditionDraft", "Rust-owned HTTP condition factory"],
  ["single-pair-editor", "httpStage?.match_fields", "Rust-owned HTTP field capabilities"],
  ["application-editor", "rule_definition_http_condition_draft", "Rust-owned HTTP condition contract"],
  ["application-editor", "document_schema_field_capabilities", "Rust-owned Document schema capability projection"],
  ["application-capabilities", "match_fields", "Rust-owned HTTP match field catalog"],
  ["application-capabilities", "RuleMatchSelectorKind::HeaderNamePointer", "Header selector capability"],
  ["application-portability", "workspace.rule_definitions", "authoritative portable rule collection"],
  ["application-portability", "validate_document_condition_schema(binding.condition", "typed portable condition schema validation"],
  ["application-portability", "validate_unified_action_schema(binding.action", "typed portable action schema validation"],
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
  ["rule-model", "descriptor.kind !== mutation.type", "stage action capability lookup by action kind"],
  ["rule-model", "descriptor.target_value_type", "Document action target compatibility"],
  ["rule-model", "descriptor.operand_value_type", "Document action operand compatibility"],
  ["generated", "export type RuleDocumentActionCapability", "generated Document action capability"],
  ["generated", "document_fields: RuleDocumentSchemaFieldCapability[]", "generated schema field capability catalog"],
  ["generated", "condition: Condition", "generated singular condition"],
  ["generated", "action: UnifiedAction", "generated singular action"],
  ["domain-rule", "pub condition: Condition", "singular condition domain contract"],
  ["domain-rule", "pub action: UnifiedAction", "singular action domain contract"],
  ["generated", 'export type RuleStage = "proxy_to_upstream" | "proxy_to_app";', "generated two-stage rule contract"],
  ["exchange-observation", "RuleProcessingAccumulator", "bounded process evidence accumulator"],
  ["exchange-observation", "MAX_OBSERVATION_TEXT_BYTES.saturating_sub", "shared observation serialization budget"],
  ["exchange-observation", "changes_truncated", "typed process evidence truncation"],
  ["exchange-observation", "event = \"processed\"", "Exchange processed event"],
  ["exchange-observation", "observe_context::<P, D>(\"encoded\"", "Exchange encoded event"],
  ["joint-runtime", "RuleProcessingChange", "runtime per-rule changes"],
  ["unified-execution", "schema.resolve_match_path(path)", "wildcard schema path validation"],
  ["unified-mutation", "DocumentMutation::Clear { path, value_type }", "typed Clear schema validation"],
  ["unified-mutation", "Some((items.as_ref(), value.value_type()))", "array item operand schema validation"],
  ["single-pair-editor", "selectedSchema?.predicates", "schema-owned predicate capability"],
  ["single-pair-editor", "selectedDocumentActions", "schema-owned action capability"],
  ["rule-model", "return unreachableContract(field);", "exhaustive HTTP field mapping"],
  ["rule-model", "return unreachableContract(operator);", "exhaustive HTTP operator mapping"],
  ["rule-model", 'if ("UpstreamConnectTimeout" in action)', "exhaustive terminal action mapping"],
  ["runtime-contract", "JointRuleConditionEvaluation", "shared typed joint condition evaluation"],
  ["runtime-contract", "UnifiedOwned(JointConditionEvaluation)", "explicit unified-owned gate result"],
  ["runtime-contract", "NotOwned", "explicit ordinary-rule gate result"],
  ["runtime-actor", "let checkpoint = current.clone()", "actor lifecycle checkpoint"],
  ["runtime-actor", "commit_runtime_deltas", "actor lifecycle commit owner"],
  ["runtime-evaluation", "rules: &[RuleDefinition]", "active RuleDefinition evaluation owner"],
  ["runtime-evaluation", "joint.gate(rule.rule_id().as_uuid())?", "Socket typed ownership gate"],
  ["runtime-evaluation", "rule.lifecycle_delta_for_successful_match", "typed lifecycle delta"],
  ["joint-runtime", "JointRuleConditionEvaluation::NotOwned", "joint ownership miss result"],
  ["http-rule-runtime", "UnifiedRuleProgram", "HTTP unified runtime program"],
  ["http-rule-runtime", "rule_definitions", "HTTP authoritative unified rules"],
  ["plain-json-runtime", "Document::parse_json", "schema-free HTTP JSON decode"],
  ["plain-json-runtime", "JointDocumentEvaluation::new_plain_json", "schema-free joint Document transaction"],
  ["plain-json-runtime", "BODY_DECODE_FAILED", "plain JSON UTF-8 decode failure"],
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
if (/\bModal\b|<Modal\./u.test(sources.get("rules-view"))) {
  failures.push("rules-view: modal rule editor remains");
}
if (sources.get("rule-creation").includes("进入规则编辑器")) {
  failures.push("rule-creation: retired intermediate creation step remains");
}
if (sources.get("rule-list").includes("rule-stage-heading")) {
  failures.push("rule-list: removed stage grouping remains");
}
for (const removedTree of ["ConditionTreeEditor", "DocumentMetadataTree"]) {
  if (sources.get("single-pair-editor").includes(removedTree)) {
    failures.push(`rule-editor: removed recursive editor remains: ${removedTree}`);
  }
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
if (/\bConditionTree\b/u.test(sources.get("generated"))) failures.push("generated: recursive ConditionTree remains");
if (sources.get("generated").includes("RejectTlsHandshake")) failures.push("generated: removed TLS rule action remains");
if (/\b(?:Breakpoint|BreakpointDecision|HttpDocumentRuleContent)\b|\bPause\b/u.test(sources.get("generated"))) {
  failures.push("generated: removed breakpoint/Pause or duplicate HTTP Document binding remains");
}
for (const [owner, source] of sources) {
  if (/\b(?:NthHit|NthCounter)\b|\bnth_hit\b|ruleDefinitionNthHit/u.test(source)) {
    failures.push(`${owner}: removed Nth capability remains`);
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
