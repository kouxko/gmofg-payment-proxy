// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  DiagnosticLogPageViewModel,
  ProxyWorkspace,
  WorkspaceSummaryViewModel,
} from "@/generated/rust-types";
import { DiagnosticLogsView } from "./diagnostic-logs-view";

const commandMocks = vi.hoisted(() => ({
  diagnosticLogQuery: vi.fn(),
  diagnosticReproductionReportExport: vi.fn(),
  workspaceList: vi.fn(),
  workspaceGet: vi.fn(),
}));
const eventRefreshMock = vi.hoisted(() => vi.fn());

vi.mock("@/generated/rust-types", () => ({ commands: commandMocks }));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: eventRefreshMock,
}));
vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: () => "Rust 操作失败",
}));

const workspaceSummary = {
  id: "11111111-1111-4111-8111-111111111111",
  name: "支付测试",
  revision: 1,
  listener_count: 1,
  enabled_listener_count: 1,
  selected: true,
} satisfies WorkspaceSummaryViewModel;
const listener = {
  id: "22222222-2222-4222-8222-222222222222",
  name: "ISO8583 本机应答",
  bind_address: "127.0.0.1",
  port: 8080,
};
const workspace = {
  id: workspaceSummary.id,
  name: workspaceSummary.name,
  listeners: [listener],
} as ProxyWorkspace;
const page: DiagnosticLogPageViewModel = {
  rows: [],
  current_cursor: 0,
  oldest_retained_event_id: null,
  snapshot_required: false,
  retained_count: 0,
  truncated: false,
  empty_message: "暂无诊断日志",
};

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) => ({
    data: key === "diagnostic-report-workspaces"
      ? [workspaceSummary]
      : key.startsWith("diagnostic-report-workspace:") ? workspace : page,
    error: undefined,
    isLoading: false,
    refresh: vi.fn(),
  }),
}));

describe("DiagnosticLogsView reproduction report", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    page.snapshot_required = false;
    commandMocks.diagnosticReproductionReportExport.mockResolvedValue({
      bytes_written: 4096,
      replaced_existing: false,
    });
  });

  it("exports the exact selected workspace and listener through the native command", async () => {
    const user = userEvent.setup();
    render(<DiagnosticLogsView />);

    await user.click(screen.getByRole("button", { name: "导出复现 Markdown" }));

    await waitFor(() => {
      expect(commandMocks.diagnosticReproductionReportExport).toHaveBeenCalledWith({
        workspace_id: workspaceSummary.id,
        listener_id: listener.id,
      });
    });
    expect(screen.getByText("故障复现报告")).toBeInTheDocument();
  });

  it("does not render a redundant clear-filter action", () => {
    render(<DiagnosticLogsView />);

    expect(screen.queryByRole("button", { name: "清除" })).not.toBeInTheDocument();
  });

  it("shows an explicit refresh warning when the requested cursor was evicted", () => {
    page.snapshot_required = true;

    render(<DiagnosticLogsView />);

    expect(screen.getByText("诊断事件游标已过期")).toBeVisible();
  });
});
