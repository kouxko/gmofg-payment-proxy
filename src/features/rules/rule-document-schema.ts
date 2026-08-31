import type {
  RuleDocumentActionCapability,
  RuleDocumentSchemaFieldCapability,
  RuleLocalDocumentPredicateKind,
  RuleLocalDocumentValueType,
} from "@/generated/rust-types";

export type DocumentSchemaField = {
  name: string;
  label: string;
  type: RuleLocalDocumentValueType;
  itemTemplate: boolean;
  predicates: RuleLocalDocumentPredicateKind[];
  actions: RuleDocumentActionCapability[];
};

export function documentSchemaFields(
  capabilities: RuleDocumentSchemaFieldCapability[],
): DocumentSchemaField[] {
  return capabilities.map((capability) => ({
    name: capability.path,
    label: capability.label,
    type: capability.value_type,
    itemTemplate: capability.item_template,
    predicates: capability.predicates,
    actions: capability.actions,
  }));
}
