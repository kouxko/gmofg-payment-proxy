// @vitest-environment jsdom

/** Socket 列表的空态、类型方向、分页和选择测试。 */

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type {
  SocketCapturePageViewModel,
  SocketCaptureRowViewModel,
} from "@/generated/rust-types";
import { SocketCaptureList } from "./socket-capture-list";

const relayRow = {
  capture_id: "11111111-1111-4111-8111-111111111111",
  runtime_epoch: "22222222-2222-4222-8222-222222222222",
  session_id: "44444444-4444-4444-8444-444444444444",
  connection_id: "44444444-4444-4444-8444-444444444444",
  listener_id: "55555555-5555-4555-8555-555555555555",
  occurred_at: "2026-08-15T10:00:00Z",
  completed_at: "2026-08-15T10:00:01Z",
  kind: "relay_frame",
  direction: "downstream",
  package: { id: "iso8583", version: "1.0.0" },
  schema: { id: "message", version: 1 },
  origin_size_bytes: 1,
  written_size_bytes: 2,
  logical_size_bytes: 300,
  matched_rule_ids: [],
} satisfies SocketCaptureRowViewModel;

const localRow = {
  ...relayRow,
  capture_id: "66666666-6666-4666-8666-666666666666",
  kind: "local_exchange",
  direction: null,
  matched_rule_ids: ["77777777-7777-4777-8777-777777777777"],
} satisfies SocketCaptureRowViewModel;

function page(
  rows: SocketCaptureRowViewModel[] = [relayRow, localRow],
): SocketCapturePageViewModel {
  if (rows.length === 0) {
    return {
      rows,
      total: 0,
      page: 1,
      page_size: 2,
      total_pages: 0,
      empty_message: "暂无 Socket 抓包",
    };
  }
  if (rows.length === 1) {
    return {
      rows,
      total: 1,
      page: 1,
      page_size: 2,
      total_pages: 1,
      empty_message: "暂无 Socket 抓包",
    };
  }
  return {
    rows,
    total: 6,
    page: 2,
    page_size: 2,
    total_pages: 3,
    empty_message: "暂无 Socket 抓包",
  };
}

function renderList(overrides: Partial<React.ComponentProps<typeof SocketCaptureList>> = {}) {
  const props = {
    page: page(),
    error: undefined,
    loading: false,
    selectedId: undefined,
    onSelect: vi.fn(),
    onPage: vi.fn(),
    onRetry: vi.fn(),
    onClear: vi.fn(),
    clearButtonId: "clear-socket-captures",
    ...overrides,
  } satisfies React.ComponentProps<typeof SocketCaptureList>;
  render(<SocketCaptureList {...props} />);
  return props;
}

describe("SocketCaptureList", () => {
  it("labels Relay direction and Local association without a fabricated server", () => {
    renderList();

    expect(screen.getByText("Server → App")).toBeVisible();
    expect(screen.getByText("应用请求 ⇄ 本机应答")).toBeVisible();
    expect(screen.getByText("转发报文")).toBeVisible();
    expect(screen.getByText("本机应答")).toBeVisible();
  });

  it("labels an upstream Relay direction as App to Server", () => {
    renderList({ page: page([{ ...relayRow, direction: "upstream" }]) });

    expect(screen.getByText("App → Server")).toBeVisible();
  });

  it("selects the exact row represented by a table key", async () => {
    const user = userEvent.setup();
    const props = renderList();
    const row = screen
      .getByRole("grid", { name: "Socket 抓包记录" })
      .querySelector<HTMLElement>(`[data-key="${localRow.capture_id}"]`);

    await user.click(row!);
    expect(props.onSelect).toHaveBeenCalledWith(localRow);
  });

  it("shows the backend empty message when no capture exists", () => {
    renderList({ page: page([]) });

    expect(screen.getByText("暂无 Socket 抓包")).toBeVisible();
    expect(screen.getByText("1 / 1")).toBeVisible();
    expect(screen.getByRole("button", { name: "上一页" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "下一页" })).toBeDisabled();
  });

  it("shows list loading and refresh progress without a fake row", () => {
    renderList({ page: undefined, loading: true });

    expect(screen.getByText("正在查询 Socket 抓包…")).toBeVisible();
    expect(screen.getByLabelText("正在刷新 Socket 抓包")).toBeVisible();
  });

  it("shows the local default empty message before a backend page exists", () => {
    renderList({ page: undefined, loading: false });

    expect(screen.getByText("当前工作区还没有 Socket 抓包")).toBeVisible();
    expect(screen.queryByLabelText("正在刷新 Socket 抓包")).toBeNull();
  });

  it("moves to both adjacent backend pages", async () => {
    const user = userEvent.setup();
    const props = renderList();

    await user.click(screen.getByRole("button", { name: "上一页" }));
    await user.click(screen.getByRole("button", { name: "下一页" }));
    expect(props.onPage).toHaveBeenNthCalledWith(1, 1);
    expect(props.onPage).toHaveBeenNthCalledWith(2, 3);
  });

  it("forwards the clear action from the stable focus target", async () => {
    const user = userEvent.setup();
    const props = renderList();

    const button = screen.getByRole("button", { name: "清空 Socket 抓包" });
    expect(button).toHaveAttribute("id", "clear-socket-captures");
    await user.click(button);
    expect(props.onClear).toHaveBeenCalledTimes(1);
  });
});
