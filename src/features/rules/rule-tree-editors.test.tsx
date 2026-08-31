// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ConditionTree, UnifiedAction } from "@/generated/rust-types";
import type { DocumentSchemaField } from "./rule-document-schema";
import { ConditionTreeEditor, DocumentMetadataTree, OrderedActionList, pointerTokens } from "./rule-tree-editors";

const fields: DocumentSchemaField[] = [
  { name: "/payment/amount", label: "Amount", type: "number", itemTemplate: false, predicates: [], actions: [] },
  { name: "/payment/currency", label: "Currency", type: "string", itemTemplate: false, predicates: [], actions: [] },
  { name: "/items/*", label: "Line item", type: "object", itemTemplate: true, predicates: [], actions: [] },
  { name: "/items/*/amount", label: "Item amount", type: "number", itemTemplate: true, predicates: [], actions: [] },
];

const nestedCondition: ConditionTree = {
  operator: "all",
  children: [
    documentLeaf("/payment/amount", 100),
    { operator: "any", children: [documentLeaf("/payment/currency", 0), documentLeaf("/items/0/amount", 5)] },
  ],
};

describe("Phase15 recursive rule editors", () => {
  it("keeps the Document root distinct from the empty-name property", () => {
    const rootAndEmptyNameFields: DocumentSchemaField[] = [
      { name: "", label: "Document root", type: "object", itemTemplate: false, predicates: [], actions: [] },
      { name: "/", label: "Empty-name property", type: "string", itemTemplate: false, predicates: [], actions: [] },
    ];
    const condition: ConditionTree = {
      operator: "all",
      children: [documentLeaf("", 1), documentLeaf("", 2), documentLeaf("/", 3)],
    };

    expect(pointerTokens("")).toEqual([]);
    expect(pointerTokens("/")).toEqual([""]);

    render(<DocumentMetadataTree condition={condition} fields={rootAndEmptyNameFields} />);

    const tree = screen.getByRole("tree", { name: "Schema metadata tree" });
    expect(within(tree).getByRole("treeitem", { name: /Document root object 只读/ })).toHaveTextContent("条件 2");
    expect(within(tree).getByRole("treeitem", { name: /Empty-name property string 只读/ })).toHaveTextContent("条件 1");
  });

  it("renders recursive readonly Schema metadata with concrete array indices and condition counts", () => {
    render(<DocumentMetadataTree condition={nestedCondition} fields={fields} />);

    expect(screen.getByRole("tree", { name: "Schema metadata tree" })).toBeVisible();
    expect(screen.getByRole("treeitem", { name: /Amount number 只读/ })).toHaveAttribute("data-readonly", "true");
    expect(within(screen.getByRole("tree", { name: "Schema metadata tree" })).getByRole("treeitem", { name: /^items group/ })).toHaveTextContent("Array items template");
  });

  it("separates readonly array item metadata from a user-created concrete index", () => {
    render(<DocumentMetadataTree
      condition={documentLeaf("/items/3/amount", 5)}
      fields={fields}
      localFields={[{ ...fields[3], name: "/items/3/amount", label: "/items/3/amount", itemTemplate: false }]}
    />);

    expect(screen.getByRole("tree", { name: "Schema metadata tree" })).toHaveTextContent("Array items template");
    expect(screen.getByRole("tree", { name: "Rule-local metadata tree" })).toHaveTextContent("Array index 3");
    expect(screen.getByRole("tree", { name: "Rule-local metadata tree" })).not.toHaveTextContent("Array index 0");
  });

  it("marks rule-local metadata as editable while leaving Schema metadata readonly", () => {
    const local = documentLeaf("/custom/value", 7);
    render(<DocumentMetadataTree condition={local} fields={[...fields, {
      name: "/custom/value", label: "/custom/value", type: "number", itemTemplate: false, predicates: [], actions: [],
    }]} readonlyPaths={new Set(fields.map((field) => field.name))} />);

    expect(screen.getByRole("treeitem", { name: /custom\/value number/ })).toHaveAttribute("data-readonly", "false");
    expect(screen.getByRole("treeitem", { name: /Amount number 只读/ })).toHaveAttribute("data-readonly", "true");
  });

  it("preserves nested AND/OR structure and applies an explicit operator edit", async () => {
    const onChange = vi.fn();
    render(<ConditionTreeEditor tree={nestedCondition} onChange={onChange} />);
    expect(screen.getByText("AND 条件组")).toBeVisible();
    expect(screen.getByText("OR 条件组")).toBeVisible();

    await userEvent.setup().click(screen.getAllByRole("button", { name: "切换为 OR" })[0]);
    expect(onChange).toHaveBeenCalledWith({ ...nestedCondition, operator: "any" });
  });

  it("selects a nested group as the insertion target and requests a leaf or non-empty subgroup", async () => {
    const onInsertRequest = vi.fn();
    const user = userEvent.setup();
    render(<ConditionTreeEditor tree={nestedCondition} onChange={vi.fn()} onInsertRequest={onInsertRequest} />);

    await user.click(screen.getByRole("button", { name: "选择 OR 条件组 2 为添加目标" }));
    expect(screen.getByRole("group", { name: "OR 条件组 2" })).toHaveAttribute("data-insertion-target", "true");

    await user.click(screen.getByRole("button", { name: "在目标组添加条件" }));
    expect(onInsertRequest).toHaveBeenLastCalledWith([1], null);

    await user.click(screen.getByRole("button", { name: "在目标组添加 AND 子组" }));
    expect(onInsertRequest).toHaveBeenLastCalledWith([1], "all");
  });

  it("reorders the unified action list without changing action payloads", async () => {
    const actions: UnifiedAction[] = [
      { source: "record_match" },
      { source: "http", value: { SetHeader: { name: "x-first", value: "1" } } },
      { source: "http", value: { Delay: { milliseconds: 5 } } },
    ];
    const onChange = vi.fn();
    render(<OrderedActionList actions={actions} label={(action) => action.source} onChange={onChange} />);

    await userEvent.setup().click(screen.getByRole("button", { name: "下移动作 1" }));
    expect(onChange).toHaveBeenCalledWith([actions[1], actions[0], actions[2]]);
  });
});

function documentLeaf(path: string, value: number): ConditionTree {
  return { operator: "leaf", children: { source: "document", path, predicate: { type: "number", value: { operator: "equal", value } } } };
}
