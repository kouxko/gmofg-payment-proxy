import type {
  DocumentAction,
  DocumentCondition,
  DocumentValue,
  OperationResultViewModel,
  ProxyListener,
  SocketDirection,
  SocketDocumentRuleDefinition,
  SocketRuleCapabilityCatalog,
  SocketRuleFieldCapability,
  SocketRuleSaveInput,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand } from "@/lib/ipc/client";

export type SocketRuleDraft = SocketRuleSaveInput;

export function scriptedSocketListeners(listeners: ProxyListener[]) {
  return listeners.filter(
    (listener) =>
      listener.data_plane.kind === "socket" &&
      listener.data_plane.settings.processing?.mode === "scripted",
  );
}

export function listenerDirections(listener: ProxyListener): SocketDirection[] {
  if (
    listener.data_plane.kind === "socket" &&
    listener.data_plane.settings.topology.mode === "local_responder"
  ) {
    return ["downstream"];
  }
  return ["upstream", "downstream"];
}

export function directionDecodeEnabled(
  listener: ProxyListener,
  direction: SocketDirection,
) {
  if (
    listener.data_plane.kind !== "socket" ||
    listener.data_plane.settings.processing?.mode !== "scripted"
  ) {
    return false;
  }
  if (listener.data_plane.settings.topology.mode === "local_responder") {
    return listener.data_plane.settings.processing.settings.upstream.decode_enabled;
  }
  return listener.data_plane.settings.processing.settings[direction]
    .decode_enabled;
}

export function newSocketRuleDraft(
  listener: ProxyListener,
  direction: SocketDirection,
  catalog: SocketRuleCapabilityCatalog,
): SocketRuleDraft {
  return {
    rule_id: null,
    expected_revision: null,
    enabled: true,
    priority: 100,
    listener_id: listener.id,
    package: catalog.package,
    schema_version: catalog.schema_version,
    direction,
    conditions: [],
    actions: [{ type: "record_match" }],
  };
}

export function draftFromRule(rule: SocketDocumentRuleDefinition): SocketRuleDraft {
  return {
    rule_id: rule.rule_id,
    expected_revision: rule.revision,
    enabled: rule.enabled,
    priority: rule.priority,
    listener_id: rule.listener_id,
    package: rule.package,
    schema_version: rule.schema_version,
    direction: rule.direction,
    conditions: rule.conditions,
    actions: rule.actions,
  };
}

export function emptyValue(field: SocketRuleFieldCapability): DocumentValue {
  switch (field.type) {
    case "string":
      return { type: "string", value: "" };
    case "int":
      return { type: "int", value: 0 };
    case "bool":
      return { type: "bool", value: false };
    case "blob":
      return { type: "blob", value: [] };
  }
}

export function conditionFor(field: SocketRuleFieldCapability): DocumentCondition {
  return { operator: "equals", field: field.name, value: emptyValue(field) };
}

export function setActionFor(field: SocketRuleFieldCapability): DocumentAction {
  return { type: "set_field", field: field.name, value: emptyValue(field) };
}

export function parseSocketRuleValue(
  fieldType: SocketRuleFieldCapability["type"],
  raw: string,
) {
  return callCommand(commands.socketRuleParseValue(fieldType, raw)).then((value) => {
    if (!isDocumentValueShape(value)) throw new Error("Rust 返回了无效的字段值");
    if (value.type !== fieldType) throw new Error("Rust 返回了错误的字段类型");
    return value;
  });
}

export function valueText(value: DocumentValue) {
  if (value.type === "blob") {
    return value.value.map((byte) => byte.toString(16).padStart(2, "0").toUpperCase()).join(" ");
  }
  return String(value.value);
}

export function capabilityCompatible(
  draft: SocketRuleDraft,
  catalog: SocketRuleCapabilityCatalog,
) {
  if (validateCapabilityCatalog(catalog)) return false;
  return validateSocketRuleDraft(draft, catalog) == null;
}

