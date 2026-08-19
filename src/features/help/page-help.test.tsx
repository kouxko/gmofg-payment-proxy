// @vitest-environment jsdom

/** 验证各页面帮助 Drawer 可访问且不会触发工作区导航或业务命令。 */

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { WorkspacePath } from "@/features/shell/workspace-navigation";
import { PageHelp } from "./page-help";
import { pageHelpGuides } from "./page-help-content";

const workspacePaths: WorkspacePath[] = [
  "/workspaces",
  "/listeners",
  "/protocol-packages",
  "/android-network",
  "/diagnostics",
  "/capture",
  "/rules",
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

  it("keeps entry guidance free of removed standalone pages", () => {
    const listenerGuide = pageHelpGuides["/listeners"].sections
      .flatMap((section) => section.steps)
      .join("\n");

    expect(listenerGuide).toContain("HTTP 或 Socket");
    expect(listenerGuide).not.toContain("运行监控");
    expect(listenerGuide).not.toContain("断点实验台");
  });

  it("documents the native and non-mutating protocol-package import boundary", () => {
    const protocolPackageGuide = pageHelpGuides["/protocol-packages"].sections
      .flatMap((section) => section.steps)
      .join("\n");

    expect(protocolPackageGuide).toContain("原生文件选择器读取文件");
    expect(protocolPackageGuide).toContain("页面不会接收本机路径或 ZIP 字节");
    expect(protocolPackageGuide).toContain("此阶段不会安装任何内容");
    expect(protocolPackageGuide).toContain("新安装版本默认停用");
    expect(protocolPackageGuide).toContain("导入不会自动修改或重绑任何入口");
  });

  it("opens the current page guide in a Drawer without document navigation", async () => {
    const user = userEvent.setup();
    const documentUrl = window.location.href;
    render(<PageHelp pathname="/workspaces" />);

    await user.click(
      screen.getByRole("button", { name: "打开Workspace 管理使用说明" }),
    );

    expect(
      screen.getByRole("dialog", { name: "Workspace 管理使用说明" }),
    ).toBeVisible();
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
