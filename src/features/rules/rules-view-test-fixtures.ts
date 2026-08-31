import type { ProxyListener, RuleEditorContext } from "@/generated/rust-types";

export const localDocumentTypes = [
  { value_type: "string", predicates: ["equals", "contains", "starts_with", "ends_with"], actions: ["set", "clear", "insert", "append"] },
  { value_type: "number", predicates: ["equals", "less", "less_equal", "greater", "greater_equal"], actions: ["set", "clear", "insert", "append"] },
  { value_type: "boolean", predicates: ["equals"], actions: ["set", "clear", "insert", "append"] },
  { value_type: "null", predicates: ["equals"], actions: ["set", "clear", "insert", "append"] },
  { value_type: "object", predicates: [], actions: ["set", "clear", "insert", "append"] },
  { value_type: "array", predicates: [], actions: ["set", "clear", "insert", "append"] },
] as RuleEditorContext["local_document_types"];

export function testListener(id: string, name: string, kind: "http" | "socket"): ProxyListener {
  return {
    id, name, enabled: true, bind_address: "127.0.0.1", port: kind === "http" ? 8080 : 9000,
    connect_timeout_ms: 1_000, read_timeout_ms: 1_000, write_timeout_ms: 1_000,
    data_plane: kind === "http"
      ? { kind: "http", settings: { body_processing: { mode: "plain" } } }
      : { kind: "socket", settings: { processing: { mode: "direct" } } },
  } as ProxyListener;
}

export function withOptionalHttpDocument(context: RuleEditorContext): RuleEditorContext {
  const stage = context.content.type === "http" ? context.content.value.stages[0] : undefined;
  if (!stage) throw new Error("HTTP context fixture is invalid");
  return {
    ...context,
    content: { type: "http", value: { stages: [{
      ...stage,
      document_fields: [{ name: "/amount", label: "Amount", type: "number", operators: ["equals"], actions: ["set_field", "clear_field"] }],
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
      fields: [{ name: "/amount", label: "Amount", type: "number", operators: ["equals"], actions: ["set_field", "clear_field"] }],
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
        stage: "response",
        match_field_kinds: ["path_or_request_type", "json_path"],
        actions: actionKinds.map((kind) => ({ kind, terminal: false, traffic_direction: null })),
      },
      new_rule_draft: {
        ...stage.new_rule_draft,
        draft: { ...stage.new_rule_draft.draft, stage: "proxy_to_app" },
      },
    }] } },
  };
}
