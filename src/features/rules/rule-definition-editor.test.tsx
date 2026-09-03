// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Condition, RuleDefinitionSaveInput, RuleEditorContext } from "@/generated/rust-types";
import { testListener } from "./rules-view-test-fixtures";
import { RuleDefinitionEditor } from "./rule-definition-editor";

const mocks = vi.hoisted(() => ({
  ruleDefinitionHttpConditionDraft: vi.fn(), ruleDefinitionActionDraft: vi.fn(),
  ruleDefinitionDocumentConditionDraft: vi.fn(), ruleDefinitionDocumentActionDraft: vi.fn(),
  ruleDefinitionDocumentCommonActionDraft: vi.fn(),
}));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));
vi.mock("@/lib/ipc/client", () => ({ callCommand: async <T,>(value: T) => value, errorMessage: () => "Rust 操作失败" }));

const listener = testListener("http-listener", "HTTP Listener", "http");
const requestTarget: Condition = { source: "http", field: "RequestTarget", operator: { Equals: "/" } };
const recordMatch = { source: "record_match" } as const;
const context: RuleEditorContext = {
  listener_id: listener.id,
  local_document_types: [
    { value_type: "string", predicates: ["equals", "contains"], actions: [{ kind: "set", target_kind: "node", target_value_type: "string", operand_value_type: "string" }] },
    { value_type: "number", predicates: ["equals"], actions: [{ kind: "set", target_kind: "node", target_value_type: "number", operand_value_type: "number" }] },
  ],
  document_condition_path: { wildcard_token: "*", wildcard_matches_exactly_one_level: true, multiple_matches_use_any: true },
  content: { type: "http", value: { stages: [{
    stage: "proxy_to_upstream",
    http: { stage: "proxy_to_upstream", match_fields: [
      { kind: "method", operators: ["equals"], selector: null },
      { kind: "request_target", operators: ["equals", "contains", "wildcard"], selector: null },
      { kind: "header", operators: ["equals"], selector: "header_name_pointer" },
    ], actions: [{ kind: "jitter", terminal: false, traffic_direction: null, parameters_required: true }] },
    package: { id: "json", version: "1" },
    document_fields: [{ path: "/customer/age", label: "Age", value_type: "number", item_template: false, predicates: ["equals"], actions: [] }],
    document_common_actions: ["record_match"],
    new_rule_draft: { listener_id: listener.id, stage: "proxy_to_upstream", content: { type: "http", value: { description: "" } } },
  }] } },
};

