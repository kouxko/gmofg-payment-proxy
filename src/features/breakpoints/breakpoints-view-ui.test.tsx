// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  BreakpointDetailViewModel,
  BreakpointSummaryViewModel,
} from "@/generated/rust-types";
import { BreakpointsView } from "./breakpoints-view";

const commandMocks = vi.hoisted(() => ({
  breakpointQuery: vi.fn(),
  breakpointFormatJson: vi.fn(),
  breakpointRestoreOriginal: vi.fn(),
  breakpointValidate: vi.fn(),
  breakpointResolve: vi.fn(),
}));
const queryMocks = vi.hoisted(() => ({
  refresh: vi.fn(),
  invalidate: vi.fn(),
}));
const navigationMocks = vi.hoisted(() => ({
  searchParams: new URLSearchParams(),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: commandMocks,
}));

vi.mock("@/lib/ipc/client", () => ({
  appErrorViewModel: () => undefined,
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: () => "Rust 操作失败",
}));

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) =>
    key === "breakpoint-query"
      ? {
          data: summaries,
          error: undefined,
          isLoading: false,
          refresh: queryMocks.refresh,
        }
      : {
          data: details[key.replace("breakpoint-detail:", "")],
          error: undefined,
          isLoading: false,
          refresh: vi.fn(),
          invalidate: queryMocks.invalidate,
        },
}));

vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: vi.fn(),
}));

vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceNavigation: () => ({
    searchParams: navigationMocks.searchParams,
  }),
}));

const message = (bodyText: string) => ({
  http_status: null,
  headers: { "content-type": ["application/json"] },
  body_text: bodyText,
  body_bytes: [123, 125],
  json: {},
  content_length: bodyText.length,
});

const summary = (
  breakpointId: string,
  title: string,
): BreakpointSummaryViewModel => ({
  breakpoint_id: breakpointId,
  session_id: `session-${breakpointId}`,
  runtime_epoch: `epoch-${breakpointId}`,
  stage: "request",
  title,
  terminal_ip: "192.168.1.20",
  channel: "transaction",
  channel_text: "交易",
  method: "POST",
  target: `/api/${breakpointId}`,
  waiting_since: "2026-07-31T10:00:00Z",
  certificate_fingerprint_suffix: "ABCD",
  state: "pending",
  state_text: "等待处理",
  ui_tone: "warning",
  revision: 3,
});
const summaries = [summary("A", "断点 A"), summary("B", "断点 B")];
const detail = (
  breakpointSummary: BreakpointSummaryViewModel,
): BreakpointDetailViewModel => ({
  summary: breakpointSummary,
  original: message('{"original":true}'),
  effective: message('{"edited":false}'),
  can_resolve: true,
  resolve_disabled_reason: null,
  available_actions: [
    {
      kind: "forward_modified",
      label: "转发修改报文",
      enabled: true,
      disabled_reason: null,
      default_delay_ms: null,
      default_http_status: null,
      default_content_length_delta: null,
      default_truncate_at: null,
    },
  ],
});
const details: Record<string, BreakpointDetailViewModel> = {
  A: detail(summaries[0]),
  B: detail(summaries[1]),
};

describe("BreakpointsView queue controls", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    navigationMocks.searchParams = new URLSearchParams();
    queryMocks.refresh.mockResolvedValue(undefined);
    commandMocks.breakpointValidate.mockResolvedValue({
      valid: true,
      field_errors: {},
      warnings: [],
    });
    commandMocks.breakpointResolve.mockResolvedValue({
      message: "已处理",
      state_text: "已转发",
      ui_tone: "positive",
    });
  });

  it("explains and executes the icon-only refresh button", async () => {
    const user = userEvent.setup();
    render(<BreakpointsView />);

    const refresh = screen.getByRole("button", {
      name: "刷新断点队列",
    });
    await user.tab();

    expect(await screen.findByText("刷新断点队列")).toBeVisible();
    await user.click(refresh);
    expect(queryMocks.refresh).toHaveBeenCalledTimes(1);
  });

  it("follows breakpointId changes while staying on the same route", async () => {
    navigationMocks.searchParams = new URLSearchParams("breakpointId=A");
    const { rerender } = render(<BreakpointsView />);

    expect(screen.getByRole("heading", { name: "断点 A" })).toBeVisible();

    navigationMocks.searchParams = new URLSearchParams("breakpointId=B");
    rerender(<BreakpointsView />);

    expect(
      await screen.findByRole("heading", { name: "断点 B" }),
    ).toBeVisible();
    expect(
      screen.queryByRole("heading", { name: "断点 A" }),
    ).not.toBeInTheDocument();
  });

  it("validates the edited body and resolves with the validated draft", async () => {
    navigationMocks.searchParams = new URLSearchParams("breakpointId=A");
    const user = userEvent.setup();
    render(<BreakpointsView />);

    fireEvent.change(screen.getByRole("textbox", { name: "有效 JSON" }), {
      target: { value: '{"edited":true}' },
    });
    await user.click(screen.getByRole("button", { name: "由 Rust 校验" }));

    await waitFor(() =>
      expect(commandMocks.breakpointValidate).toHaveBeenCalledWith(
        expect.objectContaining({
          breakpoint_id: "A",
          expected_revision: 3,
          message: expect.objectContaining({ body_text: '{"edited":true}' }),
        }),
        "epoch-A",
      ),
    );
    expect(
      await screen.findByText("报文校验通过"),
    ).toBeVisible();

    await user.click(
      screen.getByRole("button", { name: "执行所选处理" }),
    );

    await waitFor(() =>
      expect(commandMocks.breakpointResolve).toHaveBeenCalledWith(
        "epoch-A",
        expect.objectContaining({
          breakpoint_id: "A",
          expected_revision: 3,
          kind: "forward_modified",
          message: expect.objectContaining({ body_text: '{"edited":true}' }),
        }),
      ),
    );
    expect(queryMocks.invalidate).toHaveBeenCalled();
    expect(queryMocks.refresh).toHaveBeenCalled();
  });
});
