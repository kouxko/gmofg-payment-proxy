import type {
  Condition,
  DocumentMutation,
  DocumentValue,
  HttpRuleEditorStageViewModel,
  MatchField,
  ProtocolPackageRef,
  RuleCommonActionCapability,
  HttpAction,
  RuleActionKind,
  RuleDefinitionSaveInput,
  RuleDefinition_Serialize,
  RuleEditorContext,
  RuleMatchFieldKind,
  RuleLocalDocumentTypeCapability,
  RuleLocalDocumentValueType,
  RuleStage,
  SocketRuleEditorStageViewModel,
  TerminalAction,
} from "@/generated/rust-types";
import { documentSchemaFields, type DocumentSchemaField } from "./rule-document-schema";

const STAGE_LABELS: Record<RuleStage, string> = {
  proxy_to_upstream: "Proxy → Server",
  proxy_to_app: "Proxy → App",
};

export function ruleStageLabel(stage: RuleStage) {
  return STAGE_LABELS[stage];
}

const DIRECTION_LABELS: Record<RuleStage, string> = {
  proxy_to_upstream: "上行",
  proxy_to_app: "下行",
};

export function ruleDirectionLabel(stage: RuleStage) {
  return DIRECTION_LABELS[stage];
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
    return httpStageIncompatibility(input.draft.content.value, candidate, context.local_document_types);
  }
  if (context.content.type !== "socket" || "http" in candidate) return "目标阶段不支持 Socket Document 规则内容。";
  return socketStageIncompatibility(input.draft.content.value, context.content.value.package, candidate, context.local_document_types);
}

function httpStageIncompatibility(content: Extract<RuleDefinitionSaveInput["draft"]["content"], { type: "http" }>["value"], stage: HttpRuleEditorStageViewModel, localTypes: RuleLocalDocumentTypeCapability[]) {
  const httpCondition = content.condition.source === "http" ? content.condition : undefined;
  const httpAction = content.action.source === "http" || content.action.source === "terminal" ? content.action : undefined;
  if (httpCondition || httpAction) {
    if (!stage.http) return "目标阶段没有可编辑当前 HTTP 条件或动作的能力。";
    if (httpCondition) {
      const kind = matchFieldKind(httpCondition);
      const capability = kind ? stage.http.match_fields.find((item) => item.kind === kind) : undefined;
      if (!capability) return `目标阶段不支持 HTTP 条件 ${kind ? matchFieldLabel(kind) : "未知字段"}。`;
      if (!capability.operators.includes(matchOperatorKind(httpCondition.operator))) {
        return `目标阶段不支持 HTTP 条件 ${matchFieldLabel(capability.kind)} 的当前操作符。`;
      }
    }
    if (httpAction) {
      const actionKinds = stage.http.actions.map((action) => action.kind);
      const kind = ruleActionKind(httpAction.source === "http" ? httpAction.value : { Terminal: httpAction.value });
      if (!actionKinds.includes(kind)) return `目标阶段不支持 HTTP 动作 ${ruleActionKindLabel(kind)}。`;
    }
  }
  return documentIncompatibility(content.condition, content.action, stage.document_fields, stage.document_common_actions, localTypes);
}

function socketStageIncompatibility(content: Extract<RuleDefinitionSaveInput["draft"]["content"], { type: "socket" }>["value"], expectedPackage: ProtocolPackageRef, stage: SocketRuleEditorStageViewModel, localTypes: RuleLocalDocumentTypeCapability[]) {
  if (content.package.id !== expectedPackage.id || content.package.version !== expectedPackage.version) {
    return "目标阶段的 Document 协议包与当前内容不一致。";
  }
  return documentIncompatibility(content.condition, content.action, stage.document_fields, stage.common_actions, localTypes);
}

function documentIncompatibility(
  condition: Condition,
  action: import("@/generated/rust-types").UnifiedAction,
  fieldCapabilities: import("@/generated/rust-types").RuleDocumentSchemaFieldCapability[],
  commonActions: RuleCommonActionCapability[],
  localTypes: RuleLocalDocumentTypeCapability[],
) {
  const fields = documentSchemaFields(fieldCapabilities);
  if (condition.source === "document" || condition.source === "document_pattern") {
    const field = fields.find((item) => item.name === condition.path);
    if (field && !predicateMatchesType(condition.predicate, field.type)) {
      return `目标阶段不能编辑 Document 条件字段 ${condition.path}。`;
    }
  }
  return documentActionIncompatibility(action, fields, commonActions, localTypes);
}

function documentActionIncompatibility(
  action: import("@/generated/rust-types").UnifiedAction,
  fields: DocumentSchemaField[],
  commonActions: RuleCommonActionCapability[],
  localTypes: RuleLocalDocumentTypeCapability[],
) {
  if (action.source === "record_match") {
    return commonActions.includes("record_match") ? null : "目标阶段不支持 Document 动作 record_match。";
  }
  if (action.source !== "document") return null;
  const mutation: DocumentMutation = action.value;
  const field = fields.find((item) => item.name === mutation.path);
  const mutationValueType = mutation.type === "clear" ? mutation.value_type : documentValueType(mutation.value);
  const descriptors = field
    ? field.actions
    : localTypes.flatMap((capability) => capability.actions);
  const descriptor = descriptors.find((candidate) => documentActionDescriptorMatches(candidate, mutation, mutationValueType));
  if (!descriptor) {
    return `目标阶段不能编辑 Document 动作字段 ${mutation.path}。`;
  }
  return null;
}

