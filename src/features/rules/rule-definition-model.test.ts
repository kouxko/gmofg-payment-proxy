import { describe, expect, it } from "vitest";
import type { RuleDefinitionSaveInput, RuleDefinition_Serialize, RuleEditorContext } from "@/generated/rust-types";
import { groupRulesByStage, NEW_MESSAGE_RULE_STAGES, ruleStageIncompatibility, RULE_STAGE_ORDER } from "./rule-definition-model";

const stringCondition = (path = "/value", value = "value") => ({
  operator: "leaf" as const,
  children: { source: "document" as const, path, predicate: { type: "string" as const, value: { operator: "equal" as const, value } } },
});
const lifecycle = { hit_count: 0, last_hit_at: null };

function rule(stage: RuleDefinition_Serialize["stage"], priority: number, createdOrder: number): RuleDefinition_Serialize {
  return {
    rule_id: `${stage}-${createdOrder}`, revision: 1, name: stage, enabled: true, priority,
    created_order: createdOrder, listener_id: "listener", stage, one_shot: false, lifecycle,
    content: { type: "socket", value: { package: { id: "pkg", version: "1" }, condition: stringCondition(), actions: [{ source: "record_match" }] } },
  };
}

describe("groupRulesByStage", () => {
  it("keeps display groups fixed and sorts runtime/read order by priority then rule id", () => {
    const grouped = groupRulesByStage([
      rule("proxy_to_app", 999, 1), rule("proxy_to_upstream", 10, 2),
      rule("proxy_to_upstream", 20, 4), rule("proxy_to_upstream", 20, 3),
    ]);
    expect(grouped.map((group) => group.stage)).toEqual(RULE_STAGE_ORDER);
    expect(grouped[0].rules.map((item) => [item.priority, item.rule_id])).toEqual([
      [10, "proxy_to_upstream-2"], [20, "proxy_to_upstream-3"], [20, "proxy_to_upstream-4"],
    ]);
    expect(grouped[1].rules[0].priority).toBe(999);
  });
});

describe("ruleStageIncompatibility", () => {
  it("exposes only the two proxy write stages for new message rules", () => {
    expect(NEW_MESSAGE_RULE_STAGES).toEqual(["proxy_to_upstream", "proxy_to_app"]);
  });
  it("rejects an HTTP Document payload when the target capability cannot edit its field action", () => {
    const input: RuleDefinitionSaveInput = {
      rule_id: "rule", expected_revision: 1,
      draft: {
        name: "document", enabled: true, priority: 1, listener_id: "listener", stage: "proxy_to_upstream", one_shot: false,
        content: { type: "http", value: {
          description: "", condition: stringCondition(), actions: [{ source: "document", value: { type: "set", path: "/amount", value: 1 } }],
          document: { package: { id: "pkg", version: "1" } },
        } },
      },
    };
    const context: RuleEditorContext = {
      listener_id: "listener",
      content: { type: "http", value: { stages: [{
        stage: "proxy_to_app", http: null, package: { id: "pkg", version: "1" },
        document_fields: [{ name: "/value", label: "Value", type: "string", operators: ["equals"], actions: [] }], document_common_actions: [],
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
        name: "document", enabled: true, priority: 1, listener_id: "listener", stage: "proxy_to_app", one_shot: false,
        content: { type: "socket", value: {
          package: { id: "pkg", version: "1" },
          condition: stringCondition("/value", "Null"), actions: [{ source: "record_match" }],
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
          common_actions: ["record_match"],
          new_rule_draft: { ...base, rule_id: null, expected_revision: null },
        }],
      } },
    };

    expect(ruleStageIncompatibility(base, context, "proxy_to_app")).toBeNull();
    const actualNull = structuredClone(base);
    if (actualNull.draft.content.type !== "socket") throw new Error("invalid fixture");
    actualNull.draft.content.value.condition = { operator: "leaf", children: { source: "document", path: "/value", predicate: { type: "null_equal" } } };
    expect(ruleStageIncompatibility(actualNull, context, "proxy_to_app")).toBe(
      "目标阶段不能编辑 Document 条件字段 /value。",
    );
  });
});

describe("unified lifecycle wire", () => {
  it("keeps lifecycle at the rule definition and exposes nth-hit as a common condition leaf", () => {
    const socket = rule("proxy_to_app", 1, 1);
    expect(socket.lifecycle).toEqual(lifecycle);
    expect(socket.one_shot).toBe(false);
    expect(socket.content.value).not.toHaveProperty("one_shot");
    const nthHit: RuleDefinitionSaveInput["draft"]["content"] = {
      type: "socket",
      value: {
        package: { id: "pkg", version: "1" },
        condition: { operator: "leaf", children: { source: "nth_hit", count: 2 } },
        actions: [{ source: "record_match" }],
      },
    };
    expect(nthHit.value.condition).toEqual({
      operator: "leaf",
      children: { source: "nth_hit", count: 2 },
    });
  });
});
