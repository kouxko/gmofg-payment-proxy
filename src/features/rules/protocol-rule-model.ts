import type {
  DocumentAction,
  DocumentCondition,
  DocumentValue,
  OperationResultViewModel,
  ProxyListener,
  ProtocolRuleStage,
  ProtocolDocumentRuleDefinition,
  ProtocolRuleCapabilityCatalog,
  ProtocolRuleEditorContext,
  ProtocolRuleEditorStage,
  ProtocolRuleFieldCapability,
  ProtocolRuleSaveInput,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand } from "@/lib/ipc/client";

export type ProtocolRuleDraft = ProtocolRuleSaveInput;

export type ProtocolRuleKind = "http" | "socket";

export function protocolRuleListeners(
  listeners: ProxyListener[],
  kind: ProtocolRuleKind,
) {
  return listeners.filter(
    (listener) => listener.data_plane.kind === kind && isProtocolRuleListener(listener),
  );
}

export function isProtocolRuleListener(listener: ProxyListener) {
  return listener.data_plane.kind === "http"
    ? listener.data_plane.settings.body_processing.mode === "protocol"
    : listener.data_plane.settings.processing.mode === "scripted";
}

export function protocolRulePackage(listener: ProxyListener) {
  if (listener.data_plane.kind === "http") {
    return listener.data_plane.settings.body_processing.mode === "protocol"
      ? listener.data_plane.settings.body_processing.package
      : undefined;
  }
  return listener.data_plane.settings.processing.mode === "scripted"
    ? listener.data_plane.settings.processing.settings.package
    : undefined;
}

export function protocolRuleEntryDescription(listener: ProxyListener) {
  const packageRef = protocolRulePackage(listener);
  if (!packageRef) return "";
  if (listener.data_plane.kind === "http") return `HTTP · ${packageRef.id}@${packageRef.version}`;
  if (listener.data_plane.settings.topology.mode === "local_responder") {
    return `本机应答 · ${packageRef.id}@${packageRef.version}`;
  }
  return `Socket · 转发至上游 · ${packageRef.id}@${packageRef.version}`;
}

export function protocolRuleStageLabel(stage: ProtocolRuleStage) {
  switch (stage) {
    case "app_to_proxy": return "应用 → 代理";
    case "proxy_to_upstream": return "代理 → 上游服务";
    case "upstream_to_proxy": return "上游服务 → 代理";
    case "proxy_to_app": return "代理 → 应用";
  }
}

export function draftFromEditorStage(
  stage: ProtocolRuleEditorStage,
): ProtocolRuleDraft {
  return {
    ...stage.new_rule_draft,
    package: { ...stage.new_rule_draft.package },
    conditions: [...stage.new_rule_draft.conditions],
    actions: [...stage.new_rule_draft.actions],
  };
}

export function catalogFromEditorStage(
  context: ProtocolRuleEditorContext,
  stage: ProtocolRuleEditorStage,
): ProtocolRuleCapabilityCatalog {
  return {
    package: context.package,
    schema_version: stage.schema_version,
    stage: stage.stage,
    fields: stage.fields,
    common_actions: stage.common_actions,
  };
}

export function validateProtocolRuleEditorContext(context: ProtocolRuleEditorContext) {
  if (!context || typeof context !== "object") return "规则编辑上下文无效。";
  if (typeof context.listener_id !== "string" || context.listener_id.length === 0) return "规则编辑上下文缺少入口身份。";
  if (!context.package || typeof context.package.id !== "string" || typeof context.package.version !== "string") return "规则编辑上下文缺少协议包身份。";
  if (!Array.isArray(context.stages) || context.stages.length === 0) return "规则编辑上下文没有可用处理阶段。";
  const stages = new Set<ProtocolRuleStage>();
  for (const stage of context.stages) {
    if (!stage || typeof stage !== "object" || stages.has(stage.stage)) return "规则编辑上下文包含无效或重复阶段。";
    stages.add(stage.stage);
    const catalog = catalogFromEditorStage(context, stage);
    const catalogError = validateCapabilityCatalog(catalog);
    if (catalogError) return catalogError;
    const draft = stage.new_rule_draft;
    if (!draft || draft.rule_id !== null || draft.expected_revision !== null) return "Rust 新规则草稿包含无效身份。";
    if (draft.listener_id !== context.listener_id
      || draft.package.id !== context.package.id
      || draft.package.version !== context.package.version
      || draft.schema_version !== stage.schema_version
      || draft.stage !== stage.stage) return "Rust 新规则草稿绑定与编辑上下文不一致。";
    const draftError = validateProtocolRuleDraft(draft, catalog);
    if (draftError) return draftError;
  }
}

