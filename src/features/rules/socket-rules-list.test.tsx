// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ProxyListener, ProtocolDocumentRuleDefinition } from "@/generated/rust-types";
import { ProtocolRulesList } from "./protocol-rules-list";

const listener = {
  id: "relay",
  name: "交易中继",
} as ProxyListener;
const rule: ProtocolDocumentRuleDefinition = {
  rule_id: "rule-1",
  revision: 2,
  name: "金额修正规则",
  enabled: true,
  priority: 10,
  created_order: 1,
  listener_id: "relay",
  package: { id: "iso8583", version: "1.2.3" },
  schema_version: 7,
  stage: "app_to_proxy",
  conditions: [],
  actions: [{ type: "record_match" }],
};

function renderList(overrides: Partial<Parameters<typeof ProtocolRulesList>[0]> = {}) {
  const props: Parameters<typeof ProtocolRulesList>[0] = {
    kind: "socket",
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
  render(<ProtocolRulesList {...props} />);
  return props;
}

describe("Socket rules list states", () => {
  it("shows an accessible loading state", () => {
    renderList({ loading: true });
    expect(screen.getByLabelText("正在读取报文规则")).toBeVisible();
  });

  it("shows a read error and retries all sources", async () => {
    const user = userEvent.setup();
    const props = renderList({ error: "数据库不可用" });
    expect(screen.getByText("数据库不可用")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "重试" }));
    expect(props.onRetry).toHaveBeenCalledOnce();
  });

  it("explains why creation is unavailable without a protocol-enabled entry", () => {
    renderList({ listeners: [] });
    expect(screen.getByText("当前工作区没有可配置报文规则的协议入口")).toBeVisible();
    expect(screen.getByText("请先在入口配置中选择一个协议处理方案。")).toBeVisible();
    expect(screen.getByRole("button", { name: "新建报文规则" })).toBeDisabled();
  });

  it("shows an empty state when listeners exist but no rules do", () => {
    renderList();
    expect(screen.getByText("暂无报文规则")).toBeVisible();
    expect(
      screen.getByText("每个链路阶段单独配置，规则只在所选阶段执行。"),
    ).toBeVisible();
  });

  it("starts creation exactly once", async () => {
    const user = userEvent.setup();
    const props = renderList();
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    expect(props.onNew).toHaveBeenCalledOnce();
  });

  it("renders a selected rule with binding and priority details", () => {
    renderList({ rules: [rule], selectedId: "rule-1" });
    expect(screen.getByRole("listitem")).toHaveAttribute("aria-current", "true");
    expect(screen.getByText("交易中继 · iso8583@1.2.3")).toBeVisible();
    expect(screen.getByText("P10")).toBeVisible();
  });

  it("falls back to entry id and stage label for an orphaned binding", () => {
    renderList({ rules: [{ ...rule, listener_id: "missing", stage: "proxy_to_app" }] });
    expect(screen.getByText("missing · iso8583@1.2.3")).toBeVisible();
    expect(screen.getByText("代理 → 应用")).toBeVisible();
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
    await user.click(screen.getByRole("switch", { name: "停用报文规则 rule-1" }));
    expect(props.onToggle).toHaveBeenCalledWith(rule, false);
  });

  it("disables creation and toggles while a mutation is pending", () => {
    renderList({ rules: [rule], pending: true });
    expect(screen.getByRole("button", { name: "新建报文规则" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "停用报文规则 rule-1" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /交易中继/ })).toBeDisabled();
  });

  it("blocks only side effects while a value parser is pending", () => {
    renderList({ rules: [rule], sideEffectsDisabled: true });
    expect(screen.getByRole("switch", { name: "停用报文规则 rule-1" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /交易中继/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: "新建报文规则" })).toBeEnabled();
  });
});
