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

  it("offers common Method values and hides retired rule capabilities", async () => {
    const reduced = structuredClone(context); if (reduced.content.type !== "http") throw new Error("HTTP required");
    reduced.content.value.stages[0].http!.match_fields = [
      { kind: "method", operators: ["equals"], selector: null },
      { kind: "request_target", operators: ["equals", "contains", "wildcard"], selector: null },
    ];
    reduced.content.value.stages[0].http!.actions = [
      { kind: "replace_body_text", terminal: false, traffic_direction: null, parameters_required: true },
      { kind: "jitter", terminal: false, traffic_direction: null, parameters_required: true },
    ];
    reduced.content.value.stages[0].document_common_actions = [];
    const currentAction = { source: "http", value: { Jitter: { minimum_milliseconds: 1, maximum_milliseconds: 2, scope: "PerChunk" as const } } } as const;
    const nextCondition: Condition = { source: "http", field: "Method", operator: { Equals: "POST" } };
    mocks.ruleDefinitionHttpConditionDraft.mockResolvedValue(nextCondition);
    mocks.ruleDefinitionActionDraft.mockResolvedValue(currentAction.value);
    const user = userEvent.setup(); render(<Harness contextValue={reduced} initial={input(requestTarget, currentAction)} />);

    await user.click(screen.getByRole("button", { name: /HTTP 匹配字段/ }));
    expect(screen.getByRole("option", { name: "Method" })).toBeVisible();
    expect(screen.getByRole("option", { name: "Path（包含 Query 参数）" })).toBeVisible();
    expect(screen.queryByRole("option", { name: "Header" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: "Method" }));
    await selectOption(user, "HTTP 匹配操作符", "equals");

    await user.click(screen.getByRole("button", { name: /HTTP Method/ }));
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual(["GET", "POST", "PUT", "PATCH", "DELETE"]);
    await user.click(screen.getByRole("option", { name: "POST" }));

    await user.click(screen.getByRole("button", { name: /动作来源/ }));
    expect(screen.getByRole("option", { name: "HTTP" })).toBeVisible();
    expect(screen.getByRole("option", { name: "Document" })).toBeVisible();
    expect(screen.queryByRole("option", { name: "通用" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: "HTTP" }));
    const actionType = screen.getByRole("button", { name: /HTTP 动作类型/ });
    expect(actionType).toHaveTextContent("随机延迟");
    await user.click(actionType);
    expect(screen.getByRole("option", { name: "替换当前 Body" })).toBeVisible();
    expect(screen.getByRole("option", { name: "随机延迟" })).toBeVisible();
    expect(screen.queryByRole("option", { name: /Set JSON Field|Set Header|Mock Response|设置 JSON 字段|设置 Header|模拟响应/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: "随机延迟" }));

    await user.click(screen.getByRole("button", { name: "保存规则" }));
    await waitFor(() => expect(mocks.ruleDefinitionHttpConditionDraft).toHaveBeenCalledWith("method", null, "equals", "POST", "proxy_to_upstream"));
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

  it("hides Document nodes without predicate capability from condition selectors", async () => {
    const filtered = structuredClone(context); if (filtered.content.type !== "http") throw new Error("HTTP required");
    filtered.local_document_types.push(
      { value_type: "object", predicates: [], actions: [{ kind: "set", target_kind: "node", target_value_type: "object", operand_value_type: "object" }] },
      { value_type: "array", predicates: [], actions: [{ kind: "append", target_kind: "array", target_value_type: "array", operand_value_type: "string" }] },
    );
    filtered.content.value.stages[0].document_fields = [
      { path: "/GBRD_01/*", label: "GBRD item", value_type: "object", item_template: true, predicates: [], actions: [{ kind: "set", target_kind: "node", target_value_type: "object", operand_value_type: "object" }] },
      { path: "/GBRD_01/*/kid", label: "kid", value_type: "string", item_template: true, predicates: ["equals"], actions: [{ kind: "set", target_kind: "node", target_value_type: "string", operand_value_type: "string" }] },
    ];
    const user = userEvent.setup(); render(<Harness contextValue={filtered} initial={input(requestTarget, recordMatch)} />);

    await selectOption(user, "条件来源", "Document");
    await user.click(screen.getByRole("button", { name: /Document Schema 条件路径/ }));
    expect(screen.getByRole("option", { name: "kid · /GBRD_01/*/kid" })).toBeVisible();
    expect(screen.queryByRole("option", { name: "GBRD item · /GBRD_01/*" })).not.toBeInTheDocument();
    await user.keyboard("{Escape}");

    await user.click(screen.getByRole("button", { name: /Document 条件值类型/ }));
    expect(screen.getByRole("option", { name: "string" })).toBeVisible();
    expect(screen.getByRole("option", { name: "number" })).toBeVisible();
    expect(screen.queryByRole("option", { name: "object" })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "array" })).not.toBeInTheDocument();
    await user.keyboard("{Escape}");

    await selectOption(user, "动作来源", "Document");
    await user.click(screen.getByRole("button", { name: /Document Schema 动作路径/ }));
    expect(screen.getByRole("option", { name: "GBRD item · /GBRD_01/*" })).toBeVisible();
  });

  it("keeps every rule Select value on one clipped line", async () => {
    const clipped = structuredClone(context); if (clipped.content.type !== "http") throw new Error("HTTP required");
    clipped.content.value.stages[0].document_fields = [{ path: "/KCCI_01/*/kid", label: "KCCI_01 item kid", value_type: "string", item_template: true, predicates: ["equals"], actions: [
      { kind: "set", target_kind: "node", target_value_type: "string", operand_value_type: "string" },
      { kind: "clear", target_kind: "node", target_value_type: "string", operand_value_type: null },
    ] }];
    const user = userEvent.setup(); render(<Harness contextValue={clipped} initial={input(requestTarget, recordMatch)} />);
    const condition = within(screen.getByTestId("condition-form"));
    expectClippedSelect(condition.getByRole("button", { name: /条件来源/ }));
    const matchField = condition.getByRole("button", { name: /HTTP 匹配字段/ });
    expectClippedSelect(matchField);
    expect(matchField).toHaveTextContent("Path（包含 Query 参数）");
    expectClippedSelect(condition.getByRole("button", { name: /HTTP 匹配操作符/ }));
    expect(condition.getByRole("textbox", { name: "HTTP 匹配值" })).toHaveClass("h-10", "w-full");

    await selectOption(user, "条件来源", "Document");
    await selectOption(user, "Document Schema 条件路径", "KCCI_01 item kid · /KCCI_01/*/kid");
    const conditionPath = screen.getByRole("button", { name: /Document Schema 条件路径/ });
    expectClippedSelect(conditionPath);
    expect(conditionPath).toHaveTextContent("KCCI_01 item kid · /KCCI_01/*/kid");

    await selectOption(user, "动作来源", "Document");
    const action = within(screen.getByTestId("action-form"));
    expectClippedSelect(action.getByRole("button", { name: /动作来源/ }));
    await selectOption(user, "Document Schema 动作路径", "KCCI_01 item kid · /KCCI_01/*/kid");
    const actionPath = action.getByRole("button", { name: /Document Schema 动作路径/ });
    expectClippedSelect(actionPath);
    expect(actionPath).toHaveTextContent("KCCI_01 item kid · /KCCI_01/*/kid");
    expect(action.getByText("* 仅展开一层；动作会应用到当前命中的全部节点。")).toBeInTheDocument();
    const documentAction = action.getAllByRole("button").find((button) => button.getAttribute("aria-label") === "Document 动作");
    expect(documentAction).toBeDefined();
    await user.click(documentAction!);
    await user.click(await screen.findByRole("option", { name: "set" }));
    expect(documentAction).toHaveTextContent("set");
    expect(action.queryByRole("button", { name: /创建|添加/ })).not.toBeInTheDocument();
  });

  it("reverse-fills an existing HTTP action into explicit fields and saves it", async () => {
    const existingAction = { source: "http", value: { Jitter: { minimum_milliseconds: 1, maximum_milliseconds: 2, scope: "PerChunk" as const } } } as const;
    const nextAction = { Jitter: { minimum_milliseconds: 3, maximum_milliseconds: 5, scope: "PerChunk" as const } };
    mocks.ruleDefinitionHttpConditionDraft.mockResolvedValue(requestTarget);
    mocks.ruleDefinitionActionDraft.mockResolvedValue(nextAction);
    const onSave = vi.fn(); const user = userEvent.setup();
    render(<Harness initial={input(requestTarget, existingAction)} onSave={onSave} />);

    const selectorRow = screen.getByTestId("http-action-selector-row");
    expect(within(selectorRow).getByRole("button", { name: /动作来源/ })).toHaveTextContent("HTTP");
    expect(within(selectorRow).getByRole("button", { name: /HTTP 动作类型/ })).toHaveTextContent("随机延迟");
    expect(screen.queryByRole("textbox", { name: "动作参数 JSON" })).not.toBeInTheDocument();
    expect(screen.getByTestId("http-action-parameters-row")).toHaveClass("w-full");
    expect(screen.getByLabelText("最小抖动（毫秒）")).toHaveValue(1);
    expect(screen.getByLabelText("最大抖动（毫秒）")).toHaveValue(2);
    expect(screen.getByRole("button", { name: /抖动方式/ })).toHaveTextContent("每个分块");
    fireEvent.change(screen.getByLabelText("最小抖动（毫秒）"), { target: { value: "3" } });
    fireEvent.change(screen.getByLabelText("最大抖动（毫秒）"), { target: { value: "5" } });
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    await waitFor(() => expect(mocks.ruleDefinitionActionDraft).toHaveBeenCalledWith({
      kind: "jitter",
      parameters_json: JSON.stringify({ minimum_milliseconds: 3, maximum_milliseconds: 5, scope: "per_chunk" }),
    }, "proxy_to_upstream"));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ draft: expect.objectContaining({ content: { type: "http", value: expect.objectContaining({ action: { source: "http", value: nextAction } }) } }) }));
  });

  it("uses labeled delay and throttle fields instead of raw JSON", async () => {
    const editable = structuredClone(context); if (editable.content.type !== "http") throw new Error("HTTP required");
    editable.content.value.stages[0].http!.actions = [
      { kind: "delay", terminal: false, traffic_direction: null, parameters_required: true },
      { kind: "throttle", terminal: false, traffic_direction: "upstream", parameters_required: true },
    ];
    const delay = { source: "http", value: { Delay: { milliseconds: 80 } } } as const;
    const nextAction = { Delay: { milliseconds: 125 } };
    mocks.ruleDefinitionHttpConditionDraft.mockResolvedValue(requestTarget);
    mocks.ruleDefinitionActionDraft.mockResolvedValue(nextAction);
    const user = userEvent.setup(); render(<Harness contextValue={editable} initial={input(requestTarget, delay)} />);

    expect(screen.getByLabelText("延迟时间（毫秒）")).toHaveValue(80);
    expect(screen.queryByText("动作参数 JSON")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("延迟时间（毫秒）"), { target: { value: "125" } });
    await user.click(screen.getByRole("button", { name: "保存规则" }));
    await waitFor(() => expect(mocks.ruleDefinitionActionDraft).toHaveBeenCalledWith({ kind: "delay", parameters_json: JSON.stringify({ milliseconds: 125 }) }, "proxy_to_upstream"));

    await selectOption(user, "HTTP 动作类型", "限速");
    expect(screen.getByLabelText("速率（B/s）")).toHaveValue(null);
    expect(screen.getByLabelText("分块大小（字节）")).toHaveValue(null);
    expect(screen.getByRole("button", { name: "保存规则" })).toBeDisabled();
  });

  it("hides the parameter area for parameterless HTTP actions", async () => {
    const editable = structuredClone(context); if (editable.content.type !== "http") throw new Error("HTTP required");
    editable.content.value.stages[0].http!.actions = [
      { kind: "disconnect_before_upstream", terminal: true, traffic_direction: null, parameters_required: false },
    ];
    const action = { source: "terminal", value: "DisconnectBeforeUpstream" } as const;
    mocks.ruleDefinitionHttpConditionDraft.mockResolvedValue(requestTarget);
    mocks.ruleDefinitionActionDraft.mockResolvedValue({ Terminal: "DisconnectBeforeUpstream" });
    const user = userEvent.setup(); render(<Harness contextValue={editable} initial={input(requestTarget, action)} />);

    expect(screen.queryByTestId("http-action-parameters-row")).not.toBeInTheDocument();
    expect(screen.queryByText("动作参数 JSON")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "保存规则" }));
    await waitFor(() => expect(mocks.ruleDefinitionActionDraft).toHaveBeenCalledWith({ kind: "disconnect_before_upstream", parameters_json: null }, "proxy_to_upstream"));
  });

  it("deletes an existing rule immediately without a second confirmation", async () => {
    const onDelete = vi.fn();
    const user = userEvent.setup();
    render(<Harness initial={input(requestTarget, recordMatch)} onDelete={onDelete} />);

    const actions = screen.getByTestId("rule-editor-actions");
    expect(within(actions).getByRole("button", { name: "保存规则" })).toBeVisible();
    expect(within(actions).getByRole("button", { name: "复制规则" })).toBeVisible();
    await user.click(within(actions).getByRole("button", { name: "删除规则" }));
    expect(onDelete).toHaveBeenCalledOnce();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "确认删除" })).not.toBeInTheDocument();
  });

  it("bottom-aligns the enable switch with the priority input group", () => {
    render(<Harness initial={input(requestTarget, recordMatch)} />);

    expect(screen.getByTestId("rule-metadata-toggle-priority-row")).toHaveClass("items-end");
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

function Harness({ initial, contextValue = context, onDelete = vi.fn(), onSave = vi.fn() }: { initial: RuleDefinitionSaveInput; contextValue?: RuleEditorContext; onDelete?: () => void; onSave?: (value: RuleDefinitionSaveInput) => void }) {
  const [value, setValue] = useState(initial);
  return <RuleDefinitionEditor context={contextValue} fieldErrors={{}} input={value} listener={listener} loading={false} pending={false} onChange={(change) => setValue((current) => typeof change === "function" ? change(current) : change)} onCopy={vi.fn()} onDelete={onDelete} onSave={onSave} />;
}
function input(condition: Condition, action: Extract<RuleDefinitionSaveInput["draft"]["content"], { type: "http" }>["value"]["action"]): RuleDefinitionSaveInput {
  return { rule_id: "http-rule", expected_revision: 1, draft: { name: "HTTP rule", enabled: true, priority: 1, listener_id: listener.id, stage: "proxy_to_upstream", content: { type: "http", value: { description: "", condition, action } } } };
}
async function selectOption(user: ReturnType<typeof userEvent.setup>, control: string, option: string) {
  await user.click(screen.getByRole("button", { name: new RegExp(control) }));
  await user.click(await screen.findByRole("option", { name: option }));
}
function expectClippedSelect(trigger: HTMLElement) {
  expect(trigger).toHaveClass("h-10", "min-h-10", "w-full", "min-w-0", "overflow-hidden");
  expect(trigger.querySelector(".min-w-0.flex-1.truncate.whitespace-nowrap")).not.toBeNull();
  expect(trigger.querySelector(".shrink-0")).not.toBeNull();
}
