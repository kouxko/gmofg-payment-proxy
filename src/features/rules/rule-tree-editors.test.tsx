// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ConditionTree, ProtocolRuleFieldCapability, UnifiedAction } from "@/generated/rust-types";
import { ConditionTreeEditor, DocumentMetadataTree, OrderedActionList } from "./rule-tree-editors";

const fields: ProtocolRuleFieldCapability[] = [
  { name: "/payment/amount", label: "Amount", type: "number", operators: ["equals"], actions: ["set_field", "clear_field"] },
  { name: "/payment/currency", label: "Currency", type: "string", operators: ["equals"], actions: ["set_field"] },
  { name: "/items/0", label: "Line item", type: "object", operators: ["equals"], actions: ["set_field"] },
  { name: "/items/0/amount", label: "Item amount", type: "number", operators: ["equals"], actions: ["set_field"] },
];

const nestedCondition: ConditionTree = {
  operator: "all",
  children: [
    documentLeaf("/payment/amount", 100),
    { operator: "any", children: [documentLeaf("/payment/currency", 0), documentLeaf("/items/0/amount", 5)] },
  ],
};

describe("Phase15 recursive rule editors", () => {
  it("renders recursive readonly Schema metadata with concrete array indices and condition counts", () => {
    render(<DocumentMetadataTree condition={nestedCondition} fields={fields} />);

    expect(screen.getByRole("tree", { name: "Schema metadata tree" })).toBeVisible();
    expect(screen.getByRole("treeitem", { name: /Amount number 只读/ })).toHaveAttribute("data-readonly", "true");
    expect(within(screen.getByRole("tree", { name: "Schema metadata tree" })).getByRole("treeitem", { name: /^items group/ })).toHaveTextContent("Array items");
    expect(screen.queryByText(/Item template/i)).not.toBeInTheDocument();
  });

  it("separates readonly array item metadata from a user-created concrete index", () => {
    render(<DocumentMetadataTree
      condition={documentLeaf("/items/3/amount", 5)}
      fields={fields}
      localFields={[{ ...fields[3], name: "/items/3/amount", label: "/items/3/amount" }]}
    />);

    expect(screen.getByRole("tree", { name: "Schema metadata tree" })).toHaveTextContent("Array items");
    expect(screen.getByRole("tree", { name: "Rule-local metadata tree" })).toHaveTextContent("Array index 3");
    expect(screen.getByRole("tree", { name: "Rule-local metadata tree" })).not.toHaveTextContent("Array index 0");
  });

  it("marks rule-local metadata as editable while leaving Schema metadata readonly", () => {
    const local = documentLeaf("/custom/value", 7);
    render(<DocumentMetadataTree condition={local} fields={[...fields, {
      name: "/custom/value", label: "/custom/value", type: "number", operators: ["equals"], actions: ["set_field", "clear_field"],
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
