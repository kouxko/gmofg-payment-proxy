import { describe, expect, it } from "vitest";
import type { RuleDefinitionSaveInput, RuleDefinition_Serialize, RuleEditorContext } from "@/generated/rust-types";
import { ruleDirectionLabel, ruleStageIncompatibility } from "./rule-definition-model";

const documentAction = (kind: "set" | "clear" | "insert" | "append", target: "string" | "array", operand: "string" | null) => ({
  kind,
  target_kind: kind === "insert" || kind === "append" ? "array" as const : "node" as const,
  target_value_type: target,
  operand_value_type: operand,
});

const stringCondition = (path = "/value", value = "value") => ({
  source: "document" as const, path, predicate: { type: "string" as const, value: { operator: "equal" as const, value } },
});
const lifecycle = { hit_count: 0, last_hit_at: null };

function rule(stage: RuleDefinition_Serialize["stage"], priority: number, createdOrder: number): RuleDefinition_Serialize {
  return {
    rule_id: `${stage}-${createdOrder}`, revision: 1, name: stage, enabled: true, priority,
    created_order: createdOrder, listener_id: "listener", stage, lifecycle,
    content: { type: "socket", value: { package: { id: "pkg", version: "1" }, condition: stringCondition(), action: { source: "record_match" } } },
  };
}

describe("ruleDirectionLabel", () => {
  it("labels the two message directions shown in the unified rule list", () => {
    expect(ruleDirectionLabel("proxy_to_upstream")).toBe("上行");
    expect(ruleDirectionLabel("proxy_to_app")).toBe("下行");
  });
});

