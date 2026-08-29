import { describe, expect, it } from "vitest";
import type { RuleDefinitionSaveInput, RuleDefinition_Serialize, RuleEditorContext } from "@/generated/rust-types";
import { groupRulesByStage, ruleStageIncompatibility, RULE_STAGE_ORDER } from "./rule-definition-model";

function rule(stage: RuleDefinition_Serialize["stage"], priority: number, createdOrder: number): RuleDefinition_Serialize {
  return {
    rule_id: `${stage}-${priority}`, revision: 1, name: stage, enabled: true, priority,
    created_order: createdOrder, listener_id: "listener", stage,
    content: { type: "socket", value: { package: { id: "pkg", version: "1" }, conditions: [], actions: [] } },
  };
}

describe("groupRulesByStage", () => {
  it("keeps pipeline stages fixed and sorts ascending priority only inside each stage", () => {
    const grouped = groupRulesByStage([
      rule("proxy_to_app", 999, 1), rule("app_to_proxy", 10, 2),
      rule("app_to_proxy", 20, 4), rule("app_to_proxy", 20, 3),
    ]);
    expect(grouped.map((group) => group.stage)).toEqual(RULE_STAGE_ORDER);
    expect(grouped[0].rules.map((item) => [item.priority, item.created_order])).toEqual([
      [10, 2], [20, 3], [20, 4],
    ]);
    expect(grouped[3].rules[0].priority).toBe(999);
  });
});

describe("ruleStageIncompatibility", () => {
  it("rejects an HTTP Document payload when the target capability cannot edit its field action", () => {
    const input: RuleDefinitionSaveInput = {
      rule_id: "rule", expected_revision: 1,
      draft: {
        name: "document", enabled: true, priority: 1, listener_id: "listener", stage: "proxy_to_upstream",
        content: { type: "http", value: {
          description: "", conditions: [], actions: [], one_shot: false, hit_count: 0, last_hit_at: null,
          document: {
            package: { id: "pkg", version: "1" }, conditions: [],
            actions: [{ type: "set_field", field: "/amount", value: 1 }],
          },
        } },
      },
    };
    const context: RuleEditorContext = {
      listener_id: "listener",
      content: { type: "http", value: { stages: [{
        stage: "proxy_to_app", http: null, package: { id: "pkg", version: "1" },
        document_fields: [], document_common_actions: [],
        new_rule_draft: { ...input, rule_id: null, expected_revision: null, draft: { ...input.draft, stage: "proxy_to_app" } },
      }] } },
    };

    expect(ruleStageIncompatibility(input, context, "proxy_to_app")).toBe(
      "目标阶段不能编辑 Document 动作字段 /amount。",
    );
  });

  it("treats the JSON string Null as a string and actual null as null", () => {
    const base: RuleDefinitionSaveInput = {
      rule_id: "rule", expected_revision: 1,
      draft: {
        name: "document", enabled: true, priority: 1, listener_id: "listener", stage: "proxy_to_app",
        content: { type: "socket", value: {
          package: { id: "pkg", version: "1" },
          conditions: [{ operator: "equals", field: "/value", value: "Null" }], actions: [],
        } },
      },
    };
    const context: RuleEditorContext = {
      listener_id: "listener",
      content: { type: "socket", value: {
        package: { id: "pkg", version: "1" },
        stages: [{
          stage: "proxy_to_app",
          fields: [{ name: "/value", label: "Value", type: "string", operators: ["equals"], actions: [] }],
          common_actions: [],
          new_rule_draft: { ...base, rule_id: null, expected_revision: null },
        }],
      } },
    };

    expect(ruleStageIncompatibility(base, context, "proxy_to_app")).toBeNull();
    const actualNull = structuredClone(base);
    if (actualNull.draft.content.type !== "socket") throw new Error("invalid fixture");
    actualNull.draft.content.value.conditions[0].value = null;
    expect(ruleStageIncompatibility(actualNull, context, "proxy_to_app")).toBe(
      "目标阶段不能编辑 Document 条件字段 /value。",
    );
  });
});