export function validateCapabilityCatalog(catalog: SocketRuleCapabilityCatalog) {
  if (!catalog || typeof catalog !== "object") return "规则能力数据无效。";
  if (!catalog.package || typeof catalog.package.id !== "string" || typeof catalog.package.version !== "string") return "规则能力缺少精确协议包身份。";
  if (!Number.isSafeInteger(catalog.schema_version) || catalog.schema_version < 0) return "规则能力包含无效 Schema 版本。";
  if (catalog.direction !== "upstream" && catalog.direction !== "downstream") return "规则能力包含未知方向。";
  if (!Array.isArray(catalog.fields) || !Array.isArray(catalog.common_actions)) return "规则能力字段或动作目录无效。";
  const names = new Set<string>();
  for (const field of catalog.fields) {
    if (!field || typeof field !== "object" || typeof field.name !== "string" || typeof field.label !== "string") return "规则能力包含无效字段。";
    if (!Array.isArray(field.operators) || !Array.isArray(field.actions)) return "规则字段能力目录无效。";
    if (names.has(field.name)) return "规则能力包含重复字段。";
    names.add(field.name);
    if (!["string", "int", "bool", "blob"].includes(field.type)) return "规则能力包含未知字段类型。";
    if (field.operators.some((operator) => operator !== "equals")) return "规则能力包含未知操作符。";
    if (field.actions.some((action) => action !== "set_field")) return "规则能力包含未知字段动作。";
  }
  if (catalog.common_actions.some((action) => action !== "record_match" && action !== "clear_document")) {
    return "规则能力包含未知公共动作。";
  }
}

export function isSocketRuleDefinition(value: unknown): value is SocketDocumentRuleDefinition {
  if (!value || typeof value !== "object") return false;
  const rule = value as Partial<SocketDocumentRuleDefinition>;
  return typeof rule.rule_id === "string" && rule.rule_id.length > 0
    && Number.isSafeInteger(rule.revision) && rule.revision! >= 1 && Number.isSafeInteger(rule.priority)
    && Number.isSafeInteger(rule.created_order) && rule.created_order! >= 1 && typeof rule.enabled === "boolean"
    && typeof rule.listener_id === "string" && rule.listener_id.length > 0
    && Boolean(rule.package) && typeof rule.package?.id === "string" && typeof rule.package?.version === "string"
    && Number.isSafeInteger(rule.schema_version) && rule.schema_version! >= 1
    && (rule.direction === "upstream" || rule.direction === "downstream")
    && Array.isArray(rule.conditions) && rule.conditions.length <= 64
    && Array.isArray(rule.actions) && rule.actions.length >= 1 && rule.actions.length <= 64
    && rule.conditions.every(isDocumentConditionShape)
    && rule.actions.every(isDocumentActionShape);
}

/**
 * IPC 列表必须作为一个整体可信：其中任何一项结构无效或 rule_id 重复时，
 * 都不能把剩余部分伪装成正常列表展示，否则用户可能对不完整事实执行修改。
 */
export function isSocketRuleList(value: unknown): value is SocketDocumentRuleDefinition[] {
  if (!Array.isArray(value) || !value.every(isSocketRuleDefinition)) return false;
  const ids = new Set(value.map((rule) => rule.rule_id));
  return ids.size === value.length;
}

export function saveResponseMatches(
  response: unknown,
  request: SocketRuleDraft,
  previous?: SocketDocumentRuleDefinition,
): response is SocketDocumentRuleDefinition {
  if (!isSocketRuleDefinition(response) || !sameRuleBinding(response, request)) return false;
  const expectedRevision = request.expected_revision == null ? 1 : request.expected_revision + 1;
  if (!Number.isSafeInteger(expectedRevision) || response.revision !== expectedRevision) return false;
  const identityMatches = request.rule_id == null
    ? response.created_order >= 1
    : previous != null
      && response.rule_id === request.rule_id
      && response.created_order === previous.created_order;
  return identityMatches
    && response.enabled === request.enabled
    && response.priority === request.priority
    && sameDocuments(response.conditions, request.conditions)
    && sameDocuments(response.actions, request.actions);
}

export function toggleResponseMatches(
  response: unknown,
  request: SocketDocumentRuleDefinition,
  enabled: boolean,
): response is SocketDocumentRuleDefinition {
  if (!isSocketRuleDefinition(response) || !sameRuleBinding(response, request)) return false;
  const expectedRevision = request.revision + 1;
  return Number.isSafeInteger(expectedRevision)
    && response.revision === expectedRevision
    && response.rule_id === request.rule_id
    && response.enabled === enabled
    && response.priority === request.priority
    && response.created_order === request.created_order
    && sameDocuments(response.conditions, request.conditions)
    && sameDocuments(response.actions, request.actions);
}

