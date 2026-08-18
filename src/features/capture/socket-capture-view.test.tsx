// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  SocketCaptureDetailViewModel,
  SocketCaptureDocument,
  SocketCapturePageViewModel,
  WorkspaceSummaryViewModel,
} from "@/generated/rust-types";
import { SocketCaptureView } from "./socket-capture-view";
const commandMocks = vi.hoisted(() => ({
  workspaceList: vi.fn(),
  socketCaptureQuery: vi.fn(),
  socketCaptureGetDetail: vi.fn(),
  socketCaptureClear: vi.fn(),
}));
const refreshMocks = vi.hoisted(() => ({
  workspaces: vi.fn(),
  page: vi.fn(),
  detail: vi.fn(),
  invalidateDetail: vi.fn(),
}));
const eventRefreshMock = vi.hoisted(() => vi.fn());
const queryHookMock = vi.hoisted(() => vi.fn());
const state = vi.hoisted(() => ({
  workspaceData: undefined as unknown,
  workspaceError: undefined as string | undefined,
  workspaceLoading: false,
  pageData: undefined as SocketCapturePageViewModel | undefined,
  pageError: undefined as string | undefined,
  pageLoading: false,
  detailData: undefined as SocketCaptureDetailViewModel | undefined,
  detailError: undefined as string | undefined,
  detailLoading: false,
}));
vi.mock("@/generated/rust-types", () => ({ commands: commandMocks }));
vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: () => "Rust 操作失败",
}));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: eventRefreshMock,
}));
vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: queryHookMock,
}));
const workspace = {
  id: "11111111-1111-4111-8111-111111111111",
  name: "Socket workspace",
  revision: 1,
  listener_count: 1,
  enabled_listener_count: 1,
  selected: true,
} satisfies WorkspaceSummaryViewModel;
const pageFixture = {
  rows: [
    {
      capture_id: "22222222-2222-4222-8222-222222222222",
      runtime_epoch: "33333333-3333-4333-8333-333333333333",
      session_id: "55555555-5555-4555-8555-555555555555",
      connection_id: "55555555-5555-4555-8555-555555555555",
      listener_id: "66666666-6666-4666-8666-666666666666",
      occurred_at: "2026-08-15T10:00:00Z",
      completed_at: "2026-08-15T10:00:01Z",
      kind: "relay_frame",
      direction: "upstream",
      package: { id: "iso8583", version: "1.0.0" },
      schema: { id: "message", version: 1 },
      origin_size_bytes: 1,
      written_size_bytes: 1,
      logical_size_bytes: 256,
      matched_rule_ids: [],
    },
  ],
  total: 1,
  page: 1,
  page_size: 50,
  total_pages: 1,
  empty_message: "暂无 Socket 抓包",
} satisfies SocketCapturePageViewModel;
const documentFixture = {
  schema: {
    id: "message",
    version: 1,
    title: "Message",
    fields: [{ name: "value", type: "string", label: "Value" }],
  },
  values: [null],
} satisfies SocketCaptureDocument;
const detailFixture = {
  record: {
    capture_id: pageFixture.rows[0].capture_id,
    runtime_epoch: pageFixture.rows[0].runtime_epoch,
    workspace_id: workspace.id,
    listener_id: pageFixture.rows[0].listener_id,
    session_id: pageFixture.rows[0].session_id,
    connection_id: pageFixture.rows[0].connection_id,
    peer_address: "127.0.0.1:45000",
    occurred_at: pageFixture.rows[0].occurred_at,
    completed_at: pageFixture.rows[0].completed_at,
    payload: {
      kind: "relay_frame",
      capture: {
        direction: "upstream",
        package: pageFixture.rows[0].package,
        schema: pageFixture.rows[0].schema,
        origin: [0x30],
        stages: [
          { stage: "app_to_proxy", matched_rule_ids: [], document: documentFixture },
          { stage: "proxy_to_upstream", matched_rule_ids: [], document: documentFixture },
        ],
        written: [0x30],
        display: {
          type: "hex_fallback",
          reason: "entry_point_failed",
          diagnostic: { code: "DISPLAY_FAILED", message: "协议展示失败" },
        },
      },
    },
  },
} satisfies SocketCaptureDetailViewModel;

