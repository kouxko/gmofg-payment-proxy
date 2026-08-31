// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RuleDefinitionSaveInput, RuleEditorContext } from "@/generated/rust-types";
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
const httpLeaf = { operator: "leaf" as const, children: { source: "http" as const, field: "RequestTarget" as const, operator: { Equals: "/" } } };
const documentLeaf = { operator: "leaf" as const, children: { source: "document" as const, path: "/amount", predicate: { type: "number" as const, value: { operator: "equal" as const, value: 1 } } } };

const context: RuleEditorContext = {
  listener_id: listener.id,
  local_document_types: [],
  document_condition_path: { wildcard_token: "*", wildcard_matches_exactly_one_level: true, multiple_matches_use_any: true },
  content: { type: "http", value: { stages: [{
    stage: "proxy_to_upstream",
    http: { stage: "request", match_fields: [
      { kind: "method", operators: ["equals"], selector: null },
      { kind: "request_target", operators: ["equals", "contains", "wildcard"], selector: null },
      { kind: "header", operators: ["equals", "contains"], selector: "header_name_pointer" },
    ], actions: [] },
    package: null,
    document_fields: [],
    document_common_actions: [],
    new_rule_draft: {
      listener_id: listener.id,
      stage: "proxy_to_upstream",
      content: input(httpLeaf).draft.content,
    },
  }] } },
};

