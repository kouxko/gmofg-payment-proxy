// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { Condition, UnifiedAction } from "@/generated/rust-types";
import { FlatConditionList, OrderedActionList } from "./rule-list-editors";

const conditions: Condition[] = [
  { source: "http", field: "RequestTarget", operator: { Wildcard: "/customer/*/name" } },
  { source: "document", path: "/customer/age", predicate: { type: "number", value: { operator: "equal", value: 18 } } },
];

describe("flat rule editors", () => {
  it("renders a fixed AND condition list and removes one row without tree controls", async () => {
    const onChange = vi.fn();
    render(<FlatConditionList conditions={conditions} onChange={onChange} />);

    expect(screen.getByText("所有条件固定为 AND；需要 OR 时请新建多条规则。")).toBeVisible();
    expect(screen.queryByText(/条件树|子组|切换为 OR/)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "删除条件 2" }));
    expect(onChange).toHaveBeenCalledWith([conditions[0]]);
  });

  it("allows removing the final condition so the editor save gate can require a replacement", async () => {
    const onChange = vi.fn();
    render(<FlatConditionList conditions={[conditions[0]]} onChange={onChange} />);

    await userEvent.click(screen.getByRole("button", { name: "删除条件 1" }));
    expect(onChange).toHaveBeenCalledWith([]);
  });

  it("keeps ordered actions independently editable", async () => {
    const actions: UnifiedAction[] = [{ source: "record_match" }, { source: "record_match" }];
    const onChange = vi.fn();
    render(<OrderedActionList actions={actions} label={() => "记录命中"} onChange={onChange} />);

    await userEvent.click(screen.getByRole("button", { name: "上移动作 2" }));
    expect(onChange).toHaveBeenCalledWith([actions[1], actions[0]]);
  });
});