describe("ruleStageIncompatibility", () => {
  it("keeps a Schema-undeclared Document action as rule-local metadata", () => {
    const input: RuleDefinitionSaveInput = {
      rule_id: "rule", expected_revision: 1,
      draft: {
        name: "document", enabled: true, priority: 1, listener_id: "listener", stage: "proxy_to_upstream",
        content: { type: "http", value: {
          description: "", condition: stringCondition(), action: { source: "document", value: { type: "set", path: "/amount", value: 1 } },
        } },
      },
    };
    const context: RuleEditorContext = {
      listener_id: "listener",
      local_document_types: [{ value_type: "number", predicates: [], actions: [{ kind: "set", target_kind: "node", target_value_type: "number", operand_value_type: "number" }] }],
      document_condition_path: { wildcard_token: "*", wildcard_matches_exactly_one_level: true, multiple_matches_use_any: true },
      content: { type: "http", value: { stages: [{
        stage: "proxy_to_app", http: null, package: { id: "pkg", version: "1" },
        document_common_actions: [],
        document_fields: [{ path: "/value", label: "Value", value_type: "string", item_template: false, predicates: ["equals"], actions: [documentAction("set", "string", "string"), documentAction("clear", "string", null)] }],
        new_rule_draft: { listener_id: input.draft.listener_id, stage: "proxy_to_app", content: { type: "http", value: { description: "" } } },
      }] } },
    };

    expect(ruleStageIncompatibility(input, context, "proxy_to_app")).toBeNull();
  });

  it("treats the JSON string Null as a string and actual null as null", () => {
    const base: RuleDefinitionSaveInput = {
      rule_id: "rule", expected_revision: 1,
      draft: {
        name: "document", enabled: true, priority: 1, listener_id: "listener", stage: "proxy_to_app",
        content: { type: "socket", value: {
          package: { id: "pkg", version: "1" },
          condition: stringCondition("/value", "Null"), action: { source: "record_match" },
        } },
      },
    };
    const context: RuleEditorContext = {
      listener_id: "listener",
      local_document_types: [],
      document_condition_path: { wildcard_token: "*", wildcard_matches_exactly_one_level: true, multiple_matches_use_any: true },
      content: { type: "socket", value: {
        package: { id: "pkg", version: "1" },
        stages: [{
          stage: "proxy_to_app",
          document_fields: [{ path: "/value", label: "Value", value_type: "string", item_template: false, predicates: ["equals"], actions: [documentAction("set", "string", "string"), documentAction("clear", "string", null)] }],
          common_actions: ["record_match"],
          new_rule_draft: { listener_id: base.draft.listener_id, stage: base.draft.stage, content: { type: "socket", value: { package: { id: "pkg", version: "1" } } } },
        }],
      } },
    };

    expect(ruleStageIncompatibility(base, context, "proxy_to_app")).toBeNull();
    const actualNull = structuredClone(base);
    if (actualNull.draft.content.type !== "socket") throw new Error("invalid fixture");
    actualNull.draft.content.value.condition = { source: "document", path: "/value", predicate: { type: "null_equal" } };
    expect(ruleStageIncompatibility(actualNull, context, "proxy_to_app")).toBe(
      "目标阶段不能编辑 Document 条件字段 /value。",
    );
  });

  it("accepts a Rust-factory Insert on an undeclared rule-local path", () => {
    const input: RuleDefinitionSaveInput = {
      rule_id: "rule", expected_revision: 1,
      draft: {
        name: "local array", enabled: true, priority: 1, listener_id: "listener", stage: "proxy_to_app",
        content: { type: "socket", value: {
          package: { id: "pkg", version: "1" },
          condition: stringCondition(),
          action: { source: "document", value: { type: "insert", path: "/items", index: 0, value: "first" } },
        } },
      },
    };
    const context: RuleEditorContext = {
      listener_id: "listener",
      local_document_types: [{ value_type: "string", predicates: ["equals"], actions: [documentAction("insert", "array", "string"), documentAction("append", "array", "string")] }],
      document_condition_path: { wildcard_token: "*", wildcard_matches_exactly_one_level: true, multiple_matches_use_any: true },
      content: { type: "socket", value: { package: { id: "pkg", version: "1" }, stages: [{
        stage: "proxy_to_app", document_fields: [], common_actions: [],
        new_rule_draft: { listener_id: input.draft.listener_id, stage: input.draft.stage, content: { type: "socket", value: { package: { id: "pkg", version: "1" } } } },
      }] } },
    };

    expect(ruleStageIncompatibility(input, context, "proxy_to_app")).toBeNull();
  });

  it("does not apply schema-free array actions to a declared scalar path", () => {
    const input: RuleDefinitionSaveInput = {
      rule_id: "rule", expected_revision: 1,
      draft: {
        name: "schema action", enabled: true, priority: 1, listener_id: "listener", stage: "proxy_to_app",
        content: { type: "socket", value: {
          package: { id: "pkg", version: "1" },
          condition: stringCondition(),
          action: { source: "document", value: { type: "insert", path: "/name", index: 0, value: "first" } },
        } },
      },
    };
    const context: RuleEditorContext = {
      listener_id: "listener",
      local_document_types: [{ value_type: "string", predicates: ["equals"], actions: [documentAction("insert", "array", "string")] }],
      document_condition_path: { wildcard_token: "*", wildcard_matches_exactly_one_level: true, multiple_matches_use_any: true },
      content: { type: "socket", value: { package: { id: "pkg", version: "1" }, stages: [{
        stage: "proxy_to_app",
        document_fields: [{ path: "/name", label: "Name", value_type: "string", item_template: false, predicates: ["equals"], actions: [documentAction("set", "string", "string"), documentAction("clear", "string", null)] }],
        common_actions: [], new_rule_draft: { listener_id: input.draft.listener_id, stage: input.draft.stage, content: { type: "socket", value: { package: { id: "pkg", version: "1" } } } },
      }] } },
    };

    expect(ruleStageIncompatibility(input, context, "proxy_to_app")).toBe(
      "目标阶段不能编辑 Document 动作字段 /name。",
    );

    if (input.draft.content.type !== "socket") throw new Error("invalid fixture");
    input.draft.content.value.action = { source: "document", value: { type: "insert", path: "/items", index: 0, value: "first" } };
    if (context.content.type !== "socket") throw new Error("invalid context");
    context.content.value.stages[0].document_fields = [{
      path: "/items", label: "Items", value_type: "array", item_template: false, predicates: [],
      actions: [documentAction("insert", "array", "string")],
    }];
    expect(ruleStageIncompatibility(input, context, "proxy_to_app")).toBeNull();
  });

  it("rejects schema-free Insert when either the target descriptor or operand descriptor differs", () => {
    const input: RuleDefinitionSaveInput = {
      rule_id: "rule", expected_revision: 1,
      draft: {
        name: "local array", enabled: true, priority: 1, listener_id: "listener", stage: "proxy_to_app",
        content: { type: "socket", value: {
          package: { id: "pkg", version: "1" },
          condition: stringCondition(),
          action: { source: "document", value: { type: "insert", path: "/items", index: 0, value: "first" } },
        } },
      },
    };
    const context: RuleEditorContext = {
      listener_id: "listener",
      local_document_types: [{ value_type: "string", predicates: [], actions: [{
        kind: "insert", target_kind: "node", target_value_type: "string", operand_value_type: "string",
      }] }],
      document_condition_path: { wildcard_token: "*", wildcard_matches_exactly_one_level: true, multiple_matches_use_any: true },
      content: { type: "socket", value: { package: { id: "pkg", version: "1" }, stages: [{
        stage: "proxy_to_app", document_fields: [], common_actions: [],
        new_rule_draft: { listener_id: input.draft.listener_id, stage: input.draft.stage, content: { type: "socket", value: { package: { id: "pkg", version: "1" } } } },
      }] } },
    };

    expect(ruleStageIncompatibility(input, context, "proxy_to_app")).toBe(
      "目标阶段不能编辑 Document 动作字段 /items。",
    );
    if (context.content.type !== "socket") throw new Error("invalid context");
    context.local_document_types[0].actions = [{
      kind: "insert", target_kind: "array", target_value_type: "array", operand_value_type: "number",
    }];
    expect(ruleStageIncompatibility(input, context, "proxy_to_app")).toBe(
      "目标阶段不能编辑 Document 动作字段 /items。",
    );
  });
});

describe("unified lifecycle wire", () => {
  it("keeps lifecycle at the rule definition", () => {
    const socket = rule("proxy_to_app", 1, 1);
    expect(socket.lifecycle).toEqual(lifecycle);
  });
});
