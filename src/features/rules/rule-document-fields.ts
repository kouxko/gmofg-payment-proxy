import type {
  Condition,
  RuleDocumentActionCapability,
  RuleLocalDocumentPredicateKind,
  RuleLocalDocumentTypeCapability,
  RuleLocalDocumentValueType,
  UnifiedAction,
} from "@/generated/rust-types";
import type { DocumentSchemaField } from "./rule-document-schema";

export type DocumentEditorField = Omit<DocumentSchemaField, "type"> & {
  type: RuleLocalDocumentValueType;
  predicates: RuleLocalDocumentPredicateKind[];
  actions: RuleDocumentActionCapability[];
};

export function documentEditorFields(
  schemaFields: DocumentSchemaField[],
  documentConditions: Extract<Condition, { source: "document" | "document_pattern" }>[],
  actions: UnifiedAction[],
  localTypes: RuleLocalDocumentTypeCapability[],
): DocumentEditorField[] {
  const capabilities = capabilityMap(localTypes);
  const fields = new Map(schemaFields.map((field) => [field.name, schemaField(field)] as const));
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
  documentConditions: Extract<Condition, { source: "document" | "document_pattern" }>[],
  actions: UnifiedAction[],
  schemaFields: DocumentSchemaField[],
  localTypes: RuleLocalDocumentTypeCapability[],
): DocumentEditorField[] {
  const capabilities = capabilityMap(localTypes);
  const schema = new Map(schemaFields.map((field) => [field.name, schemaField(field)] as const));
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

function schemaField(field: DocumentSchemaField): DocumentEditorField {
  return field;
}

function capabilityMap(localTypes: RuleLocalDocumentTypeCapability[]) {
  return new Map(localTypes.map((capability) => [capability.value_type, capability]));
}

function localField(path: string, type: RuleLocalDocumentValueType, capability: RuleLocalDocumentTypeCapability): DocumentEditorField {
  return { name: path, label: path || "/", type, itemTemplate: false, predicates: capability.predicates, actions: capability.actions };
}

function conditionValueType(condition: Extract<Condition, { source: "document" | "document_pattern" }>): RuleLocalDocumentValueType {
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
