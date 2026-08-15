// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ProxyListener, SocketDocumentRuleDefinition } from "@/generated/rust-types";
import { SocketRulesList } from "./socket-rules-list";

const listener = {
  id: "relay",
  name: "交易中继",
} as ProxyListener;
const rule: SocketDocumentRuleDefinition = {
  rule_id: "rule-1",
  revision: 2,
  enabled: true,
  priority: 10,
  created_order: 1,
  listener_id: "relay",
  package: { id: "iso8583", version: "1.2.3" },
  schema_version: 7,
  direction: "upstream",
  conditions: [],
  actions: [{ type: "record_match" }],
};

function renderList(overrides: Partial<Parameters<typeof SocketRulesList>[0]> = {}) {
  const props: Parameters<typeof SocketRulesList>[0] = {
    rules: [],
    listeners: [listener],
    selectedId: undefined,
    loading: false,
    error: undefined,
    pending: false,
    onNew: vi.fn(),
    onSelect: vi.fn(),
    onToggle: vi.fn(),
    onRetry: vi.fn(),
    ...overrides,
  };
  render(<SocketRulesList {...props} />);
  return props;
}

describe("Socket rules list states", () => {
  it("shows an accessible loading state", () => {
    renderList({ loading: true });
    expect(screen.getByLabelText("正在读取 Socket 规则")).toBeVisible();
  });

  it("shows a read error and retries all sources", async () => {
    const user = userEvent.setup();
    const props = renderList({ error: "数据库不可用" });
    expect(screen.getByText("数据库不可用")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "重试" }));
    expect(props.onRetry).toHaveBeenCalledOnce();
  });

  it("explains why creation is unavailable without a scripted listener", () => {
    renderList({ listeners: [] });
    expect(screen.getByText("当前 Workspace 没有 Scripted Socket Listener")).toBeVisible();
    expect(screen.getByRole("button", { name: "新建 Socket 规则" })).toBeDisabled();
  });

  it("shows an empty state when listeners exist but no rules do", () => {
    renderList();
    expect(screen.getByText("暂无 Socket 规则")).toBeVisible();
  });

  it("starts creation exactly once", async () => {
    const user = userEvent.setup();
    const props = renderList();
    await user.click(screen.getByRole("button", { name: "新建 Socket 规则" }));
    expect(props.onNew).toHaveBeenCalledOnce();
  });

  it("renders a selected rule with binding and priority details", () => {
    renderList({ rules: [rule], selectedId: "rule-1" });
    expect(screen.getByRole("listitem")).toHaveAttribute("aria-current", "true");
    expect(screen.getByText("iso8583@1.2.3 · Schema v7")).toBeVisible();
    expect(screen.getByText("P10")).toBeVisible();
  });

  it("falls back to listener id and downstream label for an orphaned binding", () => {
    renderList({ rules: [{ ...rule, listener_id: "missing", direction: "downstream" }] });
    expect(screen.getByText("missing")).toBeVisible();
    expect(screen.getByText("downstream")).toBeVisible();
  });

  it("selects a rule from its row without toggling it", async () => {
    const user = userEvent.setup();
    const props = renderList({ rules: [rule] });
    await user.click(screen.getByRole("button", { name: /交易中继/ }));
    expect(props.onSelect).toHaveBeenCalledWith(rule);
    expect(props.onToggle).not.toHaveBeenCalled();
  });

  it("passes the inverse state through the row switch", async () => {
    const user = userEvent.setup();
    const props = renderList({ rules: [rule] });
    await user.click(screen.getByRole("switch", { name: "停用 Socket 规则 rule-1" }));
    expect(props.onToggle).toHaveBeenCalledWith(rule, false);
  });

  it("disables creation and toggles while a mutation is pending", () => {
    renderList({ rules: [rule], pending: true });
    expect(screen.getByRole("button", { name: "新建 Socket 规则" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "停用 Socket 规则 rule-1" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /交易中继/ })).toBeDisabled();
  });

  it("blocks only side effects while a value parser is pending", () => {
    renderList({ rules: [rule], sideEffectsDisabled: true });
    expect(screen.getByRole("switch", { name: "停用 Socket 规则 rule-1" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /交易中继/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: "新建 Socket 规则" })).toBeEnabled();
  });
});