export function deleteResponseMatches(
  response: unknown,
  ruleId: string,
): response is OperationResultViewModel {
  if (!response || typeof response !== "object") return false;
  const result = response as Partial<OperationResultViewModel>;
  return result.success === true
    && result.cancelled === false
    && typeof result.message === "string"
    && ["neutral", "info", "positive", "warning", "danger"].includes(result.ui_tone ?? "")
    && result.entity_id === ruleId
    && Number.isSafeInteger(result.revision)
    && (result.revision ?? 0) > 0
    && typeof result.requires_restart === "boolean";
}

function sameRuleBinding(
  response: SocketDocumentRuleDefinition,
  expected: Pick<SocketRuleDraft, "listener_id" | "package" | "schema_version" | "direction">,
) {
  return response.listener_id === expected.listener_id
    && response.package.id === expected.package.id
    && response.package.version === expected.package.version
    && response.schema_version === expected.schema_version
    && response.direction === expected.direction;
}

function sameDocuments(left: unknown, right: unknown) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function isDocumentConditionShape(value: unknown) {
  if (!value || typeof value !== "object") return false;
  const condition = value as Partial<DocumentCondition>;
  return condition.operator === "equals" && typeof condition.field === "string"
    && isDocumentValueShape(condition.value);
}

function isDocumentActionShape(value: unknown) {
  if (!value || typeof value !== "object") return false;
  const action = value as Partial<DocumentAction>;
  if (action.type === "record_match" || action.type === "clear_document") return true;
  return action.type === "set_field" && typeof action.field === "string"
    && isDocumentValueShape(action.value);
}

function isDocumentValueShape(value: unknown): value is DocumentValue {
  if (!value || typeof value !== "object") return false;
  const candidate = value as { type?: unknown; value?: unknown };
  if (candidate.type === "string") return typeof candidate.value === "string" && candidate.value.length <= 16 * 1024;
  if (candidate.type === "int") return typeof candidate.value === "number" && Number.isSafeInteger(candidate.value);
  if (candidate.type === "bool") return typeof candidate.value === "boolean";
  return candidate.type === "blob" && Array.isArray(candidate.value) && candidate.value.length <= 64 * 1024
    && candidate.value.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255);
}

export function isDocumentValueForType(
  value: unknown,
  fieldType: SocketRuleFieldCapability["type"],
): value is DocumentValue {
  return isDocumentValueShape(value)
    && (value as DocumentValue).type === fieldType;
}

export function validateSocketRuleDraft(
  draft: SocketRuleDraft,
  catalog: SocketRuleCapabilityCatalog,
) {
  const fields = new Map(catalog.fields.map((field) => [field.name, field]));
  const conditionFields = new Set<string>();
  if (draft.conditions.length > 64 || draft.actions.length === 0 || draft.actions.length > 64) {
    return "规则必须包含 1 至 64 个动作，且条件不能超过 64 个。";
  }
  if (
    draft.package.id !== catalog.package.id ||
    draft.package.version !== catalog.package.version ||
    draft.schema_version !== catalog.schema_version ||
    draft.direction !== catalog.direction
  ) return "规则绑定与当前能力目录不一致。";
  for (const condition of draft.conditions) {
    const field = fields.get(condition.field);
    if (!field || condition.operator !== "equals" || !field.operators.includes("equals")) return "规则包含不受支持的条件。";
    if (conditionFields.has(condition.field)) return "同一字段不能添加重复条件。";
    conditionFields.add(condition.field);
    if (!valueMatchesField(condition.value, field)) return "条件值类型或大小不符合 Schema。";
  }
  for (const action of draft.actions) {
    if (action.type === "record_match") {
      if (!catalog.common_actions.includes("record_match")) return "RecordMatch 不可用。";
      continue;
    }
    if (action.type === "clear_document") {
      if (!catalog.common_actions.includes("clear_document")) return "ClearDocument 不可用。";
      continue;
    }
    if (action.type !== "set_field") return "规则包含未知动作。";
    const field = fields.get(action.field);
    if (!field?.actions.includes("set_field") || !valueMatchesField(action.value, field)) return "SetField 与 Schema 能力不兼容。";
  }
}

function valueMatchesField(value: DocumentValue, field: SocketRuleFieldCapability) {
  if (value.type !== field.type) return false;
  const raw: unknown = value.value;
  if (value.type === "string") return typeof raw === "string";
  if (value.type === "int") return typeof raw === "number" && Number.isSafeInteger(raw);
  if (value.type === "bool") return typeof raw === "boolean";
  return Array.isArray(raw) && raw.length <= 64 * 1024
    && raw.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255);
}
