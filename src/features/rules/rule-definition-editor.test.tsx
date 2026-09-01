// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Condition, RuleDefinitionSaveInput, RuleEditorContext } from "@/generated/rust-types";
import { testListener } from "./rules-view-test-fixtures";
import { RuleDefinitionEditor } from "./rule-definition-editor";

const commandMocks = vi.hoisted(() => ({
  ruleDefinitionHttpConditionDraft: vi.fn(),
  ruleDefinitionNthHitConditionDraft: vi.fn(),
  ruleDefinitionDocumentConditionDraft: vi.fn(),
  ruleDefinitionDocumentActionDraft: vi.fn(),
  ruleDefinitionDocumentCommonActionDraft: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({ commands: commandMocks }));
vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: () => "Rust 操作失败",
}));

const listener = testListener("http-listener", "HTTP Listener", "http");
const requestTarget: Condition = { source: "http", field: "RequestTarget", operator: { Equals: "/" } };

const context: RuleEditorContext = {
  listener_id: listener.id,
  local_document_types: [{
    value_type: "string",
    predicates: ["equals", "contains", "starts_with", "ends_with"],
    actions: [
      { kind: "set", target_kind: "node", target_value_type: "string", operand_value_type: "string" },
      { kind: "clear", target_kind: "node", target_value_type: "string", operand_value_type: null },
    ],
  }, {
    value_type: "number",
    predicates: ["equals", "less", "less_equal", "greater", "greater_equal"],
    actions: [
      { kind: "set", target_kind: "node", target_value_type: "number", operand_value_type: "number" },
      { kind: "clear", target_kind: "node", target_value_type: "number", operand_value_type: null },
    ],
  }],
  document_condition_path: { wildcard_token: "*", wildcard_matches_exactly_one_level: true, multiple_matches_use_any: true },
  content: { type: "http", value: { stages: [{
    stage: "proxy_to_upstream",
    http: { stage: "proxy_to_upstream", match_fields: [
      { kind: "method", operators: ["equals"], selector: null },
      { kind: "request_target", operators: ["equals", "contains", "wildcard"], selector: null },
      { kind: "header", operators: ["equals", "contains"], selector: "header_name_pointer" },
    ], actions: [] },
    package: { id: "json", version: "1" },
    document_fields: [{ path: "/customer/age", label: "Age", value_type: "number", item_template: false, predicates: ["equals"], actions: [] }],
    document_common_actions: ["record_match"],
    new_rule_draft: {
      listener_id: listener.id,
      stage: "proxy_to_upstream",
      content: input([requestTarget]).draft.content,
    },
  }] } },
};

