import type {
  Condition,
  ProtocolRuleFieldActionCapability,
  ProtocolRuleFieldCapability,
  ProtocolRuleFieldOperatorCapability,
  RuleLocalDocumentActionKind,
  RuleLocalDocumentPredicateKind,
  RuleLocalDocumentTypeCapability,
  RuleLocalDocumentValueType,
  UnifiedAction,
} from "@/generated/rust-types";

export type DocumentEditorField = Omit<ProtocolRuleFieldCapability, "type" | "operators" | "actions"> & {
  type: RuleLocalDocumentValueType;
  predicates: RuleLocalDocumentPredicateKind[];
  actions: RuleLocalDocumentActionKind[];
};

export function documentEditorFields(
  schemaFields: ProtocolRuleFieldCapability[],
  documentConditions: Extract<Condition, { source: "document" }>[],
  actions: UnifiedAction[],
  localTypes: RuleLocalDocumentTypeCapability[],
): DocumentEditorField[] {
  const fields = new Map(schemaFields.map((field) => [field.name, schemaField(field)]));
  const capabilities = capabilityMap(localTypes);
  for (const condition of documentConditions) {
    if (fields.has(condition.path)) continue;
    const type = conditionValueType(condition);
    const capability = capabilities.get(type);
    if (capability) fields.set(condition.path, localField(condition.path, type, capability));
  }
  for (const action of actions) {
    if (action.source !== "document" || fields.has(action.value.path)) continue;
    const type = actionValueType(action.value);
    const capability = capabilities.get(type);
    if (capability) fields.set(action.value.path, localField(action.value.path, type, capability));
  }
  return [...fields.values()];
}

export function ruleLocalFields(
  documentConditions: Extract<Condition, { source: "document" }>[],
  actions: UnifiedAction[],
  schemaFields: ProtocolRuleFieldCapability[],
  localTypes: RuleLocalDocumentTypeCapability[],
): DocumentEditorField[] {
  const schema = new Map(schemaFields.map((field) => [field.name, schemaField(field)]));
  const capabilities = capabilityMap(localTypes);
  const local = new Map<string, DocumentEditorField>();
  for (const condition of documentConditions) {
    const type = conditionValueType(condition);
    const declared = schema.get(condition.path);
    const capability = capabilities.get(type);
    if (declared) local.set(condition.path, declared);
    else if (capability) local.set(condition.path, localField(condition.path, type, capability));
  }
  for (const action of actions) {
    if (action.source !== "document" || local.has(action.value.path)) continue;
    const type = actionValueType(action.value);
    const declared = schema.get(action.value.path);
    const capability = capabilities.get(type);
    if (declared) local.set(action.value.path, declared);
    else if (capability) local.set(action.value.path, localField(action.value.path, type, capability));
  }
  return [...local.values()];
}

function schemaField(field: ProtocolRuleFieldCapability): DocumentEditorField {
  return {
    ...field,
    predicates: field.operators.map(schemaPredicate),
    actions: field.actions.map(schemaAction),
  };
}

function schemaPredicate(value: ProtocolRuleFieldOperatorCapability): RuleLocalDocumentPredicateKind {
  switch (value) {
    case "equals": return "equals";
  }
}

function schemaAction(value: ProtocolRuleFieldActionCapability): RuleLocalDocumentActionKind {
  switch (value) {
    case "set_field": return "set";
    case "clear_field": return "clear";
  }
}

function capabilityMap(localTypes: RuleLocalDocumentTypeCapability[]) {
  return new Map(localTypes.map((capability) => [capability.value_type, capability]));
}

function localField(path: string, type: RuleLocalDocumentValueType, capability: RuleLocalDocumentTypeCapability): DocumentEditorField {
  return { name: path, label: path || "/", type, predicates: capability.predicates, actions: capability.actions };
}

function conditionValueType(condition: Extract<Condition, { source: "document" }>): RuleLocalDocumentValueType {
  return condition.predicate.type === "null_equal" ? "null" : condition.predicate.type;
}

function documentValueType(value: import("@/generated/rust-types").DocumentValue): RuleLocalDocumentValueType {
  if (typeof value === "string") return "string";
  if (typeof value === "number") return "number";
  if (typeof value === "boolean") return "boolean";
  if (Array.isArray(value)) return "array";
  if (value === null) return "null";
  return "object";
}

function actionValueType(action: Extract<UnifiedAction, { source: "document" }>["value"]): RuleLocalDocumentValueType {
  return action.type === "clear" ? action.value_type : documentValueType(action.value);
}
