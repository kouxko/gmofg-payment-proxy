// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { WorkspacePath } from "@/features/shell/workspace-navigation";
import { PageHelp } from "./page-help";
import { pageHelpGuides } from "./page-help-content";

const workspacePaths: WorkspacePath[] = [
  "/console",
  "/capture",
  "/sessions",
  "/breakpoints",
  "/rules",
  "/faults",
  "/certificates",
  "/settings",
];

describe("page-specific usage guides", () => {
  it("covers every frozen workspace page with detailed instructions", () => {
    expect(Object.keys(pageHelpGuides)).toEqual(workspacePaths);

    for (const path of workspacePaths) {
      const guide = pageHelpGuides[path];
      expect(guide.title.length).toBeGreaterThan(0);
      expect(guide.summary.length).toBeGreaterThan(20);
      expect(guide.recommendedFor.length).toBeGreaterThan(20);
      expect(guide.sections.length).toBeGreaterThanOrEqual(5);
      expect(guide.sections.every((section) => section.steps.length >= 4)).toBe(
        true,
      );
    }
  });

  it("opens the current page guide in a Drawer without document navigation", async () => {
    const user = userEvent.setup();
    const documentUrl = window.location.href;
    render(<PageHelp pathname="/console" />);

    await user.click(
      screen.getByRole("button", { name: "打开代理控制台使用说明" }),
    );

    expect(
      screen.getByRole("dialog", { name: "代理控制台使用说明" }),
    ).toBeVisible();
    expect(screen.getByText("首次运行前的准备")).toBeVisible();
    expect(screen.getByText("真实设备链路的成功判定")).toBeVisible();
    expect(window.location.href).toBe(documentUrl);
  });

  it("changes the guide content with the active workspace page", async () => {
    const user = userEvent.setup();
    const view = render(<PageHelp pathname="/capture" />);

    await user.click(
      screen.getByRole("button", { name: "打开实时抓包使用说明" }),
    );
    expect(
      screen.getByRole("dialog", { name: "实时抓包使用说明" }),
    ).toBeVisible();
    await user.click(
      screen.getByRole("button", { name: "关闭使用说明" }),
    );

    view.rerender(<PageHelp pathname="/certificates" />);
    await user.click(
      screen.getByRole("button", { name: "打开证书管理使用说明" }),
    );

    expect(
      screen.getByRole("dialog", { name: "证书管理使用说明" }),
    ).toBeVisible();
    expect(screen.getByText("首次配置推荐顺序")).toBeVisible();
  });
});