describe("RuleDefinitionEditor flat conditions", () => {
  beforeEach(() => vi.clearAllMocks());

  it("requires one condition and appends the Rust-authored HTTP condition", async () => {
    commandMocks.ruleDefinitionHttpConditionDraft.mockResolvedValue({ source: "http", field: "Method", operator: { Equals: "POST" } });
    const onSave = vi.fn();
    const user = userEvent.setup();
    render(<Harness initial={input([])} onSave={onSave} />);

    expect(screen.getByRole("alert")).toHaveTextContent("至少需要一个条件");
    expect(screen.getByRole("button", { name: "保存规则" })).toBeDisabled();
    await selectOption(user, "HTTP 匹配字段", "Method");
    await selectOption(user, "HTTP 匹配操作符", "equals");
    await user.type(screen.getByRole("textbox", { name: "HTTP 匹配值" }), "POST");
    await user.click(screen.getByRole("button", { name: "创建 HTTP 条件" }));
    await waitFor(() => expect(screen.getByText(/Method · equals POST/)).toBeVisible());
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    expect(onSave).toHaveBeenCalledWith([{ source: "http", field: "Method", operator: { Equals: "POST" } }]);
  });

  it("offers schema selection and manual RFC 6901 path input without a metadata tree", async () => {
    render(<Harness initial={input([requestTarget])} />);

    expect(screen.getByRole("button", { name: /Document Schema 条件路径/ })).toBeVisible();
    expect(screen.getByRole("textbox", { name: "手动 Document 条件路径" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "手动选择根路径 /" })).not.toBeInTheDocument();
    expect(screen.queryByText(/metadata tree/i)).not.toBeInTheDocument();
    expect(screen.getByText("所有条件固定为 AND；需要 OR 时请新建多条规则。")).toBeVisible();
  });

  it("creates a manual Body condition for Plain HTTP without Schema metadata", async () => {
    commandMocks.ruleDefinitionDocumentConditionDraft.mockResolvedValue({
      source: "document", path: "/customer/age", predicate: { type: "number", value: { operator: "equal", value: 18 } },
    });
    const plainContext = structuredClone(context);
    if (plainContext.content.type !== "http") throw new Error("HTTP context required");
    plainContext.content.value.stages[0].package = null;
    plainContext.content.value.stages[0].document_fields = [];
    const onSave = vi.fn();
    const user = userEvent.setup();
    render(<Harness contextValue={plainContext} initial={input([requestTarget])} onSave={onSave} />);

    await user.type(screen.getByRole("textbox", { name: "手动 Document 条件路径" }), "/customer/age");
    await selectOption(user, "Document 值类型", "number");
    await selectOption(user, "Document 谓词", "equals");
    await user.type(screen.getByRole("textbox", { name: "匹配值" }), "18");
    await user.click(screen.getByRole("button", { name: "添加 Document 条件" }));

    await waitFor(() => expect(commandMocks.ruleDefinitionDocumentConditionDraft).toHaveBeenCalledWith("/customer/age", "number", "equals", "18"));
    await waitFor(() => expect(screen.getByText(/条件 2 个/)).toBeVisible());
    await user.click(screen.getByRole("button", { name: "保存规则" }));
    expect(onSave).toHaveBeenCalledWith([requestTarget, {
      source: "document", path: "/customer/age", predicate: { type: "number", value: { operator: "equal", value: 18 } },
    }]);
    expect(screen.queryByText(/没有协议 Body Document 能力/)).not.toBeInTheDocument();
  });

  it("uses the manual slash input as the Document root without a separate button", async () => {
    commandMocks.ruleDefinitionDocumentConditionDraft.mockResolvedValue({
      source: "document", path: "", predicate: { type: "string", value: { operator: "equal", value: "root" } },
    });
    const plainContext = structuredClone(context);
    if (plainContext.content.type !== "http") throw new Error("HTTP context required");
    plainContext.content.value.stages[0].package = null;
    plainContext.content.value.stages[0].document_fields = [];
    const user = userEvent.setup();
    render(<Harness contextValue={plainContext} initial={input([requestTarget])} />);

    expect(screen.queryByRole("button", { name: "手动选择根路径 /" })).not.toBeInTheDocument();
    await user.type(screen.getByRole("textbox", { name: "手动 Document 条件路径" }), "/");
    await selectOption(user, "Document 值类型", "string");
    await selectOption(user, "Document 谓词", "equals");
    await user.type(screen.getByRole("textbox", { name: "匹配值" }), "root");
    await user.click(screen.getByRole("button", { name: "添加 Document 条件" }));

    await waitFor(() => expect(commandMocks.ruleDefinitionDocumentConditionDraft).toHaveBeenCalledWith("", "string", "equals", "root"));
  });

  it("keeps each HTTP and Document factory in its own aligned semantic row", async () => {
    const layoutContext = structuredClone(context);
    if (layoutContext.content.type !== "http") throw new Error("HTTP context required");
    const layoutStage = layoutContext.content.value.stages[0];
    if (!layoutStage?.http) throw new Error("HTTP stage required");
    layoutStage.http.actions = [{ kind: "jitter", terminal: false, traffic_direction: null, parameters_required: true }];
    const user = userEvent.setup();
    render(<Harness contextValue={layoutContext} initial={input([requestTarget])} />);

    const httpCondition = within(screen.getByTestId("http-condition-factory"));
    expect(httpCondition.getByRole("button", { name: /HTTP 匹配字段/ })).toHaveClass("h-10", "min-h-10", "w-full");
    expect(httpCondition.getByRole("button", { name: /HTTP 匹配操作符/ })).toHaveClass("h-10", "min-h-10", "w-full");
    expect(httpCondition.getByRole("textbox", { name: "HTTP 匹配值" })).toHaveClass("h-10", "py-0", "w-full");
    expect(httpCondition.getByRole("button", { name: "创建 HTTP 条件" })).toHaveClass("h-10", "w-full");
    expect(httpCondition.queryByRole("textbox", { name: "第 N 次命中" })).not.toBeInTheDocument();

    const nthCondition = within(screen.getByTestId("nth-condition-factory"));
    expect(nthCondition.getByRole("textbox", { name: "第 N 次命中" })).toHaveClass("h-10", "py-0", "w-full");
    expect(nthCondition.getByRole("button", { name: "添加条件：第 N 次命中" })).toHaveClass("h-10", "w-full");
    expect(nthCondition.queryByRole("button", { name: /HTTP 匹配字段/ })).not.toBeInTheDocument();

    await selectOption(user, "HTTP 动作类型", "Jitter");
    const httpActionControls = within(screen.getByTestId("http-action-controls"));
    expect(httpActionControls.getByRole("button", { name: /HTTP 动作类型/ })).toHaveClass("h-10", "min-h-10", "w-full");
    expect(httpActionControls.getByRole("button", { name: "创建 HTTP 动作" })).toHaveClass("h-10", "w-full");
    expect(httpActionControls.queryByRole("textbox", { name: "动作参数 JSON" })).not.toBeInTheDocument();
    const httpActionParameters = within(screen.getByTestId("http-action-parameters"));
    expect(httpActionParameters.getByRole("textbox", { name: "动作参数 JSON" })).toHaveClass("min-h-24", "w-full");
    expect(httpActionParameters.queryByRole("button", { name: "创建 HTTP 动作" })).not.toBeInTheDocument();

    const documentPath = within(screen.getByTestId("document-path-factory"));
    expect(documentPath.getByRole("button", { name: /Document Schema 条件路径/ })).toHaveClass("h-10", "min-h-10", "w-full");
    expect(documentPath.getByRole("textbox", { name: "手动 Document 条件路径" })).toHaveClass("h-10", "py-0", "w-full");
    expect(documentPath.getByRole("button", { name: /Document 值类型/ })).toHaveClass("h-10", "min-h-10", "w-full");

    const documentCondition = within(screen.getByTestId("document-condition-factory"));
    expect(documentCondition.getByRole("textbox", { name: "匹配值" })).toHaveClass("h-10", "py-0", "w-full");
    expect(documentCondition.getByRole("button", { name: "添加 Document 条件" })).toHaveClass("h-10", "w-full");
    expect(documentCondition.queryByRole("button", { name: "添加 Document 动作" })).not.toBeInTheDocument();

    const documentAction = within(screen.getByTestId("document-action-factory"));
    expect(documentAction.getByRole("textbox", { name: "动作值" })).toHaveClass("h-10", "py-0", "w-full");
    expect(documentAction.getByRole("button", { name: "添加 Document 动作" })).toHaveClass("h-10", "w-full");
    expect(documentAction.queryByRole("button", { name: "添加 Document 条件" })).not.toBeInTheDocument();
  });
});

function Harness({ initial, contextValue = context, onSave = vi.fn() }: { initial: RuleDefinitionSaveInput; contextValue?: RuleEditorContext; onSave?: (conditions: Condition[]) => void }) {
  const [value, setValue] = useState(initial);
  return <RuleDefinitionEditor context={contextValue} fieldErrors={{}} input={value} listener={listener} loading={false} pending={false}
    onChange={(change) => setValue((current) => typeof change === "function" ? change(current) : change)}
    onCopy={vi.fn()} onDelete={vi.fn()} onSave={() => onSave(value.draft.content.value.conditions)} onToggle={vi.fn()} />;
}

function input(conditions: Condition[]): RuleDefinitionSaveInput {
  return { rule_id: "http-rule", expected_revision: 1, draft: {
    name: "HTTP rule", enabled: true, priority: 1, listener_id: listener.id, stage: "proxy_to_upstream", one_shot: false,
    content: { type: "http", value: { description: "", conditions, actions: [] } },
  } };
}

async function selectOption(user: ReturnType<typeof userEvent.setup>, control: string, option: string) {
  await user.click(screen.getByRole("button", { name: new RegExp(control) }));
  await user.click(await screen.findByRole("option", { name: option }));
}