export function draftFromRule(rule: ProtocolDocumentRuleDefinition): ProtocolRuleDraft {
  return {
    rule_id: rule.rule_id,
    expected_revision: rule.revision,
    name: rule.name,
    enabled: rule.enabled,
    priority: rule.priority,
    listener_id: rule.listener_id,
    package: rule.package,
    schema_version: rule.schema_version,
    stage: rule.stage,
    conditions: rule.conditions,
    actions: rule.actions,
  };
}

export function emptyValue(field: ProtocolRuleFieldCapability): DocumentValue {
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

export function conditionFor(field: ProtocolRuleFieldCapability): DocumentCondition {
  return { operator: "equals", field: field.name, value: emptyValue(field) };
}

export function setActionFor(field: ProtocolRuleFieldCapability): DocumentAction {
  return { type: "set_field", field: field.name, value: emptyValue(field) };
}

export function clearActionFor(field: ProtocolRuleFieldCapability): DocumentAction {
  return { type: "clear_field", field: field.name };
}

export function parseProtocolRuleValue(
  fieldType: ProtocolRuleFieldCapability["type"],
  raw: string,
) {
  return callCommand(commands.protocolRuleParseValue(fieldType, raw)).then((value) => {
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
  draft: ProtocolRuleDraft,
  catalog: ProtocolRuleCapabilityCatalog,
) {
  if (validateCapabilityCatalog(catalog)) return false;
  return validateProtocolRuleDraft(draft, catalog) == null;
}

export function validateCapabilityCatalog(catalog: ProtocolRuleCapabilityCatalog) {
  if (!catalog || typeof catalog !== "object") return "规则能力数据无效。";
  if (!catalog.package || typeof catalog.package.id !== "string" || typeof catalog.package.version !== "string") return "规则能力缺少精确协议包身份。";
  if (!Number.isSafeInteger(catalog.schema_version) || catalog.schema_version < 0) return "规则能力包含无效字段结构版本。";
  if (!["app_to_proxy", "proxy_to_upstream", "upstream_to_proxy", "proxy_to_app"].includes(catalog.stage)) return "规则能力包含未知处理阶段。";
  if (!Array.isArray(catalog.fields) || !Array.isArray(catalog.common_actions)) return "规则能力字段或动作目录无效。";
  const names = new Set<string>();
  for (const field of catalog.fields) {
    if (!field || typeof field !== "object" || typeof field.name !== "string" || typeof field.label !== "string") return "规则能力包含无效字段。";
    if (!Array.isArray(field.operators) || !Array.isArray(field.actions)) return "规则字段能力目录无效。";
    if (names.has(field.name)) return "规则能力包含重复字段。";
    names.add(field.name);
    if (!["string", "int", "bool", "blob"].includes(field.type)) return "规则能力包含未知字段类型。";
    if (field.operators.some((operator) => operator !== "equals")) return "规则能力包含未知操作符。";
    if (field.actions.some((action) => action !== "set_field" && action !== "clear_field")) return "规则能力包含未知字段动作。";
  }
  if (catalog.common_actions.some((action) => action !== "record_match" && action !== "clear_document")) {
    return "规则能力包含未知公共动作。";
  }
}

export function isProtocolRuleDefinition(value: unknown): value is ProtocolDocumentRuleDefinition {
  if (!value || typeof value !== "object") return false;
  const rule = value as Partial<ProtocolDocumentRuleDefinition>;
  return typeof rule.rule_id === "string" && rule.rule_id.length > 0
    && typeof rule.name === "string" && rule.name.trim().length > 0
    && Number.isSafeInteger(rule.revision) && rule.revision! >= 1 && Number.isSafeInteger(rule.priority)
    && Number.isSafeInteger(rule.created_order) && rule.created_order! >= 1 && typeof rule.enabled === "boolean"
    && typeof rule.listener_id === "string" && rule.listener_id.length > 0
    && Boolean(rule.package) && typeof rule.package?.id === "string" && typeof rule.package?.version === "string"
    && Number.isSafeInteger(rule.schema_version) && rule.schema_version! >= 1
    && typeof rule.stage === "string"
    && ["app_to_proxy", "proxy_to_upstream", "upstream_to_proxy", "proxy_to_app"].includes(rule.stage)
    && Array.isArray(rule.conditions) && rule.conditions.length <= 64
    && Array.isArray(rule.actions) && rule.actions.length >= 1 && rule.actions.length <= 64
    && rule.conditions.every(isDocumentConditionShape)
    && rule.actions.every(isDocumentActionShape);
}

/**
 * IPC 列表必须作为一个整体可信：其中任何一项结构无效或 rule_id 重复时，
 * 都不能把剩余部分伪装成正常列表展示，否则用户可能对不完整事实执行修改。
 */
export function isProtocolRuleList(value: unknown): value is ProtocolDocumentRuleDefinition[] {
  if (!Array.isArray(value) || !value.every(isProtocolRuleDefinition)) return false;
  const ids = new Set(value.map((rule) => rule.rule_id));
  return ids.size === value.length;
}

export function saveResponseMatches(
  response: unknown,
  request: ProtocolRuleDraft,
  previous?: ProtocolDocumentRuleDefinition,
): response is ProtocolDocumentRuleDefinition {
  if (!isProtocolRuleDefinition(response) || !sameRuleBinding(response, request)) return false;
  const expectedRevision = request.expected_revision == null ? 1 : request.expected_revision + 1;
  if (!Number.isSafeInteger(expectedRevision) || response.revision !== expectedRevision) return false;
  const identityMatches = request.rule_id == null
    ? response.created_order >= 1
    : previous != null
      && response.rule_id === request.rule_id
      && response.created_order === previous.created_order;
  return identityMatches
    && response.name === request.name
    && response.enabled === request.enabled
    && response.priority === request.priority
    && sameDocuments(response.conditions, request.conditions)
    && sameDocuments(response.actions, request.actions);
}

export function toggleResponseMatches(
  response: unknown,
  request: ProtocolDocumentRuleDefinition,
  enabled: boolean,
): response is ProtocolDocumentRuleDefinition {
  if (!isProtocolRuleDefinition(response) || !sameRuleBinding(response, request)) return false;
  const expectedRevision = request.revision + 1;
  return Number.isSafeInteger(expectedRevision)
    && response.revision === expectedRevision
    && response.rule_id === request.rule_id
    && response.name === request.name
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
  response: ProtocolDocumentRuleDefinition,
  expected: Pick<ProtocolRuleDraft, "listener_id" | "package" | "schema_version" | "stage">,
) {
  return response.listener_id === expected.listener_id
    && response.package.id === expected.package.id
    && response.package.version === expected.package.version
    && response.schema_version === expected.schema_version
    && response.stage === expected.stage;
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
  if (action.type === "clear_field") return typeof action.field === "string";
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
  fieldType: ProtocolRuleFieldCapability["type"],
): value is DocumentValue {
  return isDocumentValueShape(value)
    && (value as DocumentValue).type === fieldType;
}

export function validateProtocolRuleDraft(
  draft: ProtocolRuleDraft,
  catalog: ProtocolRuleCapabilityCatalog,
) {
  if (draft.name.trim().length === 0 || draft.name.length > 128) {
    return "规则名称不能为空且不能超过 128 个字符。";
  }
  const fields = new Map(catalog.fields.map((field) => [field.name, field]));
  const conditionFields = new Set<string>();
  if (draft.conditions.length > 64 || draft.actions.length === 0 || draft.actions.length > 64) {
    return "规则必须包含 1 至 64 个动作，且条件不能超过 64 个。";
  }
  if (
    draft.package.id !== catalog.package.id ||
    draft.package.version !== catalog.package.version ||
    draft.schema_version !== catalog.schema_version ||
    draft.stage !== catalog.stage
  ) return "规则绑定与当前能力目录不一致。";
  for (const condition of draft.conditions) {
    const field = fields.get(condition.field);
    if (!field || condition.operator !== "equals" || !field.operators.includes("equals")) return "规则包含不受支持的条件。";
    if (conditionFields.has(condition.field)) return "同一字段不能添加重复条件。";
    conditionFields.add(condition.field);
    if (!valueMatchesField(condition.value, field)) return "条件值类型或大小不符合字段结构。";
  }
  for (const action of draft.actions) {
    if (action.type === "record_match") {
      if (!catalog.common_actions.includes("record_match")) return "RecordMatch 不可用。";
      continue;
    }
    if (action.type === "clear_document") {
      if (!catalog.common_actions.includes("clear_document")) return "清空全部字段动作不可用。";
      continue;
    }
    if (action.type === "clear_field") {
      const field = fields.get(action.field);
      if (!field?.actions.includes("clear_field")) return "清除字段与字段能力不兼容。";
      continue;
    }
    if (action.type !== "set_field") return "规则包含未知动作。";
    const field = fields.get(action.field);
    if (!field?.actions.includes("set_field") || !valueMatchesField(action.value, field)) return "设置字段与字段能力不兼容。";
  }
}

function valueMatchesField(value: DocumentValue, field: ProtocolRuleFieldCapability) {
  if (value.type !== field.type) return false;
  const raw: unknown = value.value;
  if (value.type === "string") return typeof raw === "string";
  if (value.type === "int") return typeof raw === "number" && Number.isSafeInteger(raw);
  if (value.type === "bool") return typeof raw === "boolean";
  return Array.isArray(raw) && raw.length <= 64 * 1024
    && raw.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255);
}
