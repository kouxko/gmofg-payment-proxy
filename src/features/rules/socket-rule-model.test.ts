import { describe, expect, it, vi } from "vitest";
import type {
  ProxyListener,
  ProtocolRuleCapabilityCatalog,
} from "@/generated/rust-types";
import {
  capabilityCompatible,
  conditionFor,
  deleteResponseMatches,
  draftFromRule,
  emptyValue,
  isDocumentValueForType,
  isProtocolRuleDefinition,
  listenerStages,
  newProtocolRuleDraft,
  parseProtocolRuleValue,
  protocolRuleListeners,
  saveResponseMatches,
  setActionFor,
  toggleResponseMatches,
  validateCapabilityCatalog,
  validateProtocolRuleDraft,
  valueText,
} from "./protocol-rule-model";
import { defaultSocketRuntimeLimits } from "@/features/listeners/listener-data-plane";

const commandMocks = vi.hoisted(() => ({ protocolRuleParseValue: vi.fn() }));
vi.mock("@/generated/rust-types", () => ({ commands: commandMocks }));
vi.mock("@/lib/ipc/client", () => ({ callCommand: async <T,>(value: Promise<T> | T) => value }));

const packageRef = { id: "iso8583", version: "1.2.3" };

function listener(
  id: string,
  options: {
    kind?: "http" | "socket";
    scripted?: boolean;
    local?: boolean;
  } = {},
): ProxyListener {
  const common = {
    id,
    name: id,
    enabled: true,
    bind_address: "127.0.0.1",
    port: 9000,
    allowed_client_cidrs: [],
    connect_timeout_ms: 1_000,
    read_timeout_ms: 1_000,
    write_timeout_ms: 1_000,
  };
  if (options.kind === "http") {
    return {
      ...common,
      data_plane: {
        kind: "http",
        settings: {
          body_processing: options.scripted === false
            ? { mode: "plain" }
            : { mode: "protocol", package: packageRef },
        },
      },
    } as unknown as ProxyListener;
  }
  return {
    ...common,
    data_plane: {
      kind: "socket",
      settings: {
        topology: options.local
          ? { mode: "local_responder", settings: { downstream_security: { mode: "tcp" } } }
          : {
              mode: "relay",
              settings: {
                upstream: { host: "example.test", port: 9001 },
                security: { mode: "transparent" },
              },
            },
        maximum_connections: 8,
        runtime_limits: defaultSocketRuntimeLimits(),
        processing: options.scripted === false
          ? { mode: "direct" }
          : {
              mode: "scripted",
              settings: {
                package: packageRef,
              },
            },
      },
    },
  };
}

const catalog: ProtocolRuleCapabilityCatalog = {
  package: packageRef,
  schema_version: 7,
  stage: "app_to_proxy",
  fields: [
    { name: "message_type", label: "消息类型", type: "string", operators: ["equals"], actions: ["set_field"] },
    { name: "amount", label: "金额", type: "int", operators: ["equals"], actions: ["set_field"] },
    { name: "approved", label: "批准", type: "bool", operators: ["equals"], actions: ["set_field"] },
    { name: "bitmap", label: "位图", type: "blob", operators: ["equals"], actions: ["set_field"] },
  ],
  common_actions: ["record_match", "clear_document"],
};