function installQueryMock() {
  queryHookMock.mockImplementation((key: string, loader: () => unknown) => {
    if (key === "socket-capture-workspaces") {
      return {
        data: state.workspaceData,
        error: state.workspaceError,
        isLoading: state.workspaceLoading,
        refresh: refreshMocks.workspaces,
      };
    }
    if (key.startsWith("socket-capture-query:")) {
      return {
        data: state.pageData,
        error: state.pageError,
        isLoading: state.pageLoading,
        refresh: refreshMocks.page,
      };
    }
    if (!key.endsWith(":none")) void loader();
    return {
      data: state.detailData,
      error: state.detailError,
      isLoading: state.detailLoading,
      refresh: refreshMocks.detail,
      invalidate: refreshMocks.invalidateDetail,
    };
  });
}

function selectWorkspace(id: string, name: string) {
  state.workspaceData = [{ ...workspace, id, name }];
}

describe("SocketCaptureView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    state.workspaceData = [workspace];
    state.workspaceError = undefined;
    state.workspaceLoading = false;
    state.pageData = pageFixture;
    state.pageError = undefined;
    state.pageLoading = false;
    state.detailData = detailFixture;
    state.detailError = undefined;
    state.detailLoading = false;
    commandMocks.socketCaptureClear.mockResolvedValue({
      success: true,
      cancelled: false,
      message: "cleared",
      ui_tone: "positive",
      entity_id: null,
      revision: null,
      requires_restart: false,
    });
    refreshMocks.page.mockResolvedValue(undefined);
    installQueryMock();
  });

  it("shows a labelled workspace loading state before issuing a Socket page", () => {
    state.workspaceData = undefined;
    state.workspaceLoading = true;
    render(<SocketCaptureView />);

    expect(screen.getByLabelText("正在读取当前工作区")).toBeVisible();
    expect(screen.queryByRole("grid", { name: "Socket 抓包记录" })).toBeNull();
  });

  it("loads workspace ownership through the Rust workspace command", async () => {
    commandMocks.workspaceList.mockResolvedValue([workspace]);
    render(<SocketCaptureView />);
    const call = queryHookMock.mock.calls.find(
      ([key]) => key === "socket-capture-workspaces",
    );

    await expect((call![1] as () => Promise<unknown>)()).resolves.toEqual([
      workspace,
    ]);
    expect(commandMocks.workspaceList).toHaveBeenCalledTimes(1);
  });

  it("fails closed when there is not exactly one selected workspace", async () => {
    const user = userEvent.setup();
    state.workspaceData = [{ ...workspace, selected: false }];
    render(<SocketCaptureView />);

    expect(screen.getByText("无法确定唯一的当前工作区")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "重试" }));
    expect(refreshMocks.workspaces).toHaveBeenCalledTimes(1);
    expect(commandMocks.socketCaptureQuery).not.toHaveBeenCalled();
  });

  it("shows list error and retries the Socket page query", async () => {
    const user = userEvent.setup();
    state.pageError = "database unavailable";
    render(<SocketCaptureView />);

    expect(screen.getByText("Socket 抓包读取失败")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "重试" }));
    expect(refreshMocks.page).toHaveBeenCalledTimes(1);
  });

  it("validates an exact page returned by the real Socket query loader", async () => {
    commandMocks.socketCaptureQuery.mockResolvedValue(pageFixture);
    render(<SocketCaptureView />);
    const call = queryHookMock.mock.calls.find(
      ([key]) => typeof key === "string" && key.startsWith("socket-capture-query:"),
    );
    expect(call).toBeDefined();

    await expect((call![1] as () => Promise<unknown>)()).resolves.toEqual(
      pageFixture,
    );
    expect(commandMocks.socketCaptureQuery).toHaveBeenCalledWith(
      expect.objectContaining({ workspace_id: workspace.id }),
    );
  });

  it("rejects a malformed page returned by the Socket query loader", async () => {
    commandMocks.socketCaptureQuery.mockResolvedValue({ rows: "invalid" });
    render(<SocketCaptureView />);
    const call = queryHookMock.mock.calls.find(
      ([key]) => typeof key === "string" && key.startsWith("socket-capture-query:"),
    );

    await expect((call![1] as () => Promise<unknown>)()).rejects.toThrow(
      "Socket 抓包列表返回了不一致或畸形的数据",
    );
  });

  it("moves to the next backend page and keeps pagination bounded", async () => {
    const user = userEvent.setup();
    state.pageData = {
      ...pageFixture,
      rows: Array.from({ length: 50 }, (_, index) => ({
        ...pageFixture.rows[0],
        capture_id: `capture-${index}`,
      })),
      total: 51,
      total_pages: 2,
    };
    render(<SocketCaptureView />);

    expect(screen.getByRole("button", { name: "上一页" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "下一页" }));

    await waitFor(() =>
      expect(
        queryHookMock.mock.calls.some(
          ([key]) => typeof key === "string" && key.includes('"page":2'),
        ),
      ).toBe(true),
    );
  });

  it("opens a selected row through the Socket detail command identity", async () => {
    const user = userEvent.setup();
    render(<SocketCaptureView />);

    const row = screen
      .getByRole("grid", { name: "Socket 抓包记录" })
      .querySelector<HTMLElement>(`[data-key="${pageFixture.rows[0].capture_id}"]`);
    expect(row).not.toBeNull();
    await user.click(row!);

    expect(screen.getByRole("dialog", { name: "Socket 抓包详情" })).toBeVisible();
    expect(commandMocks.socketCaptureGetDetail).toHaveBeenCalledWith(
      pageFixture.rows[0].capture_id,
    );
  });

  it("subscribes Socket completion and workspace events to the correct refreshers", () => {
    render(<SocketCaptureView />);

    expect(eventRefreshMock).toHaveBeenCalledWith(
      ["workspace_changed"],
      refreshMocks.workspaces,
    );
    expect(eventRefreshMock).toHaveBeenCalledWith(
      ["socket_capture_completed", "snapshot_required", "workspace_changed"],
      refreshMocks.page,
      { paused: false },
    );
  });

  it("clears only after confirmation and refreshes the current workspace page", async () => {
    const user = userEvent.setup();
    render(<SocketCaptureView />);

    await user.click(screen.getByRole("button", { name: "清空 Socket 抓包" }));
    expect(commandMocks.socketCaptureClear).not.toHaveBeenCalled();
    expect(
      screen.getByRole("alertdialog", { name: "清空当前工作区的 Socket 抓包？" }),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "确认清空" }));

    await waitFor(() =>
      expect(commandMocks.socketCaptureClear).toHaveBeenCalledWith(workspace.id, true),
    );
    expect(refreshMocks.invalidateDetail).toHaveBeenCalledTimes(1);
    expect(refreshMocks.page).toHaveBeenCalledTimes(1);
  });

  it("closes clear confirmation without issuing a command", async () => {
    const user = userEvent.setup();
    render(<SocketCaptureView />);

    await user.click(screen.getByRole("button", { name: "清空 Socket 抓包" }));
    await user.click(screen.getByRole("button", { name: "取消" }));

    await waitFor(() =>
      expect(
        screen.queryByRole("alertdialog", { name: "清空当前工作区的 Socket 抓包？" }),
      ).toBeNull(),
    );
    expect(commandMocks.socketCaptureClear).not.toHaveBeenCalled();
  });

  it("invalidates a confirmation when the selected workspace changes", async () => {
    const user = userEvent.setup();
    const view = render(<SocketCaptureView />);

    await user.click(screen.getByRole("button", { name: "清空 Socket 抓包" }));
    selectWorkspace("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "Another workspace");
    view.rerender(<SocketCaptureView />);

    await waitFor(() =>
      expect(
        screen.queryByRole("alertdialog", { name: "清空当前工作区的 Socket 抓包？" }),
      ).toBeNull(),
    );
    expect(commandMocks.socketCaptureClear).not.toHaveBeenCalled();
  });

  it("ignores a delayed clear response after the workspace changes", async () => {
    const user = userEvent.setup();
    let resolveClear!: (value: unknown) => void;
    commandMocks.socketCaptureClear.mockReturnValue(new Promise((resolve) => {
      resolveClear = resolve;
    }));
    const view = render(<SocketCaptureView />);

    await user.click(screen.getByRole("button", { name: "清空 Socket 抓包" }));
    await user.click(screen.getByRole("button", { name: "确认清空" }));
    expect(commandMocks.socketCaptureClear).toHaveBeenCalledWith(workspace.id, true);

    selectWorkspace("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "New selected workspace");
    view.rerender(<SocketCaptureView />);
    resolveClear({
      success: true,
      cancelled: false,
      message: "cleared",
      ui_tone: "positive",
      entity_id: null,
      revision: null,
      requires_restart: false,
    });

    await waitFor(() =>
      expect(
        screen.queryByRole("alertdialog", { name: "清空当前工作区的 Socket 抓包？" }),
      ).toBeNull(),
    );
    expect(refreshMocks.page).not.toHaveBeenCalled();
    expect(refreshMocks.invalidateDetail).not.toHaveBeenCalled();
  });

  it("does not apply post-refresh clear effects after the workspace changes", async () => {
    const user = userEvent.setup();
    let resolveRefresh!: () => void;
    refreshMocks.page.mockReturnValue(new Promise<void>((resolve) => {
      resolveRefresh = resolve;
    }));
    const view = render(<SocketCaptureView />);

    await user.click(screen.getByRole("button", { name: "清空 Socket 抓包" }));
    await user.click(screen.getByRole("button", { name: "确认清空" }));
    await waitFor(() => expect(refreshMocks.page).toHaveBeenCalledTimes(1));

    selectWorkspace(
      "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
      "Workspace after refresh began",
    );
    view.rerender(<SocketCaptureView />);
    resolveRefresh();

    await waitFor(() =>
      expect(
        screen.queryByRole("alertdialog", { name: "清空当前工作区的 Socket 抓包？" }),
      ).toBeNull(),
    );
    expect(screen.getByRole("button", { name: "清空 Socket 抓包" })).not.toHaveFocus();
  });

  it("locks clear controls while the Rust command is pending", async () => {
    const user = userEvent.setup();
    let resolveClear!: (value: unknown) => void;
    commandMocks.socketCaptureClear.mockReturnValue(
      new Promise((resolve) => {
        resolveClear = resolve;
      }),
    );
    render(<SocketCaptureView />);

    await user.click(screen.getByRole("button", { name: "清空 Socket 抓包" }));
    await user.click(screen.getByRole("button", { name: "确认清空" }));
    expect(screen.getByRole("button", { name: "正在清空…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消" })).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(
      screen.getByRole("alertdialog", { name: "清空当前工作区的 Socket 抓包？" }),
    ).toBeVisible();

    resolveClear({
      success: true,
      cancelled: false,
      message: "cleared",
      ui_tone: "positive",
      entity_id: null,
      revision: null,
      requires_restart: false,
    });
    await waitFor(() => expect(refreshMocks.page).toHaveBeenCalled());
  });

  it("keeps captures when Rust returns a cancelled clear result", async () => {
    const user = userEvent.setup();
    commandMocks.socketCaptureClear.mockResolvedValue({
      success: false,
      cancelled: true,
      message: "cancelled",
      ui_tone: "warning",
      entity_id: null,
      revision: null,
      requires_restart: false,
    });
    render(<SocketCaptureView />);
    await user.click(screen.getByRole("button", { name: "清空 Socket 抓包" }));
    await user.click(screen.getByRole("button", { name: "确认清空" }));
    await waitFor(() => expect(commandMocks.socketCaptureClear).toHaveBeenCalled());
    expect(refreshMocks.page).not.toHaveBeenCalled();
    expect(
      screen.getByRole("alertdialog", { name: "清空当前工作区的 Socket 抓包？" }),
    ).toBeVisible();
  });

  it("fails closed when the clear command returns a malformed result", async () => {
    const user = userEvent.setup();
    commandMocks.socketCaptureClear.mockResolvedValue({ message: "not enough" });
    render(<SocketCaptureView />);
    await user.click(screen.getByRole("button", { name: "清空 Socket 抓包" }));
    await user.click(screen.getByRole("button", { name: "确认清空" }));
    await waitFor(() => expect(commandMocks.socketCaptureClear).toHaveBeenCalled());
    expect(refreshMocks.page).not.toHaveBeenCalled();
    expect(
      screen.getByRole("alertdialog", { name: "清空当前工作区的 Socket 抓包？" }),
    ).toBeVisible();
  });

  it("releases detail and focuses the list when a refreshed page removes selection", async () => {
    const user = userEvent.setup();
    const view = render(<SocketCaptureView />);
    const row = screen
      .getByRole("grid", { name: "Socket 抓包记录" })
      .querySelector<HTMLElement>(`[data-key="${pageFixture.rows[0].capture_id}"]`);
    await user.click(row!);
    expect(screen.getByRole("dialog", { name: "Socket 抓包详情" })).toBeVisible();
    state.pageData = { ...pageFixture, rows: [], total: 0 };
    view.rerender(<SocketCaptureView />);
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Socket 抓包详情" })).toBeNull(),
    );
    expect(refreshMocks.invalidateDetail).toHaveBeenCalled();
    expect(document.getElementById("socket-capture-list")).toHaveFocus();
  });
});
