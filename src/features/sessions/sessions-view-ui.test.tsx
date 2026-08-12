// @vitest-environment jsdom

/** 验证会话详情按需读取、导出确认、清空确认和 Payload 释放。 */

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionPageViewModel } from "@/generated/rust-types";
import { SessionsView } from "./sessions-view";

const commandMocks = vi.hoisted(() => ({
  sessionExport: vi.fn(),
  sessionClear: vi.fn(),
}));
const queryMocks = vi.hoisted(() => ({
  pageRefresh: vi.fn(),
  detailRefresh: vi.fn(),
  detailInvalidate: vi.fn(),
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

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) =>
    key.startsWith("session-query:")
      ? {
          data: page,
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

const page: SessionPageViewModel = {
  items: [
    {
      session_id: "session-1",
      request_id: "request-1",
      started_at: "2026-07-31T10:00:00Z",
      completed_at: "2026-07-31T10:00:01Z",
      terminal_ip: "192.168.1.20",
      channel: "transaction",
      channel_text: "交易",
      method: "POST",
      target: "/payment",
      http_status: 200,
      result: "成功",
      ui_tone: "positive",
      duration_ms: 1000,
      matched_rule_ids: [],
      request_size_bytes: 128,
      response_size_bytes: 256,
      pending_breakpoint: false,
      revision: 1,
    },
  ],
  total: 1,
  page: 1,
  page_size: 10,
  total_pages: 1,
  empty_message: "暂无会话",
};

describe("SessionsView destructive actions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    queryMocks.pageRefresh.mockResolvedValue(undefined);
    commandMocks.sessionExport.mockResolvedValue({
      message: "导出完成",
      ui_tone: "positive",
    });
    commandMocks.sessionClear.mockResolvedValue({
      message: "清空完成",
      ui_tone: "positive",
    });
  });

  it("opens the selected session in a modal and releases its payload on close", async () => {
    const user = userEvent.setup();
    render(<SessionsView />);

    const row = screen
      .getByRole("grid", { name: "会话记录" })
      .querySelector<HTMLElement>('[data-key="session-1"]');
    expect(row).toBeTruthy();
    await user.click(row!);

    expect(
      screen.getByRole("dialog", { name: "完整会话报文" }),
    ).toBeVisible();
    expect(screen.getByText("POST /payment · 192.168.1.20")).toBeVisible();
    const closeButton = screen.getByRole("button", {
      name: "关闭会话详情并释放报文",
    });
    expect(closeButton).toHaveAttribute("data-slot", "modal-close-trigger");

    queryMocks.detailInvalidate.mockClear();
    await user.click(closeButton);

    expect(
      screen.queryByRole("dialog", { name: "完整会话报文" }),
    ).not.toBeInTheDocument();
    expect(queryMocks.detailInvalidate).toHaveBeenCalledTimes(1);
    expect(
      screen.getByRole("button", { name: "导出所选会话" }),
    ).toBeEnabled();
  });

  it("exports the selected session only after the sensitive-data confirmation", async () => {
    const user = userEvent.setup();
    render(<SessionsView />);

    const row = screen
      .getByRole("grid", { name: "会话记录" })
      .querySelector<HTMLElement>('[data-key="session-1"]');
    expect(row).toBeTruthy();
    await user.click(row!);
    await user.click(
      screen.getByRole("button", {
        name: "关闭会话详情并释放报文",
      }),
    );
    await user.click(screen.getByRole("button", { name: "导出所选会话" }));
    expect(
      await screen.findByRole("heading", { name: "确认导出原始报文" }),
    ).toBeVisible();
    expect(commandMocks.sessionExport).not.toHaveBeenCalled();

    await user.click(
      screen.getByRole("button", { name: "确认并选择位置" }),
    );

    await waitFor(() =>
      expect(commandMocks.sessionExport).toHaveBeenCalledWith(
        "session-1",
        true,
      ),
    );
  });

  it("clears through confirmation and releases the selected detail", async () => {
    const user = userEvent.setup();
    render(<SessionsView />);

    const row = screen
      .getByRole("grid", { name: "会话记录" })
      .querySelector<HTMLElement>('[data-key="session-1"]');
    expect(row).toBeTruthy();
    await user.click(row!);
    await user.click(
      screen.getByRole("button", {
        name: "关闭会话详情并释放报文",
      }),
    );
    await user.click(screen.getByRole("button", { name: "清空全部会话" }));
    expect(
      await screen.findByRole("heading", { name: "清空已完成会话？" }),
    ).toBeVisible();
    expect(commandMocks.sessionClear).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "确认清空" }));

    await waitFor(() =>
      expect(commandMocks.sessionClear).toHaveBeenCalledWith(true),
    );
    expect(queryMocks.detailInvalidate).toHaveBeenCalled();
    expect(queryMocks.pageRefresh).toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "导出所选会话" }),
    ).toBeDisabled();
  });
});
