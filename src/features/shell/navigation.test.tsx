// @vitest-environment jsdom

/** 验证桌面外壳导航、移动 Drawer 和选中态不会触发整页刷新。 */

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import {
  AppShell,
  navigation,
  shellErrorRegionClassName,
  sideNavigationClassName,
  sideNavigationIconClassName,
  sideNavigationItemClassName,
  sideNavigationLabelClassName,
} from "./app-shell";

const workspaceNavigationMocks = vi.hoisted(() => ({
  navigate: vi.fn(),
}));

vi.mock("./workspace-navigation", () => ({
  useWorkspaceNavigation: () => ({
    pathname: "/console",
    searchParams: new URLSearchParams(),
    navigate: workspaceNavigationMocks.navigate,
  }),
}));

vi.mock("./bootstrap-context", () => ({
  BootstrapProvider: ({ children }: { children: React.ReactNode }) => children,
  useAppEventRefresh: vi.fn(),
  useBootstrap: () => ({
    bootstrap: undefined,
    proxy: undefined,
    isLoading: false,
    error: undefined,
    refresh: vi.fn(),
  }),
}));

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (queryKey: string) =>
    queryKey === "shell-workspaces"
      ? {
          data: [
            {
              id: "workspace-1",
              name: "测试工作区",
              selected: true,
              listener_count: 1,
              enabled_listener_count: 0,
              revision: 1,
            },
          ],
          isLoading: false,
          refresh: vi.fn(),
        }
      : {
          data: {
            workspace_id: "workspace-1",
            workspace_name: "测试工作区",
            state_text: "全部入口已停止",
            ui_tone: "neutral",
            total_count: 1,
            active_count: 0,
            faulted_count: 0,
            rows: [],
          },
          isLoading: false,
          refresh: vi.fn(),
        },
}));

describe("UI-001 fixed navigation order", () => {
  it("matches the frozen requirement document", () => {
    expect(navigation.map((item) => item.href)).toEqual([
      "/workspaces",
      "/listeners",
      "/android-network",
      "/diagnostics",
      "/console",
      "/capture",
      "/sessions",
      "/breakpoints",
      "/rules",
      "/faults",
      "/certificates",
      "/settings",
    ]);
  });
});

describe("side navigation alignment", () => {
  it("insets the shared item box and keeps visible space between tabs", () => {
    expect(sideNavigationClassName).toContain("gap-2");
    expect(sideNavigationClassName).toContain("px-2");
    expect(sideNavigationItemClassName).toContain("!w-full");
    expect(sideNavigationItemClassName).toContain("items-center");
    expect(sideNavigationItemClassName).toContain("justify-center");
    expect(sideNavigationItemClassName).toContain("text-center");
  });

  it("uses the same centered icon and label contract for links and About", () => {
    expect(sideNavigationIconClassName).toContain("self-center");
    expect(sideNavigationIconClassName).toContain("shrink-0");
    expect(sideNavigationLabelClassName).toContain("w-14");
    expect(sideNavigationLabelClassName).toContain("shrink-0");
    expect(sideNavigationLabelClassName).toContain("whitespace-nowrap");
    expect(sideNavigationLabelClassName).toContain("text-center");
  });
});

describe("shell content boundary", () => {
  it("keeps the full-width Rust error alert inside the right page edge", () => {
    expect(shellErrorRegionClassName).toContain("px-5");
    expect(shellErrorRegionClassName).not.toContain("m-");
  });
});

describe("desktop client navigation", () => {
  it("switches workspace state instead of starting document navigation", async () => {
    workspaceNavigationMocks.navigate.mockClear();
    const user = userEvent.setup();
    render(
      <AppShell>
        <div>当前页面</div>
      </AppShell>,
    );

    await user.click(screen.getByRole("button", { name: "实时抓包" }));

    expect(workspaceNavigationMocks.navigate).toHaveBeenCalledWith("/capture");
    expect(
      screen.queryByRole("link", { name: "实时抓包" }),
    ).not.toBeInTheDocument();
  });

  it("uses the same client router for the toolbar settings action", async () => {
    workspaceNavigationMocks.navigate.mockClear();
    const user = userEvent.setup();
    render(
      <AppShell>
        <div>当前页面</div>
      </AppShell>,
    );

    await user.click(screen.getByRole("button", { name: "打开系统设置" }));

    expect(workspaceNavigationMocks.navigate).toHaveBeenCalledWith("/settings");
  });

  it("opens contextual help without navigating away from the current page", async () => {
    workspaceNavigationMocks.navigate.mockClear();
    const user = userEvent.setup();
    const documentUrl = window.location.href;
    render(
      <AppShell>
        <div>当前页面</div>
      </AppShell>,
    );

    await user.click(
      screen.getByRole("button", { name: "打开运行监控使用说明" }),
    );

    expect(
      screen.getByRole("dialog", { name: "运行监控使用说明" }),
    ).toBeVisible();
    expect(workspaceNavigationMocks.navigate).not.toHaveBeenCalled();
    expect(window.location.href).toBe(documentUrl);
  });
});