function documentActionDescriptorMatches(
  descriptor: import("@/generated/rust-types").RuleDocumentActionCapability,
  mutation: DocumentMutation,
  mutationValueType: RuleLocalDocumentValueType,
) {
  if (descriptor.kind !== mutation.type) return false;
  switch (mutation.type) {
    case "set":
      return descriptor.target_kind === "node"
        && descriptor.target_value_type === mutationValueType
        && descriptor.operand_value_type === mutationValueType;
    case "clear":
      return descriptor.target_kind === "node"
        && descriptor.target_value_type === mutation.value_type
        && descriptor.operand_value_type === null;
    case "insert":
    case "append":
      return descriptor.target_kind === "array"
        && descriptor.target_value_type === "array"
        && descriptor.operand_value_type === mutationValueType;
  }
}

function documentValueType(value: DocumentValue): RuleLocalDocumentValueType {
  if (typeof value === "string") return "string";
  if (typeof value === "number") return "number";
  if (typeof value === "boolean") return "boolean";
  if (Array.isArray(value)) return "array";
  return value === null ? "null" : "object";
}

function predicateMatchesType(predicate: import("@/generated/rust-types").DocumentPredicate, type: DocumentSchemaField["type"]): boolean {
  return predicate.type === type || (predicate.type === "null_equal" && false);
}


function matchFieldKind(condition: Extract<Condition, { source: "http" }>): RuleMatchFieldKind | null {
  const field: MatchField = condition.field;
  if (field === "TerminalIp") return "terminal_ip";
  if (field === "CertificateFingerprint") return "certificate_fingerprint";
  if (field === "Method") return "method";
  if (field === "RequestTarget") return "request_target";
  if (typeof field === "object" && "Header" in field) return "header";
  return unreachableContract(field);
}

function matchOperatorKind(operator: import("@/generated/rust-types").MatchOperator): import("@/generated/rust-types").RuleMatchOperatorKind {
  if ("Equals" in operator) return "equals";
  if ("Contains" in operator) return "contains";
  if ("StartsWith" in operator) return "starts_with";
  if ("EndsWith" in operator) return "ends_with";
  if ("Wildcard" in operator) return "wildcard";
  return unreachableContract(operator);
}

function ruleActionKind(action: HttpAction): RuleActionKind {
  if ("SetJsonField" in action) return "set_json_field";
  if ("ReplaceBodyText" in action) return "replace_body_text";
  if ("SetHeader" in action) return "set_header";
  if ("Delay" in action) return "delay";
  if ("Jitter" in action) return "jitter";
  if ("Throttle" in action) return "throttle";
  if ("Intermittent" in action) return "intermittent";
  if ("CustomHttpStatus" in action) return "custom_http_status";
  if ("Terminal" in action) return terminalActionKind(action.Terminal);
  return unreachableContract(action);
}

function terminalActionKind(action: TerminalAction): RuleActionKind {
  if (action === "DisconnectBeforeUpstream") return "disconnect_before_upstream";
  if ("UpstreamConnectTimeout" in action) return "upstream_connect_timeout";
  if ("UpstreamWriteTimeout" in action) return "upstream_write_timeout";
  if ("UpstreamReadTimeout" in action) return "upstream_read_timeout";
  if ("DropUpstreamResponse" in action) return "drop_upstream_response";
  if ("MockResponse" in action) return "mock_response";
  if ("InvalidJson" in action) return "invalid_json";
  if ("IncorrectContentLength" in action) return "incorrect_content_length";
  if ("TruncateResponse" in action) return "truncate_response";
  if ("DisconnectDuringUpstreamWrite" in action) return "disconnect_during_upstream_write";
  if ("DisconnectDuringDownstreamWrite" in action) return "disconnect_during_downstream_write";
  return unreachableContract(action);
}

const MATCH_FIELD_LABELS = {
  terminal_ip: "终端 IP",
  certificate_fingerprint: "证书指纹",
  method: "Method",
  request_target: "Path（包含 Query 参数）",
  header: "Header",
} as const satisfies Record<RuleMatchFieldKind, string>;

const RULE_ACTION_LABELS = {
  set_json_field: "Set JSON Field", replace_body_text: "Replace Body", set_header: "Set Header",
  delay: "Delay", jitter: "Jitter", throttle: "Throttle", intermittent: "Intermittent",
  custom_http_status: "Custom HTTP Status",
  disconnect_before_upstream: "Disconnect Before Upstream", upstream_connect_timeout: "Upstream Connect Timeout",
  upstream_write_timeout: "Upstream Write Timeout", upstream_read_timeout: "Upstream Read Timeout",
  drop_upstream_response: "Drop Upstream Response", mock_response: "Mock Response", invalid_json: "Invalid JSON",
  incorrect_content_length: "Incorrect Content Length", truncate_response: "Truncate Response",
  disconnect_during_upstream_write: "Disconnect During Upstream Write",
  disconnect_during_downstream_write: "Disconnect During Downstream Write",
} as const satisfies Record<RuleActionKind, string>;

export function matchFieldLabel(kind: RuleMatchFieldKind) {
  return MATCH_FIELD_LABELS[kind];
}

export function ruleActionKindLabel(kind: RuleActionKind) {
  return RULE_ACTION_LABELS[kind];
}

export function httpActionLabel(action: HttpAction) {
  return ruleActionKindLabel(ruleActionKind(action));
}

function unreachableContract(value: never): never {
  throw new Error(`Rust 规则合同包含未处理的 variant: ${String(value)}`);
}