describe("RuleDefinitionEditor single pair", () => {
  beforeEach(() => vi.clearAllMocks());

  it("materializes the edited condition and action only on Save", async () => {
    const next: Condition = { source: "http", field: "RequestTarget", operator: { Equals: "/changed" } };
    mocks.ruleDefinitionHttpConditionDraft.mockResolvedValue(next);
    mocks.ruleDefinitionDocumentCommonActionDraft.mockResolvedValue(recordMatch);
    const onSave = vi.fn(); const user = userEvent.setup();
    render(<Harness initial={input(requestTarget, recordMatch)} onSave={onSave} />);

    expect(screen.getAllByTestId("condition-form")).toHaveLength(1);
    expect(screen.getAllByTestId("action-form")).toHaveLength(1);
    expect(screen.getByRole("textbox", { name: "HTTP 匹配值" })).toHaveValue("/");
    expect(screen.queryByTestId("rule-pair-card")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /创建 HTTP|添加 Document|添加：记录命中/ })).not.toBeInTheDocument();
    await user.clear(screen.getByRole("textbox", { name: "HTTP 匹配值" }));
    await user.type(screen.getByRole("textbox", { name: "HTTP 匹配值" }), "/changed");
    expect(mocks.ruleDefinitionHttpConditionDraft).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    await waitFor(() => expect(mocks.ruleDefinitionHttpConditionDraft).toHaveBeenCalledWith("request_target", null, "equals", "/changed", "proxy_to_upstream"));
    expect(mocks.ruleDefinitionDocumentCommonActionDraft).toHaveBeenCalledWith("record_match");
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ draft: expect.objectContaining({ content: { type: "http", value: expect.objectContaining({ condition: next, action: recordMatch }) } }) }));
  });

  it("materializes schema-free Body manual path only on Save", async () => {
    mocks.ruleDefinitionDocumentConditionDraft.mockResolvedValue({ source: "document", path: "/customer/age", predicate: { type: "number", value: { operator: "equal", value: 18 } } });
    mocks.ruleDefinitionDocumentCommonActionDraft.mockResolvedValue(recordMatch);
    const plain = structuredClone(context); if (plain.content.type !== "http") throw new Error("HTTP required");
    plain.content.value.stages[0].package = null; plain.content.value.stages[0].document_fields = [];
    const user = userEvent.setup(); render(<Harness contextValue={plain} initial={input(requestTarget, recordMatch)} />);
    await selectOption(user, "条件来源", "Document");
    expect(screen.queryByRole("button", { name: /Document Schema 条件路径/ })).not.toBeInTheDocument();
    await user.type(screen.getByRole("textbox", { name: "手动 Document 条件路径" }), "/customer/age");
    await selectOption(user, "Document 条件值类型", "number");
    await selectOption(user, "Document 谓词", "equals");
    await user.type(screen.getByRole("textbox", { name: "匹配值" }), "18");
    expect(mocks.ruleDefinitionDocumentConditionDraft).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "保存规则" }));
    await waitFor(() => expect(mocks.ruleDefinitionDocumentConditionDraft).toHaveBeenCalledWith("/customer/age", "number", "equals", "18"));
  });

  it("shows schema root as slash but materializes the empty Rust path", async () => {
    mocks.ruleDefinitionDocumentConditionDraft.mockResolvedValue({ source: "document", path: "", predicate: { type: "string", value: { operator: "equal", value: "root" } } });
    mocks.ruleDefinitionDocumentCommonActionDraft.mockResolvedValue(recordMatch);
    const root = structuredClone(context); if (root.content.type !== "http") throw new Error("HTTP required");
    root.content.value.stages[0].document_fields = [{ path: "", label: "Root", value_type: "string", item_template: false, predicates: ["equals"], actions: [] }];
    const user = userEvent.setup(); render(<Harness contextValue={root} initial={input(requestTarget, recordMatch)} />);
    await selectOption(user, "条件来源", "Document");
    await selectOption(user, "Document Schema 条件路径", "Root · /（根）");
    expect(screen.getByRole("textbox", { name: "手动 Document 条件路径" })).toHaveValue("/");
    await selectOption(user, "Document 谓词", "equals"); await user.type(screen.getByRole("textbox", { name: "匹配值" }), "root");
    await user.click(screen.getByRole("button", { name: "保存规则" }));
    await waitFor(() => expect(mocks.ruleDefinitionDocumentConditionDraft).toHaveBeenCalledWith("", "string", "equals", "root"));
  });

  it("keeps the single forms aligned without intermediate buttons", async () => {
    const user = userEvent.setup(); render(<Harness initial={input(requestTarget, recordMatch)} />);
    const condition = within(screen.getByTestId("condition-form"));
    expect(condition.getByRole("button", { name: /条件来源/ })).toHaveClass("h-10", "min-h-10", "w-full");
    const matchField = condition.getByRole("button", { name: /HTTP 匹配字段/ });
    expect(matchField).toHaveClass("h-10", "min-h-10", "w-full", "min-w-0", "overflow-hidden");
    expect(matchField).toHaveTextContent("Path（包含 Query 参数）");
    expect(within(matchField).getByText("Path（包含 Query 参数）")).toHaveClass("min-w-0", "flex-1", "truncate", "whitespace-nowrap");
    expect(condition.getByRole("textbox", { name: "HTTP 匹配值" })).toHaveClass("h-10", "w-full");
    await selectOption(user, "动作来源", "HTTP");
    const action = within(screen.getByTestId("action-form"));
    expect(action.getByRole("button", { name: /HTTP 动作类型/ })).toHaveClass("h-10", "min-h-10", "w-full");
    expect(action.queryByRole("button", { name: /创建|添加/ })).not.toBeInTheDocument();
  });

  it("reverse-fills an existing HTTP action and saves its edited parameters", async () => {
    const existingAction = { source: "http", value: { Jitter: { minimum_milliseconds: 1, maximum_milliseconds: 2, scope: "PerChunk" as const } } } as const;
    const nextAction = { Jitter: { minimum_milliseconds: 3, maximum_milliseconds: 5, scope: "PerChunk" as const } };
    mocks.ruleDefinitionHttpConditionDraft.mockResolvedValue(requestTarget);
    mocks.ruleDefinitionActionDraft.mockResolvedValue(nextAction);
    const onSave = vi.fn(); const user = userEvent.setup();
    render(<Harness initial={input(requestTarget, existingAction)} onSave={onSave} />);

    const selectorRow = screen.getByTestId("http-action-selector-row");
    expect(within(selectorRow).getByRole("button", { name: /动作来源/ })).toHaveTextContent("HTTP");
    expect(within(selectorRow).getByRole("button", { name: /HTTP 动作类型/ })).toHaveTextContent("Jitter");
    const parameters = screen.getByRole("textbox", { name: "动作参数 JSON" });
    expect(selectorRow).not.toContainElement(parameters);
    expect(parameters.closest("[data-testid='http-action-parameters-row']")).toHaveClass("w-full");
    expect(parameters).toHaveValue(JSON.stringify(existingAction.value.Jitter));
    fireEvent.change(parameters, { target: { value: JSON.stringify(nextAction.Jitter) } });
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    await waitFor(() => expect(mocks.ruleDefinitionActionDraft).toHaveBeenCalledWith({ kind: "jitter", parameters_json: JSON.stringify(nextAction.Jitter) }, "proxy_to_upstream"));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ draft: expect.objectContaining({ content: { type: "http", value: expect.objectContaining({ action: { source: "http", value: nextAction } }) } }) }));
  });

  it("keeps save, copy, and delete in one action row", () => {
    render(<Harness initial={input(requestTarget, recordMatch)} />);

    const actions = screen.getByTestId("rule-editor-actions");
    expect(within(actions).getByRole("button", { name: "保存规则" })).toBeVisible();
    expect(within(actions).getByRole("button", { name: "复制规则" })).toBeVisible();
    expect(within(actions).getByRole("button", { name: "删除规则" })).toBeVisible();
  });

  it("keeps Socket on the Document boundary and materializes its single pair on Save", async () => {
    const condition: Condition = { source: "document", path: "/amount", predicate: { type: "number", value: { operator: "equal", value: 10 } } };
    const next: Condition = { source: "document", path: "/amount", predicate: { type: "number", value: { operator: "equal", value: 20 } } };
    mocks.ruleDefinitionDocumentConditionDraft.mockResolvedValue(next);
    mocks.ruleDefinitionDocumentCommonActionDraft.mockResolvedValue(recordMatch);
    const socketContext: RuleEditorContext = {
      ...context,
      content: { type: "socket", value: { package: { id: "socket-json", version: "1" }, stages: [{
        stage: "proxy_to_app", document_fields: [], common_actions: ["record_match"],
        new_rule_draft: { listener_id: listener.id, stage: "proxy_to_app", content: { type: "socket", value: { package: { id: "socket-json", version: "1" } } } },
      }] } },
    };
    const socketInput: RuleDefinitionSaveInput = { rule_id: "socket-rule", expected_revision: 1, draft: { name: "Socket rule", enabled: true, priority: 1, listener_id: listener.id, stage: "proxy_to_app", content: { type: "socket", value: { package: { id: "socket-json", version: "1" }, condition, action: recordMatch } } } };
    const onSave = vi.fn(); const user = userEvent.setup();
    render(<Harness contextValue={socketContext} initial={socketInput} onSave={onSave} />);

    expect(screen.queryByRole("button", { name: "条件来源" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /HTTP 匹配字段|HTTP 动作类型/ })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "手动 Document 条件路径" })).toHaveValue("/amount");
    await user.clear(screen.getByRole("textbox", { name: "匹配值" })); await user.type(screen.getByRole("textbox", { name: "匹配值" }), "20");
    await user.click(screen.getByRole("button", { name: "保存规则" }));
    await waitFor(() => expect(mocks.ruleDefinitionDocumentConditionDraft).toHaveBeenCalledWith("/amount", "number", "equals", "20"));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ draft: expect.objectContaining({ content: { type: "socket", value: expect.objectContaining({ condition: next, action: recordMatch }) } }) }));
  });
});

function Harness({ initial, contextValue = context, onSave = vi.fn() }: { initial: RuleDefinitionSaveInput; contextValue?: RuleEditorContext; onSave?: (value: RuleDefinitionSaveInput) => void }) {
  const [value, setValue] = useState(initial);
  return <RuleDefinitionEditor context={contextValue} fieldErrors={{}} input={value} listener={listener} loading={false} pending={false} onChange={(change) => setValue((current) => typeof change === "function" ? change(current) : change)} onCopy={vi.fn()} onDelete={vi.fn()} onSave={onSave} />;
}
function input(condition: Condition, action: Extract<RuleDefinitionSaveInput["draft"]["content"], { type: "http" }>["value"]["action"]): RuleDefinitionSaveInput {
  return { rule_id: "http-rule", expected_revision: 1, draft: { name: "HTTP rule", enabled: true, priority: 1, listener_id: listener.id, stage: "proxy_to_upstream", content: { type: "http", value: { description: "", condition, action } } } };
}
async function selectOption(user: ReturnType<typeof userEvent.setup>, control: string, option: string) {
  await user.click(screen.getByRole("button", { name: new RegExp(control) }));
  await user.click(await screen.findByRole("option", { name: option }));
}
