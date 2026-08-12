// @vitest-environment jsdom

/** 验证抓包暂停/恢复/清空/选择失效等 UI 意图会调用正确 Rust Command。 */

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CapturePageViewModel } from "@/generated/rust-types";
import { CaptureView } from "./capture-view";

const commandMocks = vi.hoisted(() => ({
  captureClearView: vi.fn(),
}));
const queryMocks = vi.hoisted(() => ({
  pageRefresh: vi.fn(),
  detailRefresh: vi.fn(),
  detailInvalidate: vi.fn(),
}));
const captureState = vi.hoisted(() => ({
  page: undefined as CapturePageViewModel | undefined,
}));

vi.mock("@/generated/rust-types", () => ({
  commands: commandMocks,
}));

vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: () => "Rust 操作失败",
}));

vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: vi.fn(),
  useBootstrap: () => ({
    bootstrap: { channel_catalog: [] },
    proxy: { state_text: "运行中", ui_tone: "positive" },
  }),
}));

vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceNavigation: () => ({ navigate: vi.fn() }),
}));

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) =>
    key.startsWith("capture-query:")
      ? {
          data: captureState.page,
          error: undefined,
          isLoading: false,
          refresh: queryMocks.pageRefresh,
        }
      : {
          data: undefined,
          error: undefined,
          isLoading: false,
          refresh: queryMocks.detailRefresh,
          invalidate: queryMocks.detailInvalidate,
        },
}));

const page = (): CapturePageViewModel => ({
  rows: [
    {
      event_id: 42,
      runtime_epoch: "epoch-1",
      session_id: "session-1",
      occurred_at: "2026-07-31T10:00:00Z",
      terminal_ip: "192.168.1.20",
      channel: "transaction",
      channel_text: "交易",
      stage: "request",
      stage_text: "请求",
      method: "POST",
      target: "/payment",
      http_status: null,
      result: "处理中",
      ui_tone: "info",
      duration_ms: null,
      matched_rule_ids: [],
      size_bytes: 128,
      breakpoint_id: null,
      can_go_to_breakpoint: false,
      breakpoint_disabled_reason: null,
    },
  ],
  total: 1,
  page: 1,
  page_size: 50,
  total_pages: 1,
  event_cursor: 99,
  oldest_event_id: 42,
  runtime_epoch: "epoch-1",
  snapshot_required: false,
  empty_message: "暂无抓包",
});

describe("CaptureView live controls", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    captureState.page = page();
    queryMocks.pageRefresh.mockResolvedValue(undefined);
    commandMocks.captureClearView.mockResolvedValue(99);
  });

  it("pauses and resumes the display, then clears the current Rust cursor", async () => {
    const user = userEvent.setup();
    render(<CaptureView />);

    await user.click(
      screen.getByRole("button", { name: "暂停列表滚动" }),
    );
    expect(
      screen.getByRole("button", { name: "恢复列表滚动" }),
    ).toBeVisible();

    await user.click(
      screen.getByRole("button", { name: "恢复列表滚动" }),
    );
    expect(
      screen.getByRole("button", { name: "暂停列表滚动" }),
    ).toBeVisible();

    const row = screen
      .getByRole("grid", { name: "实时抓包事件" })
      .querySelector<HTMLElement>('[data-key="42"]');
    expect(row).toBeTruthy();
    await user.click(row!);
    await user.click(
      screen.getByRole("button", { name: "清空当前显示" }),
    );

    await waitFor(() =>
      expect(commandMocks.captureClearView).toHaveBeenCalledWith(99),
    );
    expect(queryMocks.detailInvalidate).toHaveBeenCalled();
    expect(queryMocks.pageRefresh).toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: "关闭详情并释放报文" }),
    ).not.toBeInTheDocument();
  });

  it("invalidates a selection when a refreshed page no longer contains it", async () => {
    const user = userEvent.setup();
    const { rerender } = render(<CaptureView />);

    const row = screen
      .getByRole("grid", { name: "实时抓包事件" })
      .querySelector<HTMLElement>('[data-key="42"]');
    expect(row).toBeTruthy();
    await user.click(row!);
    expect(
      screen.getByRole("button", { name: "关闭详情并释放报文" }),
    ).toBeVisible();
    expect(screen.getByLabelText("实时抓包工作区")).toHaveAttribute(
      "data-layout",
      "stacked",
    );

    captureState.page = { ...page(), rows: [], total: 0 };
    rerender(<CaptureView />);

    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "关闭详情并释放报文" }),
      ).not.toBeInTheDocument(),
    );
  });
});
