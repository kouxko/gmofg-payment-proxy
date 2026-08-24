// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type {
  ProtocolDocumentRuleDefinition,
  RuleSummaryViewModel,
} from "@/generated/rust-types";
import { RulesListPanel } from "./rules-list-panel";

const standardRule: RuleSummaryViewModel = {
  rule_id: "standard-1",
  revision: 1,
  name: "Header 规则",
  enabled: true,
  priority: 10,
  creation_order: 1,
  channel_text: "支付",
  stage_text: "请求",
  match_summary: "Header 匹配",
  action_summary: "修改 Header",
  hit_count: 2,
  last_hit_at: null,
  ui_tone: "positive",
};

const bodyRule: ProtocolDocumentRuleDefinition = {
  rule_id: "body-1",
  revision: 3,
  name: "金额修正规则",
  enabled: false,
  priority: 20,
  created_order: 2,
  listener_id: "http-entry",
  package: { id: "iso8583", version: "1.0.0" },
  schema_version: 1,
  stage: "app_to_proxy",
  conditions: [],
  actions: [{ type: "record_match" }],
};

const socketRule: ProtocolDocumentRuleDefinition = {
  ...bodyRule,
  rule_id: "socket-1",
  name: "Socket 金额修正规则",
  listener_id: "socket-entry",
  stage: "proxy_to_upstream",
};

function renderPanel({
  editorBlocked = false,
  writePending = false,
}: {
  editorBlocked?: boolean;
  writePending?: boolean;
} = {}) {
  const onSelect = vi.fn();
  const onSelectProtocol = vi.fn();
  const onToggleProtocol = vi.fn();
  render(
    <RulesListPanel
      rules={[standardRule]}
      bodyRules={[bodyRule]}
      bodyListenerNames={new Map([["http-entry", "支付入口"]])}
      socketRules={[socketRule]}
      socketListenerNames={new Map([["socket-entry", "收单入口"]])}
      isLoading={false}
      writePending={writePending}
      editorBlocked={editorBlocked}
      onNew={vi.fn()}
      onRefresh={vi.fn()}
      onSelect={onSelect}
      onSelectProtocol={onSelectProtocol}
      onToggle={vi.fn()}
      onToggleProtocol={onToggleProtocol}
    />,
  );
  return { onSelect, onSelectProtocol, onToggleProtocol };
}

describe("unified HTTP and Socket rule list", () => {
  it("uses one rule workspace and one create action", () => {
    renderPanel();

    expect(screen.getByRole("heading", { name: "规则" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "HTTP 规则" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Socket 报文规则" })).not.toBeInTheDocument();
    expect(screen.getByText("规则按优先级数值从小到大逐条匹配；同优先级按创建顺序执行。")).toBeVisible();
    expect(screen.getByRole("button", { name: "新建规则" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "导入规则" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "导出规则" })).not.toBeInTheDocument();
    expect(screen.queryByText("执行顺序")).not.toBeInTheDocument();
  });

  it("keeps new-rule creation available when only the current draft is invalid", async () => {
    const user = userEvent.setup();
    renderPanel({ editorBlocked: true });

    const button = screen.getByRole("button", { name: "新建规则" });
    expect(button).toBeEnabled();
    await user.click(button);
  });

  it("blocks new-rule creation while a write is in progress", () => {
    renderPanel({ writePending: true });

    expect(screen.getByRole("button", { name: "新建规则" })).toBeDisabled();
  });

  it("renders HTTP standard, HTTP Body and Socket rules in one table", () => {
    renderPanel();

    expect(screen.getByText("Header 规则")).toBeVisible();
    expect(screen.getByText("金额修正规则")).toBeVisible();
    expect(screen.getByText("Socket 金额修正规则")).toBeVisible();
    expect(screen.getAllByText("HTTP").length).toBeGreaterThan(0);
    expect(screen.getByText("HTTP Body")).toBeVisible();
    expect(screen.getByText("Socket")).toBeVisible();
    expect(screen.queryByRole("tab", { name: "常规规则" })).toBeNull();
    expect(screen.queryByRole("tab", { name: "Body 报文规则" })).toBeNull();
    expect(screen.getByText(/规则按优先级数值从小到大逐条匹配/)).toBeVisible();
    expect(document.querySelector('[data-key="body:body-1"]')).toBeTruthy();
    expect(document.querySelector('[data-key="socket:socket-1"]')).toBeTruthy();
  });

  it("routes protocol row selection and toggling with its protocol kind", async () => {
    const user = userEvent.setup();
    const { onSelect, onSelectProtocol, onToggleProtocol } = renderPanel();

    await user.click(screen.getByText("金额修正规则"));
    expect(onSelectProtocol).toHaveBeenCalledWith("body", "body-1");
    expect(onSelect).not.toHaveBeenCalled();

    await user.click(
      screen.getByRole("switch", { name: "启用 Body 报文规则 金额修正规则" }),
    );
    expect(onToggleProtocol).toHaveBeenCalledWith("body", bodyRule, true);

    await user.click(screen.getByText("Socket 金额修正规则"));
    expect(onSelectProtocol).toHaveBeenCalledWith("socket", "socket-1");
  });
});
