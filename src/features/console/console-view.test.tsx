// @vitest-environment jsdom

/** 验证运行监控只展示 Rust 汇总的 Workspace 入口状态。 */

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ListenerOverviewViewModel } from "@/generated/rust-types";
import { ConsoleView } from "./console-view";

const navigate = vi.fn();
vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceNavigation: () => ({ navigate }),
}));

const overview: ListenerOverviewViewModel = {
  workspace_id: "workspace-1",
  workspace_name: "联调环境",
  state_text: "部分入口运行中",
  ui_tone: "positive",
  total_count: 2,
  active_count: 1,
  faulted_count: 0,
  rows: [
    {
      listener_id: "transaction",
      name: "交易入口",
      kind_text: "固定上游",
      listen_address: "0.0.0.0:16627",
      request_destination: "https://transaction.example.test:16627",
      state: "running",
      state_text: "运行中",
      ui_tone: "positive",
      fault_reason: null,
    },
    {
      listener_id: "dll",
      name: "DLL 入口",
      kind_text: "固定上游",
      listen_address: "0.0.0.0:16127",
      request_destination: "https://dll.example.test:16127",
      state: "stopped",
      state_text: "已停止",
      ui_tone: "neutral",
      fault_reason: null,
    },
  ],
};

describe("ConsoleView", () => {
  it("renders the Rust listener overview without recomputing business state", () => {
    render(
      <ConsoleView
        overview={overview}
        recentCaptureLoading={false}
        onRecentCaptureRetry={vi.fn()}
      />,
    );

    expect(screen.getByRole("heading", { name: "运行监控" })).toBeVisible();
    expect(screen.getByText("交易入口")).toBeVisible();
    expect(screen.getByText("0.0.0.0:16627")).toBeVisible();
    expect(screen.getByText("https://dll.example.test:16127")).toBeVisible();
    expect(screen.getByText(/共 2 个入口，当前活动 1 个/)).toBeVisible();
  });

  it("opens the single listener configuration page", async () => {
    const user = userEvent.setup();
    render(
      <ConsoleView
        overview={overview}
        recentCaptureLoading={false}
        onRecentCaptureRetry={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: "管理代理入口" }));
    expect(navigate).toHaveBeenCalledWith("/listeners");
  });
});
