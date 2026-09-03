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
  });

  it("does not describe the removed nth-hit rule behavior", () => {
    const allGuidance = Object.values(pageHelpGuides)
      .flatMap((guide) => guide.sections)
      .flatMap((section) => section.steps)
      .join("\n");

    expect(allGuidance).not.toMatch(/第 N 次命中|默认命中次数|一次性生效|仅命中一次/);
  });

  it("documents standalone weak-network setup before optional proxy settings", () => {
    const androidGuide = pageHelpGuides["/android-network"];
    const androidGuidance = androidGuide.sections
      .flatMap((section) => section.steps)
      .join("\n");

    expect(androidGuide.summary).toContain("单独运行弱网");
    expect(androidGuidance).toContain("不配置代理入口也可以单独保存并启动弱网");
    expect(androidGuidance).toContain("参考慢速 4G");
    expect(androidGuidance).toContain("RTT 换算为单向延迟");
    expect(androidGuidance).toContain("同时接入代理调试");
    expect(androidGuidance).toContain("专家参数");
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

  it("documents capture lifecycle as connection status without inferring a business result", () => {
    const captureGuide = pageHelpGuides["/capture"].sections
      .flatMap((section) => section.steps)
      .join("\n");

    expect(captureGuide).toContain("连接状态");
    expect(captureGuide).toContain("保持连接");
    expect(captureGuide).toContain("正常结束");
    expect(captureGuide).toContain("异常结束");
    expect(captureGuide).not.toContain("最终结果");
    expect(captureGuide).not.toContain("支付成功");
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
