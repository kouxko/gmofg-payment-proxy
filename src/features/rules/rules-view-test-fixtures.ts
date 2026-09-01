import type { ProxyListener, RuleDefinition_Serialize, RuleDocumentActionCapability, RuleEditorContext, RuleLocalDocumentValueType } from "@/generated/rust-types";

function localActions(valueType: RuleLocalDocumentValueType): RuleDocumentActionCapability[] {
  return [
    { kind: "set", target_kind: "node", target_value_type: valueType, operand_value_type: valueType },
    { kind: "clear", target_kind: "node", target_value_type: valueType, operand_value_type: null },
    { kind: "insert", target_kind: "array", target_value_type: "array", operand_value_type: valueType },
    { kind: "append", target_kind: "array", target_value_type: "array", operand_value_type: valueType },
  ];
}

export const localDocumentTypes = [
  { value_type: "string", predicates: ["equals", "contains", "starts_with", "ends_with"], actions: localActions("string") },
  { value_type: "number", predicates: ["equals", "less", "less_equal", "greater", "greater_equal"], actions: localActions("number") },
  { value_type: "boolean", predicates: ["equals"], actions: localActions("boolean") },
  { value_type: "null", predicates: ["equals"], actions: localActions("null") },
  { value_type: "object", predicates: [], actions: localActions("object") },
  { value_type: "array", predicates: [], actions: localActions("array") },
] satisfies RuleEditorContext["local_document_types"];

export function testListener(id: string, name: string, kind: "http" | "socket"): ProxyListener {
  return {
    id, name, enabled: true, bind_address: "127.0.0.1", port: kind === "http" ? 8080 : 9000,
    connect_timeout_ms: 1_000, read_timeout_ms: 1_000, write_timeout_ms: 1_000,
    data_plane: kind === "http"
      ? { kind: "http", settings: { body_processing: { mode: "plain" } } }
      : { kind: "socket", settings: { processing: { mode: "direct" } } },
  } as ProxyListener;
}

export const httpListener = testListener("http-listener", "HTTP Listener", "http");
export const socketListener = testListener("socket-listener", "Socket Listener", "socket");
export const httpCondition = { source: "http" as const, field: "RequestTarget" as const, operator: { Equals: "/" } };
export const documentCondition = (path = "/amount", value = 0) => ({ source: "document" as const, path, predicate: { type: "number" as const, value: { operator: "equal" as const, value } } });
const lifecycle = { hit_count: 0, last_hit_at: null };

export function httpRule(overrides: Partial<RuleDefinition_Serialize> = {}): RuleDefinition_Serialize {
  return {
    rule_id: "http-rule", revision: 3, name: "HTTP combined", enabled: true, priority: 50,
    created_order: 2, listener_id: httpListener.id, stage: "proxy_to_upstream", one_shot: false, lifecycle,
    content: { type: "http", value: { description: "headers and body", conditions: [httpCondition], actions: [{ source: "record_match" }] } },
    ...overrides,
  };
}

export function socketRule(): RuleDefinition_Serialize {
  return {
    rule_id: "socket-rule", revision: 4, name: "Socket document", enabled: true, priority: 20,
    created_order: 1, listener_id: socketListener.id, stage: "proxy_to_app", one_shot: false, lifecycle,
    content: { type: "socket", value: { package: { id: "iso8583", version: "1.0.0" }, conditions: [documentCondition()], actions: [{ source: "record_match" }] } },
  };
}

export function withOptionalHttpDocument(context: RuleEditorContext): RuleEditorContext {
  const stage = context.content.type === "http" ? context.content.value.stages[0] : undefined;
  if (!stage) throw new Error("HTTP context fixture is invalid");
  return {
    ...context,
    content: { type: "http", value: { stages: [{
      ...stage,
      document_fields: [{ path: "/amount", label: "Amount", value_type: "number", item_template: false, predicates: ["equals"], actions: [
        { kind: "set", target_kind: "node", target_value_type: "number", operand_value_type: "number" },
        { kind: "clear", target_kind: "node", target_value_type: "number", operand_value_type: null },
      ] }],
    }] } },
  } as unknown as RuleEditorContext;
}

export function withSocketFields(context: RuleEditorContext): RuleEditorContext {
  const stage = context.content.type === "socket" ? context.content.value.stages[0] : undefined;
  if (!stage) throw new Error("Socket context fixture is invalid");
  return {
    ...context,
    content: { type: "socket", value: { ...context.content.value, stages: [{
      ...stage,
      document_fields: [{ path: "/amount", label: "Amount", value_type: "number", item_template: false, predicates: ["equals"], actions: [
        { kind: "set", target_kind: "node", target_value_type: "number", operand_value_type: "number" },
        { kind: "clear", target_kind: "node", target_value_type: "number", operand_value_type: null },
      ] }],
    }] } },
  } as RuleEditorContext;
}

export function withSecondHttpStage(
  context: RuleEditorContext,
  actionKinds: Array<"set_header">,
): RuleEditorContext {
  const stage = context.content.type === "http" ? context.content.value.stages[0] : undefined;
  if (!stage) throw new Error("HTTP context fixture is invalid");
  return {
    ...context,
    content: { type: "http", value: { stages: [stage, {
      ...stage,
      stage: "proxy_to_app",
      http: {
        stage: "proxy_to_app",
        match_fields: stage.http?.match_fields ?? [],
        actions: actionKinds.map((kind) => ({ kind, terminal: false, traffic_direction: null, parameters_required: true })),
      },
      new_rule_draft: {
        ...stage.new_rule_draft,
        stage: "proxy_to_app",
      },
    }] } },
  };
}
