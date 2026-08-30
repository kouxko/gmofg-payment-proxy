import type {
  Condition,
  ConditionTree,
  DocumentMutation,
  DocumentValue,
  HttpRuleEditorStage,
  MatchCondition,
  MatchField,
  ProtocolPackageRef,
  ProtocolRuleCommonActionCapability,
  ProtocolRuleFieldCapability,
  RuleAction,
  RuleActionKind,
  RuleDefinitionSaveInput,
  RuleDefinition_Serialize,
  RuleEditorContext,
  RuleMatchFieldKind,
  RuleStage,
  SocketRuleEditorStage,
  TerminalAction,
} from "@/generated/rust-types";

export const RULE_STAGE_ORDER = [
  "app_to_proxy",
  "proxy_to_upstream",
  "upstream_to_proxy",
  "proxy_to_app",
  "tls_handshake",
] as const satisfies readonly RuleStage[];

export const NEW_MESSAGE_RULE_STAGES = [
  "proxy_to_upstream",
  "proxy_to_app",
] as const satisfies readonly RuleStage[];

const STAGE_LABELS: Record<RuleStage, string> = {
  app_to_proxy: "App → Proxy",
  proxy_to_upstream: "Proxy → Upstream",
  upstream_to_proxy: "Upstream → Proxy",
  proxy_to_app: "Proxy → App",
  tls_handshake: "TLS 握手",
};

export function ruleStageLabel(stage: RuleStage) {
  return STAGE_LABELS[stage];
}

export function groupRulesByStage(rules: readonly RuleDefinition_Serialize[]) {
  return RULE_STAGE_ORDER.map((stage) => ({
    stage,
    rules: rules
      .filter((rule) => rule.stage === stage)
      .sort((left, right) => left.priority - right.priority || left.rule_id.localeCompare(right.rule_id)),
  }));
}

export function ruleContentLabel(rule: RuleDefinition_Serialize) {
  return rule.content.type === "http" ? "HTTP" : "Socket";
}

export function ruleStageIncompatibility(
  input: RuleDefinitionSaveInput,
  context: RuleEditorContext | undefined,
  stage: RuleStage,
): string | null {
  if (!context) return "Rust 尚未返回阶段编辑能力。";
  const candidate = context.content.value.stages.find((item) => item.stage === stage);
  if (!candidate) return "Rust 未声明目标阶段的编辑能力。";
  if (input.draft.content.type === "http") {
    if (context.content.type !== "http" || !("http" in candidate)) return "目标阶段不支持 HTTP 规则内容。";
    return httpStageIncompatibility(input.draft.content.value, candidate);
  }
  if (context.content.type !== "socket" || !("fields" in candidate)) return "目标阶段不支持 Socket Document 规则内容。";
  return socketStageIncompatibility(input.draft.content.value, context.content.value.package, candidate);
}

function httpStageIncompatibility(content: Extract<RuleDefinitionSaveInput["draft"]["content"], { type: "http" }>["value"], stage: HttpRuleEditorStage) {
  const leaves = conditionLeaves(content.condition);
  const httpConditions = leaves.filter((leaf): leaf is Extract<Condition, { source: "http" }> => leaf.source === "http");
  const httpActions = content.actions.filter((action) => action.source === "http" || action.source === "terminal");
  if (httpConditions.length > 0 || httpActions.length > 0) {
    if (!stage.http) return "目标阶段没有可编辑当前 HTTP 条件或动作的能力。";
    for (const leaf of httpConditions) {
      const kind = matchFieldKind(leaf.condition);
      if (kind && !stage.http.match_field_kinds.includes(kind)) return `目标阶段不支持 HTTP 条件 ${matchFieldLabel(kind)}。`;
    }
    const actionKinds = stage.http.actions.map((action) => action.kind);
    for (const action of httpActions) {
      const kind = ruleActionKind(action.source === "http" ? action.value : { Terminal: action.value });
      if (!actionKinds.includes(kind)) return `目标阶段不支持 HTTP 动作 ${ruleActionKindLabel(kind)}。`;
    }
  }
  return content.document ? documentIncompatibility(content.condition, content.actions, content.document.package, stage.package, stage.document_fields, stage.document_common_actions) : null;
}

function socketStageIncompatibility(content: Extract<RuleDefinitionSaveInput["draft"]["content"], { type: "socket" }>["value"], expectedPackage: ProtocolPackageRef, stage: SocketRuleEditorStage) {
  return documentIncompatibility(content.condition, content.actions, content.package, expectedPackage, stage.fields, stage.common_actions);
}

