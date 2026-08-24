import type {
  ProxyListener,
  ProtocolDocumentRuleDefinition,
  ProtocolRuleCapabilityCatalog,
} from "@/generated/rust-types";
import { defaultSocketRuntimeLimits } from "@/features/listeners/listener-data-plane";

export const packageRef = { id: "iso8583", version: "1.2.3" };

export function socketListener(id: string, mode: "relay" | "local" | "direct" = "relay"): ProxyListener {
  return {
    id,
    name: id,
    enabled: true,
    bind_address: "127.0.0.1",
    port: 9000,
    allowed_client_cidrs: [],
    connect_timeout_ms: 1_000,
    read_timeout_ms: 1_000,
    write_timeout_ms: 1_000,
    data_plane: {
      kind: "socket",
      settings: {
        topology: mode === "local"
          ? { mode: "local_responder", settings: { downstream_security: { mode: "tcp" } } }
          : { mode: "relay", settings: { upstream: { host: "example.test", port: 9001 }, security: { mode: "transparent" } } },
        maximum_connections: 8,
        runtime_limits: defaultSocketRuntimeLimits(),
        processing: mode === "direct" ? { mode: "direct" } : {
          mode: "scripted",
          settings: {
            package: packageRef,
          },
        },
      },
    },
  };
}

export function httpListener(id = "http", protocol = true): ProxyListener {
  return {
    ...socketListener(id),
    id,
    data_plane: {
      kind: "http",
      settings: {
        body_processing: protocol
          ? { mode: "protocol", package: packageRef }
          : { mode: "plain" },
      },
    },
  } as unknown as ProxyListener;
}

export function capability(
  stage: ProtocolRuleCapabilityCatalog["stage"],
  schemaVersion = 7,
): ProtocolRuleCapabilityCatalog {
  return {
    package: packageRef,
    schema_version: schemaVersion,
    stage,
    fields: [
      { name: "message_type", label: "消息类型", type: "string", operators: ["equals"], actions: ["set_field"] },
      { name: "amount", label: "金额", type: "int", operators: ["equals"], actions: ["set_field"] },
    ],
    common_actions: ["record_match", "clear_document"],
  };
}

export function rule(revision = 3): ProtocolDocumentRuleDefinition {
  return {
    rule_id: "rule-1",
    revision,
    name: "测试规则",
    enabled: true,
    priority: 10,
    created_order: 1,
    listener_id: "relay",
    package: packageRef,
    schema_version: 7,
    stage: "app_to_proxy",
    conditions: [{ operator: "equals", field: "message_type", value: { type: "string", value: "0200" } }],
    actions: [{ type: "record_match" }],
  };
}

export function savedFromInput(input: Record<string, unknown>) {
  return {
    ...input,
    rule_id: input.rule_id ?? "rule-1",
    revision: input.expected_revision == null ? 1 : Number(input.expected_revision) + 1,
    created_order: 1,
  };
}

export function deleted(ruleId = "rule-1", revision = 4) {
  return {
    success: true,
    cancelled: false,
    message: "deleted",
    ui_tone: "positive",
    entity_id: ruleId,
    revision,
    requires_restart: false,
  };
}
