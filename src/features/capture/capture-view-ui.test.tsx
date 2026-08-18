// @vitest-environment jsdom

/** 验证抓包暂停/恢复/清空/选择失效等 UI 意图会调用正确 Rust Command。 */

import "@testing-library/jest-dom/vitest";
import { useState } from "react";
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

vi.mock("./socket-capture-view", () => ({
  SocketCaptureView: () => {
    const [filter, setFilter] = useState("");
    return (
      <section aria-label="Socket 抓包工作区">
        <h2>Socket 抓包记录</h2>
        <label>
          Socket 测试筛选
          <input value={filter} onChange={(event) => setFilter(event.target.value)} />
        </label>
        <button type="button">刷新 Socket 抓包</button>
      </section>
    );
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

  it("shows the fixed capture title and compact HTTP and Socket tabs", () => {
    render(<CaptureView />);

    expect(screen.getByRole("heading", { level: 1, name: "实时抓包" })).toBeVisible();
    const tablist = screen.getByRole("tablist", { name: "抓包协议" });
    expect(tablist).toBeVisible();
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual(["HTTP", "Socket"]);
    expect(tablist.className).not.toMatch(/(?:^|\s)(?:w-full|flex-1)(?:\s|$)/);
  });

  it("links each protocol tab to exactly one conditionally mounted tabpanel", () => {
    render(<CaptureView />);

    const httpTab = screen.getByRole("tab", { name: "HTTP" });
    const panel = screen.getByRole("tabpanel");
    expect(httpTab).toHaveAttribute("aria-controls", panel.id);
    expect(panel).toHaveAttribute("aria-labelledby", httpTab.id);
    expect(screen.getByRole("grid", { name: "实时抓包事件" })).toBeVisible();
    expect(screen.queryByRole("region", { name: "Socket 抓包工作区" })).toBeNull();
  });

  it.each([
    ["{ArrowRight}", "Socket"],
    ["{End}", "Socket"],
  ])("switches from HTTP with %s", async (key, expected) => {
    const user = userEvent.setup();
    render(<CaptureView />);

    const httpTab = screen.getByRole("tab", { name: "HTTP" });
    httpTab.focus();
    await user.keyboard(key);

    expect(screen.getByRole("tab", { name: expected })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("region", { name: "Socket 抓包工作区" })).toBeVisible();
    expect(screen.queryByRole("grid", { name: "实时抓包事件" })).toBeNull();
    expect(screen.queryByRole("button", { name: "清空当前显示" })).toBeNull();
  });

  it.each([
    ["{ArrowLeft}"],
    ["{Home}"],
  ])("switches from Socket with %s", async (key) => {
    const user = userEvent.setup();
    render(<CaptureView />);

    await user.click(screen.getByRole("tab", { name: "Socket" }));
    await user.keyboard(key);

    expect(screen.getByRole("tab", { name: "HTTP" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("grid", { name: "实时抓包事件" })).toBeVisible();
    expect(screen.queryByRole("region", { name: "Socket 抓包工作区" })).toBeNull();
  });

  it.each(["{Enter}", " "])("activates the focused Socket tab with %s", async (key) => {
    const user = userEvent.setup();
    render(<CaptureView />);

    const socketTab = screen.getByRole("tab", { name: "Socket" });
    socketTab.focus();
    await user.keyboard(key);

    expect(socketTab).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("region", { name: "Socket 抓包工作区" })).toBeVisible();
  });

  it("unmounts protocol-only controls and resets local state when returning", async () => {
    const user = userEvent.setup();
    render(<CaptureView />);

    await user.type(screen.getByRole("searchbox", { name: "关键字或请求 ID" }), "http-only");
    await user.click(screen.getByRole("tab", { name: "Socket" }));
    expect(screen.queryByRole("searchbox", { name: "关键字或请求 ID" })).toBeNull();
    expect(screen.queryByText(/HTTP 状态码|Cookie|JSONPath/)).toBeNull();
    const socketFilter = screen.getByRole("textbox", { name: "Socket 测试筛选" });
    await user.type(socketFilter, "socket-only");

    await user.click(screen.getByRole("tab", { name: "HTTP" }));
    expect(screen.queryByRole("textbox", { name: "Socket 测试筛选" })).toBeNull();
    expect(screen.getByRole("searchbox", { name: "关键字或请求 ID" })).toHaveValue("");

    await user.click(screen.getByRole("tab", { name: "Socket" }));
    expect(screen.getByRole("textbox", { name: "Socket 测试筛选" })).toHaveValue("");
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
      screen.getByRole("button", { name: "关闭详情并释放报文" }),
    );
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
    expect(screen.getByRole("dialog", { name: "抓包详情" })).toBeVisible();
    expect(screen.getByLabelText("实时抓包工作区")).toHaveAttribute(
      "data-layout",
      "list-only",
    );

    captureState.page = { ...page(), rows: [], total: 0 };
    rerender(<CaptureView />);

    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "关闭详情并释放报文" }),
      ).not.toBeInTheDocument(),
    );
  });

  it("通过详情工具栏关闭并恢复列表布局", async () => {
    const user = userEvent.setup();
    render(<CaptureView />);

    const row = screen
      .getByRole("grid", { name: "实时抓包事件" })
      .querySelector<HTMLElement>('[data-key="42"]');
    await user.click(row!);

    expect(screen.getByText("抓包详情")).toBeVisible();
    expect(screen.getByText("POST /payment · 192.168.1.20")).toBeVisible();
    expect(screen.getByRole("dialog", { name: "抓包详情" })).toBeVisible();
    const closeButton = screen.getByRole("button", {
      name: "关闭详情并释放报文",
    });
    expect(closeButton).toHaveAttribute("data-slot", "modal-close-trigger");
    expect(closeButton).toHaveTextContent("");
    await user.click(closeButton);

    expect(screen.getByLabelText("实时抓包工作区")).toHaveAttribute(
      "data-layout",
      "list-only",
    );
    expect(
      screen.queryByRole("dialog", { name: "抓包详情" }),
    ).not.toBeInTheDocument();
    expect(queryMocks.detailInvalidate).toHaveBeenCalled();
  });
});
