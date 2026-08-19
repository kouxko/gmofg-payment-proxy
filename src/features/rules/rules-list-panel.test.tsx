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

function renderPanel() {
  const onSelect = vi.fn();
  const onSelectBody = vi.fn();
  const onToggleBody = vi.fn();
  render(
    <RulesListPanel
      rules={[standardRule]}
      bodyRules={[bodyRule]}
      bodyListenerNames={new Map([["http-entry", "支付入口"]])}
      isLoading={false}
      writePending={false}
      editorBlocked={false}
      onNew={vi.fn()}
      onImport={vi.fn()}
      onExport={vi.fn()}
      onRefresh={vi.fn()}
      onSelect={onSelect}
      onSelectBody={onSelectBody}
      onToggle={vi.fn()}
      onToggleBody={onToggleBody}
    />,
  );
  return { onSelect, onSelectBody, onToggleBody };
}

describe("unified HTTP rule list", () => {
  it("renders standard and Body rules in one table without a second type tab", () => {
    renderPanel();

    expect(screen.getByText("Header 规则")).toBeVisible();
    expect(screen.getByText("金额修正规则")).toBeVisible();
    expect(screen.getByText("Body 报文")).toBeVisible();
    expect(screen.queryByRole("tab", { name: "常规规则" })).toBeNull();
    expect(screen.queryByRole("tab", { name: "Body 报文规则" })).toBeNull();
    expect(screen.getByText(/先执行 HTTP 基础规则，再执行 Body 报文规则/)).toBeVisible();
    expect(document.querySelector('[data-key="body:body-1"]')).toBeTruthy();
  });

  it("routes Body row selection and toggling through the Body callbacks", async () => {
    const user = userEvent.setup();
    const { onSelect, onSelectBody, onToggleBody } = renderPanel();

    await user.click(screen.getByText("金额修正规则"));
    expect(onSelectBody).toHaveBeenCalledWith("body-1");
    expect(onSelect).not.toHaveBeenCalled();

    await user.click(
      screen.getByRole("switch", { name: "启用 Body 报文规则 金额修正规则" }),
    );
    expect(onToggleBody).toHaveBeenCalledWith(bodyRule, true);
  });
});
