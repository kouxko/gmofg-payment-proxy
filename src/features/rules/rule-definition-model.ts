import type {
  DocumentAction,
  HttpDocumentRuleContent,
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
  SocketRuleContent,
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
      .sort((left, right) => left.priority - right.priority || left.created_order - right.created_order),
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
  if (content.conditions.length > 0 || content.actions.length > 0) {
    if (!stage.http) return "目标阶段没有可编辑当前 HTTP 条件或动作的能力。";
    for (const condition of content.conditions) {
      const kind = matchFieldKind(condition);
      if (kind && !stage.http.match_field_kinds.includes(kind)) return `目标阶段不支持 HTTP 条件 ${matchFieldLabel(kind)}。`;
    }
    const actionKinds = stage.http.actions.map((action) => action.kind);
    for (const action of content.actions) {
      const kind = ruleActionKind(action);
      if (!actionKinds.includes(kind)) return `目标阶段不支持 HTTP 动作 ${ruleActionKindLabel(kind)}。`;
    }
  }
  return content.document
    ? documentIncompatibility(content.document, stage.package, stage.schema_version, stage.document_fields, stage.document_common_actions)
    : null;
}

function socketStageIncompatibility(content: SocketRuleContent, expectedPackage: ProtocolPackageRef, stage: SocketRuleEditorStage) {
  return documentIncompatibility(content, expectedPackage, stage.schema_version, stage.fields, stage.common_actions);
}

function documentIncompatibility(
  content: HttpDocumentRuleContent | SocketRuleContent,
  expectedPackage: ProtocolPackageRef | null,
  expectedSchemaVersion: number | null,
  fields: ProtocolRuleFieldCapability[],
  commonActions: ProtocolRuleCommonActionCapability[],
) {
  if (!expectedPackage || content.package.id !== expectedPackage.id || content.package.version !== expectedPackage.version) {
    return "目标阶段的 Document 协议包与当前内容不一致。";
  }
  if (content.schema_version !== expectedSchemaVersion) return "目标阶段的 Document Schema 与当前内容不一致。";
  for (const condition of content.conditions) {
    const field = fields.find((item) => item.name === condition.field);
    if (!field || !field.operators.includes(condition.operator) || field.type !== condition.value.type) {
      return `目标阶段不能编辑 Document 条件字段 ${condition.field}。`;
    }
  }
  for (const action of content.actions) {
    const reason = documentActionIncompatibility(action, fields, commonActions);
    if (reason) return reason;
  }
  return null;
}

function documentActionIncompatibility(
  action: DocumentAction,
  fields: ProtocolRuleFieldCapability[],
  commonActions: ProtocolRuleCommonActionCapability[],
) {
  if (action.type === "record_match" || action.type === "clear_document") {
    return commonActions.includes(action.type) ? null : `目标阶段不支持 Document 动作 ${action.type}。`;
  }
  const field = fields.find((item) => item.name === action.field);
  if (!field || !field.actions.includes(action.type) || (action.type === "set_field" && field.type !== action.value.type)) {
    return `目标阶段不能编辑 Document 动作字段 ${action.field}。`;
  }
  return null;
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