describe("RuleDefinitionEditor condition tree", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("blocks a real empty Rust condition draft and keeps its typed repair factory available", async () => {
    render(<Harness initial={input({ operator: "all", children: [] })} />);

    expect(screen.getByRole("alert")).toHaveTextContent("条件树不能为空，请通过下方 Rust 条件工厂添加第一个条件");
    expect(screen.getByRole("button", { name: "保存规则" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "创建 HTTP 条件" })).toBeDisabled();
    const user = userEvent.setup();
    await selectMethodEquals(user);
    await user.type(screen.getByRole("textbox", { name: "HTTP 匹配值" }), "POST");
    expect(screen.getByRole("button", { name: "创建 HTTP 条件" })).toBeEnabled();
  });

  it("inserts a Rust-authored condition into the selected nested group without flattening the tree", async () => {
    commandMocks.ruleDefinitionHttpConditionDraft.mockResolvedValue({ source: "http", field: "Method", operator: { Equals: "POST" } });
    const onSave = vi.fn();
    const initial = input({ operator: "all", children: [httpLeaf, { operator: "any", children: [documentLeaf] }] });
    const user = userEvent.setup();
    render(<Harness initial={initial} onSave={onSave} />);

    await user.click(screen.getByRole("button", { name: "选择 OR 条件组 2 为添加目标" }));
    await user.click(screen.getByRole("button", { name: "在目标组添加 AND 子组" }));
    await selectMethodEquals(user);
    await user.type(screen.getByRole("textbox", { name: "HTTP 匹配值" }), "POST");
    await user.click(screen.getByRole("button", { name: "创建 HTTP 条件" }));
    await waitFor(() => expect(commandMocks.ruleDefinitionHttpConditionDraft).toHaveBeenCalled());
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    expect(onSave).toHaveBeenCalledWith({
      operator: "all",
      children: [
        httpLeaf,
        {
          operator: "any",
          children: [
            documentLeaf,
            { operator: "all", children: [{ operator: "leaf", children: { source: "http", field: "Method", operator: { Equals: "POST" } } }] },
          ],
        },
      ],
    });
  });

  it("uses only Rust-declared HTTP fields, selectors, and operators", async () => {
    commandMocks.ruleDefinitionHttpConditionDraft.mockResolvedValue({ source: "http", field: { Header: "/x-request-id" }, operator: { Contains: "abc" } });
    const user = userEvent.setup();
    render(<Harness initial={input(httpLeaf)} />);

    expect(screen.queryByRole("button", { name: "添加条件：字段" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /HTTP 匹配字段/ }));
    await user.click(await screen.findByRole("option", { name: "Header" }));
    await user.type(screen.getByRole("textbox", { name: "Header selector（/name）" }), "/x-request-id");
    await user.click(screen.getByRole("button", { name: /HTTP 匹配操作符/ }));
    await user.click(await screen.findByRole("option", { name: "contains" }));
    await user.type(screen.getByRole("textbox", { name: "HTTP 匹配值" }), "abc");
    await user.click(screen.getByRole("button", { name: "创建 HTTP 条件" }));

    expect(commandMocks.ruleDefinitionHttpConditionDraft).toHaveBeenCalledWith("header", "/x-request-id", "contains", "abc", "request");
    expect(screen.getAllByText("Request target（路径与查询参数）").length).toBeGreaterThan(0);
  });

  it("shows recursive Schema condition paths separately from the manual path", async () => {
    const schemaContext = structuredClone(context);
    if (schemaContext.content.type !== "http") throw new Error("HTTP context is invalid");
    schemaContext.content.value.stages[0].document_fields = [
      { path: "/payment", label: "/payment", value_type: "object", item_template: false, predicates: [], actions: [] },
      { path: "/payment/method", label: "Payment method", value_type: "string", item_template: false, predicates: ["equals", "contains", "starts_with", "ends_with"], actions: [
        { kind: "set", target_kind: "node", target_value_type: "string", operand_value_type: "string" },
        { kind: "clear", target_kind: "node", target_value_type: "string", operand_value_type: null },
      ] },
      { path: "/items", label: "/items", value_type: "array", item_template: false, predicates: [], actions: [
        { kind: "insert", target_kind: "array", target_value_type: "array", operand_value_type: "object" },
        { kind: "append", target_kind: "array", target_value_type: "array", operand_value_type: "object" },
      ] },
      { path: "/items/*", label: "/items/*", value_type: "object", item_template: true, predicates: [], actions: [] },
      { path: "/items/*/amount", label: "/items/*/amount", value_type: "number", item_template: true, predicates: ["equals"], actions: [] },
    ];
    schemaContext.content.value.stages[0].package = { id: "iso8583", version: "1.0.0" };
    const withDocument = input(httpLeaf);
    if (withDocument.draft.content.type !== "http") throw new Error("HTTP input is invalid");
    withDocument.draft.content.value.document = { package: { id: "iso8583", version: "1.0.0" } };
    const user = userEvent.setup();
    render(<Harness contextOverride={schemaContext} initial={withDocument} />);

    expect(screen.getByRole("button", { name: /Document Schema 条件路径/ })).toBeVisible();
    expect(screen.getByRole("textbox", { name: "手动 Document 条件路径" })).toBeVisible();
    expect(screen.getByText("* 仅匹配一层；展开多个节点时按 ANY 匹配。" )).toBeVisible();

    await user.click(screen.getByRole("button", { name: /Document Schema 条件路径/ }));
    await user.click(await screen.findByRole("option", { name: /Payment method/ }));
    await user.click(screen.getByRole("button", { name: /规则本地动作/ }));
    expect(await screen.findByRole("option", { name: "set" })).toBeVisible();
    expect(screen.getByRole("option", { name: "clear" })).toBeVisible();
    expect(screen.queryByRole("option", { name: "insert" })).not.toBeInTheDocument();
  });

  it("submits the root pointer as empty text and keeps the empty-name property as slash", async () => {
    const schemaContext = structuredClone(context);
    if (schemaContext.content.type !== "http") throw new Error("HTTP context is invalid");
    schemaContext.content.value.stages[0].document_fields = [
      { path: "", label: "Root", value_type: "string", item_template: false, predicates: ["equals"], actions: [
        { kind: "set", target_kind: "node", target_value_type: "string", operand_value_type: "string" },
      ] },
      { path: "/", label: "Empty name", value_type: "string", item_template: false, predicates: ["equals"], actions: [
        { kind: "set", target_kind: "node", target_value_type: "string", operand_value_type: "string" },
      ] },
    ];
    schemaContext.content.value.stages[0].package = { id: "iso8583", version: "1.0.0" };
    const withDocument = input(httpLeaf);
    if (withDocument.draft.content.type !== "http") throw new Error("HTTP input is invalid");
    withDocument.draft.content.value.document = { package: { id: "iso8583", version: "1.0.0" } };
    commandMocks.ruleDefinitionDocumentConditionDraft.mockResolvedValue({ source: "document", path: "", predicate: { type: "string", value: { operator: "equal", value: "root" } } });
    commandMocks.ruleDefinitionDocumentActionDraft.mockResolvedValue({ source: "document", value: { type: "set", path: "", value: "root" } });
    const user = userEvent.setup();
    render(<Harness contextOverride={schemaContext} initial={withDocument} />);

    await user.click(screen.getByRole("button", { name: /Document Schema 条件路径/ }));
    await user.click(await screen.findByRole("option", { name: "Root · /（根）" }));
    await user.type(screen.getByRole("textbox", { name: "JSON 值" }), '"root"');
    await user.click(screen.getByRole("button", { name: /规则本地谓词/ }));
    await user.click(await screen.findByRole("option", { name: "equals" }));
    await user.click(screen.getByRole("button", { name: "创建规则本地元数据条件" }));
    await waitFor(() => expect(commandMocks.ruleDefinitionDocumentConditionDraft).toHaveBeenCalledWith("", "string", "equals", '"root"'));
    await user.click(screen.getByRole("button", { name: /规则本地动作/ }));
    await user.click(await screen.findByRole("option", { name: "set" }));
    await user.click(screen.getByRole("button", { name: "创建规则本地元数据动作" }));
    await waitFor(() => expect(commandMocks.ruleDefinitionDocumentActionDraft).toHaveBeenCalledWith("", "string", "set", '"root"', null));

    await user.click(screen.getByRole("button", { name: /Document Schema 条件路径/ }));
    await user.click(await screen.findByRole("option", { name: "Empty name · /（空名称属性）" }));
    await user.click(screen.getByRole("button", { name: "创建规则本地元数据条件" }));
    await waitFor(() => expect(commandMocks.ruleDefinitionDocumentConditionDraft).toHaveBeenLastCalledWith("/", "string", "equals", '"root"'));
  });

  it("requires an explicit positive integer before creating an nth-hit condition", async () => {
    commandMocks.ruleDefinitionNthHitConditionDraft.mockResolvedValue({ source: "nth_hit", count: 3 });
    const user = userEvent.setup();
    render(<Harness initial={input(httpLeaf)} />);

    const button = screen.getByRole("button", { name: "添加条件：第 N 次命中" });
    const count = screen.getByRole("textbox", { name: "第 N 次命中" });
    expect(button).toBeDisabled();
    await user.type(count, "0");
    expect(button).toBeDisabled();
    await user.clear(count);
    await user.type(count, "3");
    expect(button).toBeEnabled();
    await user.click(button);

    await waitFor(() => expect(commandMocks.ruleDefinitionNthHitConditionDraft).toHaveBeenCalledWith({ count: 3 }));
  });
});

