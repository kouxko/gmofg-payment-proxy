// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import {
  ProtocolWorkspaceTabs,
  type ProtocolType,
} from "./protocol-workspace-tabs";

function ProtocolTabsHarness() {
  const [protocol, setProtocol] = useState<ProtocolType>("http");
  return (
    <ProtocolWorkspaceTabs
      ariaLabel="测试协议"
      pageTitle="测试工作区"
      selectedKey={protocol}
      onSelectionChange={setProtocol}
    >
      <section aria-label={`${protocol} content`}>{protocol} content</section>
    </ProtocolWorkspaceTabs>
  );
}

describe("ProtocolWorkspaceTabs", () => {
  it("renders an optional page title with full-width protocol tabs", () => {
    render(<ProtocolTabsHarness />);

    expect(screen.getByRole("heading", { level: 1, name: "测试工作区" })).toBeVisible();
    const tablist = screen.getByRole("tablist", { name: "测试协议" });
    expect(tablist).not.toHaveClass("w-fit");
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "HTTP",
      "Socket",
    ]);
  });

  it("keeps the header bounded without horizontal overflow classes", () => {
    render(<ProtocolTabsHarness />);

    const header = screen.getByRole("heading", { name: "测试工作区" }).closest("header");
    expect(header).toHaveClass("shrink-0");
    expect(header?.className).not.toMatch(/(?:^|\s)(?:w-screen|min-w-max)(?:\s|$)/);
  });

  it("does not render a duplicate title when the workspace owns its heading", () => {
    render(
      <ProtocolWorkspaceTabs
        ariaLabel="测试协议"
        selectedKey="http"
        onSelectionChange={() => undefined}
      >
        <section>content</section>
      </ProtocolWorkspaceTabs>,
    );

    expect(screen.queryByRole("heading")).not.toBeInTheDocument();
  });

  it("connects the selected tab to the active tabpanel", () => {
    render(<ProtocolTabsHarness />);

    const tab = screen.getByRole("tab", { name: "HTTP" });
    const panel = screen.getByRole("tabpanel");
    expect(tab).toHaveAttribute("aria-controls", panel.id);
    expect(panel).toHaveAttribute("aria-labelledby", tab.id);
    expect(screen.getByRole("region", { name: "http content" })).toBeVisible();
    expect(screen.queryByRole("region", { name: "socket content" })).toBeNull();
  });

  it.each([
    ["{ArrowRight}", "Socket"],
    ["{End}", "Socket"],
    ["{ArrowLeft}", "Socket"],
  ])("selects Socket from HTTP with %s", async (key, expected) => {
    const user = userEvent.setup();
    render(<ProtocolTabsHarness />);
    screen.getByRole("tab", { name: "HTTP" }).focus();

    await user.keyboard(key);

    expect(screen.getByRole("tab", { name: expected })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.queryByRole("region", { name: "http content" })).toBeNull();
    expect(screen.getByRole("region", { name: "socket content" })).toBeVisible();
  });

  it("selects HTTP from Socket with Home", async () => {
    const user = userEvent.setup();
    render(<ProtocolTabsHarness />);
    await user.click(screen.getByRole("tab", { name: "Socket" }));

    await user.keyboard("{Home}");

    expect(screen.getByRole("tab", { name: "HTTP" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it.each(["{Enter}", " "])("activates a focused tab with %s", async (key) => {
    const user = userEvent.setup();
    render(<ProtocolTabsHarness />);
    const socketTab = screen.getByRole("tab", { name: "Socket" });
    socketTab.focus();

    await user.keyboard(key);

    expect(socketTab).toHaveAttribute("aria-selected", "true");
  });
});