describe("protocol document rule model", () => {
  it("keeps HTTP protocol and Socket protocol entries in separate workspaces", () => {
    const listeners = [
      listener("relay"),
      listener("local", { local: true }),
      listener("direct", { scripted: false }),
      listener("http", { kind: "http" }),
      listener("plain-http", { kind: "http", scripted: false }),
    ];
    expect(protocolRuleListeners(listeners, "socket").map((item) => item.id))
      .toEqual(["relay", "local"]);
    expect(protocolRuleListeners(listeners, "http").map((item) => item.id))
      .toEqual(["http"]);
  });

  it("offers all four stages for Relay and the two app-facing stages for LocalResponder", () => {
    expect(listenerStages(listener("relay"))).toEqual(["app_to_proxy", "proxy_to_upstream", "upstream_to_proxy", "proxy_to_app"]);
    expect(listenerStages(listener("local", { local: true }))).toEqual(["app_to_proxy", "proxy_to_app"]);
  });

  it("offers all four stages for an HTTP protocol entry", () => {
    expect(listenerStages(listener("http", { kind: "http" }))).toEqual([
      "app_to_proxy", "proxy_to_upstream", "upstream_to_proxy", "proxy_to_app",
    ]);
  });

  it("creates a bound empty rule with one RecordMatch action", () => {
    expect(newProtocolRuleDraft(listener("relay"), "app_to_proxy", catalog)).toEqual({
      rule_id: null,
      expected_revision: null,
      name: "新规则",
      enabled: true,
      priority: 100,
      listener_id: "relay",
      package: packageRef,
      schema_version: 7,
      stage: "app_to_proxy",
      conditions: [],
      actions: [{ type: "record_match" }],
    });
  });

  it("uses RecordMatch for a new HTTP protocol rule", () => {
    expect(newProtocolRuleDraft(listener("http", { kind: "http" }), "proxy_to_app", {
      ...catalog,
      stage: "proxy_to_app",
    }).actions).toEqual([{ type: "record_match" }]);
  });

  it("preserves an existing rule identity and revision in its editable draft", () => {
    const draft = draftFromRule({
      rule_id: "rule-1",
      revision: 9,
      name: "现有规则",
      enabled: false,
      priority: 20,
      created_order: 3,
      listener_id: "relay",
      package: packageRef,
      schema_version: 7,
      stage: "app_to_proxy",
      conditions: [],
      actions: [{ type: "record_match" }],
    });
    expect(draft.rule_id).toBe("rule-1");
    expect(draft.expected_revision).toBe(9);
  });

  it.each([
    [catalog.fields[0], { type: "string", value: "" }],
    [catalog.fields[1], { type: "int", value: 0 }],
    [catalog.fields[2], { type: "bool", value: false }],
    [catalog.fields[3], { type: "blob", value: [] }],
  ] as const)("creates the correct empty value for $name", (field, expected) => {
    expect(emptyValue(field)).toEqual(expected);
    expect(conditionFor(field)).toEqual({ operator: "equals", field: field.name, value: expected });
    expect(setActionFor(field)).toEqual({ type: "set_field", field: field.name, value: expected });
  });

  it("delegates typed value parsing to the generated Rust command", async () => {
    commandMocks.protocolRuleParseValue.mockResolvedValue({ type: "blob", value: [1, 160, 255] });
    await expect(parseProtocolRuleValue("blob", "01 A0 FF")).resolves.toEqual({ type: "blob", value: [1, 160, 255] });
    expect(commandMocks.protocolRuleParseValue).toHaveBeenCalledWith("blob", "01 A0 FF");
  });

  it.each([
    [{ type: "string", value: "0200" }, "0200"],
    [{ type: "int", value: 100 }, "100"],
    [{ type: "bool", value: true }, "true"],
  ] as const)("formats a non-Blob typed value as editable text", (value, expected) => {
    expect(valueText(value)).toBe(expected);
  });

  it("formats Blob bytes as uppercase editable hex", () => {
    expect(valueText({ type: "blob", value: [1, 160, 255] })).toBe("01 A0 FF");
  });

  it.each([
    [null, "数据无效"],
    [{ ...catalog, package: null }, "协议包身份"],
    [{ ...catalog, schema_version: -1 }, "字段结构版本"],
    [{ ...catalog, stage: "sideways" }, "未知处理阶段"],
    [{ ...catalog, fields: null }, "字段或动作目录"],
    [{ ...catalog, fields: [null] }, "无效字段"],
    [{ ...catalog, fields: [{ ...catalog.fields[0], operators: null }] }, "字段能力目录"],
    [{ ...catalog, fields: [catalog.fields[0], catalog.fields[0]] }, "重复字段"],
    [{ ...catalog, fields: [{ ...catalog.fields[0], type: "float" }] }, "未知字段类型"],
    [{ ...catalog, fields: [{ ...catalog.fields[0], operators: ["contains"] }] }, "未知操作符"],
    [{ ...catalog, fields: [{ ...catalog.fields[0], actions: ["append"] }] }, "未知字段动作"],
    [{ ...catalog, common_actions: ["stop"] }, "未知公共动作"],
  ])("rejects malformed or future capability catalogs", (candidate, message) => {
    expect(validateCapabilityCatalog(candidate as ProtocolRuleCapabilityCatalog)).toContain(message);
  });

  it("accepts the current capability catalog vocabulary", () => {
    expect(validateCapabilityCatalog(catalog)).toBeUndefined();
  });

  it("accepts a complete rule response shape", () => {
    expect(isProtocolRuleDefinition({
      rule_id: "rule-1", revision: 1, name: "金额规则", enabled: true, priority: 10, created_order: 1,
      listener_id: "relay", package: packageRef, schema_version: 7, stage: "app_to_proxy",
      conditions: [{ operator: "equals", field: "amount", value: { type: "int", value: 100 } }],
      actions: [{ type: "set_field", field: "approved", value: { type: "bool", value: true } }],
    })).toBe(true);
  });

  it("requires save responses to persist the exact draft and advance revision once", () => {
    const create = newProtocolRuleDraft(listener("relay"), "app_to_proxy", catalog);
    const created = {
      ...create, rule_id: "rule-1", revision: 1, created_order: 9,
    };
    expect(saveResponseMatches(created, create)).toBe(true);
    expect(saveResponseMatches({ ...created, revision: 2 }, create)).toBe(false);
    expect(saveResponseMatches({ ...created, priority: 101 }, create)).toBe(false);
    expect(saveResponseMatches({ ...created, actions: [{ type: "clear_document" }] }, create)).toBe(false);

    const update = { ...create, rule_id: "rule-1", expected_revision: 7, enabled: false };
    const previous = { ...created, revision: 7 };
    expect(saveResponseMatches({ ...created, revision: 8, enabled: false }, update, previous)).toBe(true);
    expect(saveResponseMatches({ ...created, rule_id: "other", revision: 8, enabled: false }, update, previous)).toBe(false);
    expect(saveResponseMatches({ ...created, revision: 8, enabled: false, created_order: 10 }, update, previous)).toBe(false);
  });

  it("requires toggle responses to change only enabled and revision", () => {
    const before = {
      rule_id: "rule-1", revision: 3, name: "启停规则", enabled: true, priority: 10, created_order: 4,
      listener_id: "relay", package: packageRef, schema_version: 7, stage: "app_to_proxy" as const,
      conditions: [], actions: [{ type: "record_match" as const }],
    };
    expect(toggleResponseMatches({ ...before, revision: 4, enabled: false }, before, false)).toBe(true);
    expect(toggleResponseMatches({ ...before, revision: 5, enabled: false }, before, false)).toBe(false);
    expect(toggleResponseMatches({ ...before, revision: 4, enabled: false, created_order: 5 }, before, false)).toBe(false);
    expect(toggleResponseMatches({ ...before, revision: 4, enabled: false, conditions: [{ operator: "equals", field: "amount", value: { type: "int", value: 1 } }] }, before, false)).toBe(false);
  });

  it("accepts only a confirmed successful delete result for the exact rule", () => {
    const result = { success: true, cancelled: false, message: "deleted", ui_tone: "positive", entity_id: "rule-1", revision: 4, requires_restart: false };
    expect(deleteResponseMatches(result, "rule-1")).toBe(true);
    expect(deleteResponseMatches({ ...result, success: false }, "rule-1")).toBe(false);
    expect(deleteResponseMatches({ ...result, cancelled: true }, "rule-1")).toBe(false);
    expect(deleteResponseMatches({ ...result, entity_id: "other" }, "rule-1")).toBe(false);
    expect(deleteResponseMatches({ ...result, revision: 0 }, "rule-1")).toBe(false);
    expect(deleteResponseMatches({ ...result, ui_tone: "success" }, "rule-1")).toBe(false);
    expect(deleteResponseMatches({ ...result, message: undefined }, "rule-1")).toBe(false);
    expect(deleteResponseMatches({ ...result, requires_restart: undefined }, "rule-1")).toBe(false);
  });

  it.each([
    null,
    {},
    { rule_id: "", revision: 1 },
    { rule_id: "rule", revision: 0, priority: 1, created_order: 1, enabled: true, listener_id: "listener", package: packageRef, schema_version: 1, stage: "app_to_proxy", conditions: [], actions: [{ type: "record_match" }] },
    { rule_id: "rule", revision: 1, priority: 1, created_order: 0, enabled: true, listener_id: "listener", package: packageRef, schema_version: 1, stage: "app_to_proxy", conditions: [], actions: [{ type: "record_match" }] },
    { rule_id: "rule", revision: 1, priority: 1, created_order: 1, enabled: true, listener_id: "listener", package: packageRef, schema_version: 0, stage: "app_to_proxy", conditions: [], actions: [{ type: "record_match" }] },
    { rule_id: "rule", revision: 1, priority: 1, created_order: 1, enabled: true, listener_id: "listener", package: packageRef, schema_version: 1, stage: "sideways", conditions: [], actions: [{ type: "record_match" }] },
    { rule_id: "rule", revision: 1, priority: 1, created_order: 1, enabled: true, listener_id: "listener", package: packageRef, schema_version: 1, stage: "app_to_proxy", conditions: [null], actions: [{ type: "record_match" }] },
    { rule_id: "rule", revision: 1, priority: 1, created_order: 1, enabled: true, listener_id: "listener", package: packageRef, schema_version: 1, stage: "app_to_proxy", conditions: [], actions: [{ type: "unknown" }] },
  ])("rejects malformed rule response payloads", (candidate) => {
    expect(isProtocolRuleDefinition(candidate)).toBe(false);
  });

  it("validates Document values against the selected field type", () => {
    expect(isDocumentValueForType({ type: "string", value: "0200" }, "string")).toBe(true);
    expect(isDocumentValueForType({ type: "string", value: "0200" }, "int")).toBe(false);
    expect(isDocumentValueForType({ type: "int", value: Number.MAX_SAFE_INTEGER + 1 }, "int")).toBe(false);
    expect(isDocumentValueForType({ type: "bool", value: "true" }, "bool")).toBe(false);
    expect(isDocumentValueForType({ type: "blob", value: [0, 256] }, "blob")).toBe(false);
  });

  it("accepts an empty Schema with a no-condition RecordMatch rule", () => {
    const emptyCatalog = { ...catalog, fields: [], common_actions: ["record_match" as const] };
    const draft = newProtocolRuleDraft(listener("relay"), "app_to_proxy", emptyCatalog);
    expect(validateProtocolRuleDraft(draft, emptyCatalog)).toBeUndefined();
  });

  it("rejects stale package, schema, or stage bindings", () => {
    const draft = newProtocolRuleDraft(listener("relay"), "app_to_proxy", catalog);
    expect(validateProtocolRuleDraft({ ...draft, package: { ...packageRef, id: "other" } }, catalog)).toContain("绑定");
    expect(validateProtocolRuleDraft({ ...draft, package: { ...packageRef, version: "9.9.9" } }, catalog)).toContain("绑定");
    expect(validateProtocolRuleDraft({ ...draft, schema_version: 6 }, catalog)).toContain("绑定");
    expect(capabilityCompatible({ ...draft, stage: "proxy_to_app" }, catalog)).toBe(false);
  });

  it("rejects an unknown condition field, operator, or typed value", () => {
    const draft = newProtocolRuleDraft(listener("relay"), "app_to_proxy", catalog);
    expect(validateProtocolRuleDraft({ ...draft, conditions: [{ operator: "equals", field: "missing", value: { type: "string", value: "x" } }] }, catalog)).toContain("条件");
    expect(validateProtocolRuleDraft({ ...draft, conditions: [{ operator: "contains", field: "message_type", value: { type: "string", value: "x" } } as never] }, catalog)).toContain("条件");
    expect(validateProtocolRuleDraft({ ...draft, conditions: [{ operator: "equals", field: "amount", value: { type: "string", value: "100" } }] }, catalog)).toContain("类型或大小");
  });

  it("rejects duplicate condition fields", () => {
    const draft = newProtocolRuleDraft(listener("relay"), "app_to_proxy", catalog);
    const condition = conditionFor(catalog.fields[0]);
    expect(validateProtocolRuleDraft({ ...draft, conditions: [condition, condition] }, catalog)).toContain("重复条件");
  });

  it("rejects actions omitted from the capability catalog", () => {
    const recordOnly = {
      ...catalog,
      fields: catalog.fields.map((field) => ({ ...field, actions: [] })),
      common_actions: ["record_match" as const],
    };
    const draft = newProtocolRuleDraft(listener("relay"), "app_to_proxy", recordOnly);
    expect(validateProtocolRuleDraft({ ...draft, actions: [{ type: "clear_document" }] }, recordOnly)).toContain("清空全部字段");
    expect(validateProtocolRuleDraft({ ...draft, actions: [setActionFor(catalog.fields[0])] }, recordOnly)).toContain("设置字段");
  });

  it("rejects RecordMatch when it is absent and unknown action tags", () => {
    const noCommon = { ...catalog, common_actions: ["clear_document" as const] };
    const draft = newProtocolRuleDraft(listener("relay"), "app_to_proxy", noCommon);
    expect(validateProtocolRuleDraft(draft, noCommon)).toContain("RecordMatch");
    expect(validateProtocolRuleDraft({ ...draft, actions: [{ type: "stop" }] as never }, catalog)).toContain("未知动作");
  });

  it("rejects out-of-range Blob bytes in conditions and actions", () => {
    const draft = newProtocolRuleDraft(listener("relay"), "app_to_proxy", catalog);
    const invalidBlob = { type: "blob" as const, value: [256] };
    expect(validateProtocolRuleDraft({ ...draft, conditions: [{ operator: "equals", field: "bitmap", value: invalidBlob }] }, catalog)).toContain("类型或大小");
    expect(validateProtocolRuleDraft({ ...draft, actions: [{ type: "set_field", field: "bitmap", value: invalidBlob }] }, catalog)).toContain("设置字段");
  });
});