function documentIncompatibility(
  condition: ConditionTree,
  actions: import("@/generated/rust-types").UnifiedAction[],
  packageRef: ProtocolPackageRef,
  expectedPackage: ProtocolPackageRef | null,
  fields: ProtocolRuleFieldCapability[],
  commonActions: ProtocolRuleCommonActionCapability[],
) {
  if (!expectedPackage || packageRef.id !== expectedPackage.id || packageRef.version !== expectedPackage.version) {
    return "目标阶段的 Document 协议包与当前内容不一致。";
  }
  for (const leaf of conditionLeaves(condition).filter((item) => item.source === "document")) {
    const field = fields.find((item) => item.name === leaf.path);
    if (!field || !predicateMatchesType(leaf.predicate, field.type)) {
      return `目标阶段不能编辑 Document 条件字段 ${leaf.path}。`;
    }
  }
  for (const action of actions) {
    const reason = documentActionIncompatibility(action, fields, commonActions);
    if (reason) return reason;
  }
  return null;
}

function documentActionIncompatibility(
  action: import("@/generated/rust-types").UnifiedAction,
  fields: ProtocolRuleFieldCapability[],
  commonActions: ProtocolRuleCommonActionCapability[],
) {
  if (action.source === "record_match") {
    return commonActions.includes("record_match") ? null : "目标阶段不支持 Document 动作 record_match。";
  }
  if (action.source !== "document") return null;
  const mutation: DocumentMutation = action.value;
  const field = fields.find((item) => item.name === mutation.path);
  const capability = mutation.type === "set" ? "set_field" : mutation.type === "clear" ? "clear_field" : null;
  if (!field || !capability || !field.actions.includes(capability) || (mutation.type === "set" && !valueMatchesType(mutation.value, field.type))) {
    return `目标阶段不能编辑 Document 动作字段 ${mutation.path}。`;
  }
  return null;
}

function conditionLeaves(tree: ConditionTree): Condition[] {
  return tree.operator === "leaf" ? [tree.children] : tree.children.flatMap(conditionLeaves);
}

function predicateMatchesType(predicate: import("@/generated/rust-types").DocumentPredicate, type: ProtocolRuleFieldCapability["type"]): boolean {
  return predicate.type === type || (predicate.type === "null_equal" && false);
}

function valueMatchesType(value: DocumentValue, type: ProtocolRuleFieldCapability["type"]): boolean {
  if (type === "string") return typeof value === "string";
  if (type === "number") return typeof value === "number";
  if (type === "boolean") return typeof value === "boolean";
  if (type === "array") return Array.isArray(value);
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function matchFieldKind(condition: MatchCondition): RuleMatchFieldKind | null {
  if (!("Field" in condition) || !condition.Field) return null;
  const field: MatchField = condition.Field.field;
  if (field === "TerminalIp") return "terminal_ip";
  if (field === "CertificateFingerprint") return "certificate_fingerprint";
  if (field === "PathOrRequestType") return "path_or_request_type";
  return "json_path";
}

function ruleActionKind(action: RuleAction): RuleActionKind {
  if (action === "Pause") return "pause";
  if ("SetJsonField" in action) return "set_json_field";
  if ("ReplaceBodyText" in action) return "replace_body_text";
  if ("SetHeader" in action) return "set_header";
  if ("Delay" in action) return "delay";
  if ("Jitter" in action) return "jitter";
  if ("Throttle" in action) return "throttle";
  if ("Intermittent" in action) return "intermittent";
  if ("CustomHttpStatus" in action) return "custom_http_status";
  return terminalActionKind(action.Terminal);
}

function terminalActionKind(action: TerminalAction): RuleActionKind {
  if (action === "RejectTlsHandshake") return "reject_tls_handshake";
  if (action === "DisconnectBeforeUpstream") return "disconnect_before_upstream";
  const key = Object.keys(action)[0];
  return ({
    UpstreamConnectTimeout: "upstream_connect_timeout", UpstreamWriteTimeout: "upstream_write_timeout",
    UpstreamReadTimeout: "upstream_read_timeout", DropUpstreamResponse: "drop_upstream_response",
    MockResponse: "mock_response", InvalidJson: "invalid_json", IncorrectContentLength: "incorrect_content_length",
    TruncateResponse: "truncate_response", DisconnectDuringUpstreamWrite: "disconnect_during_upstream_write",
    DisconnectDuringDownstreamWrite: "disconnect_during_downstream_write",
  } as Record<string, RuleActionKind>)[key];
}

function matchFieldLabel(kind: RuleMatchFieldKind) {
  return ({ terminal_ip: "终端 IP", certificate_fingerprint: "证书指纹", path_or_request_type: "URL / 请求类型", json_path: "JSON Path" } as const)[kind];
}

function ruleActionKindLabel(kind: RuleActionKind) {
  return ({ set_header: "Set Header", set_json_field: "Set JSON Field", replace_body_text: "Replace Body", mock_response: "Mock Response" } as Partial<Record<RuleActionKind, string>>)[kind] ?? kind;
}
