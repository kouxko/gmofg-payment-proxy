// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { BreakpointsView } from "./breakpoints-view";

const queryMocks = vi.hoisted(() => ({
  refresh: vi.fn(),
  invalidate: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: {
    breakpointQuery: vi.fn(),
  },
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
          data: [],
          error: undefined,
          isLoading: false,
          refresh: queryMocks.refresh,
        }
      : {
          data: undefined,
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
    searchParams: new URLSearchParams(),
  }),
}));

describe("BreakpointsView queue controls", () => {
  it("explains and executes the icon-only refresh button", async () => {
    queryMocks.refresh.mockResolvedValue(undefined);
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
});