function Harness({ initial, onSave = vi.fn(), contextOverride = context }: { initial: RuleDefinitionSaveInput; onSave?: (condition: RuleDefinitionSaveInput["draft"]["content"]["value"]["condition"]) => void; contextOverride?: RuleEditorContext }) {
  const [value, setValue] = useState(initial);
  return <RuleDefinitionEditor
    context={contextOverride}
    fieldErrors={{}}
    input={value}
    listener={listener}
    loading={false}
    pending={false}
    onChange={(change) => setValue((current) => typeof change === "function" ? change(current) : change)}
    onCopy={vi.fn()}
    onDelete={vi.fn()}
    onSave={() => onSave(value.draft.content.value.condition)}
    onToggle={vi.fn()}
  />;
}

function input(condition: RuleDefinitionSaveInput["draft"]["content"]["value"]["condition"]): RuleDefinitionSaveInput {
  return {
    rule_id: "http-rule",
    expected_revision: 1,
    draft: {
      name: "HTTP rule",
      enabled: true,
      priority: 1,
      listener_id: listener.id,
      stage: "proxy_to_upstream",
      one_shot: false,
      content: { type: "http", value: { description: "", condition, actions: [], document: null } },
    },
  };
}

async function selectMethodEquals(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: /HTTP 匹配字段/ }));
  await user.click(await screen.findByRole("option", { name: "Method" }));
  await user.click(screen.getByRole("button", { name: /HTTP 匹配操作符/ }));
  await user.click(await screen.findByRole("option", { name: "equals" }));
}
