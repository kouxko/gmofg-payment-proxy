// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WorkspaceNavigationProvider } from "@/features/shell/workspace-navigation";
import { ExchangeObservationView } from "./exchange-observation-view";
import { exchangePage, exchangeRecord } from "./exchange-observation-test-fixture";

const queryMocks = vi.hoisted(() => ({
  workspaceRefresh: vi.fn(async () => undefined),
  pageRefresh: vi.fn(async () => undefined),
  detailRefresh: vi.fn(async () => undefined),
  detailInvalidate: vi.fn(),
}));
const eventRegistrations = vi.hoisted(() => [] as Array<{
  eventTypes: readonly string[];
  refresh: () => Promise<void>;
  options?: { paused?: boolean; entityId?: string };
}>);

vi.mock("@/generated/rust-types", () => ({
  commands: {
    workspaceList: vi.fn(),
    exchangeObservationQuery: vi.fn(),
    exchangeObservationGet: vi.fn(),
    exchangeObservationClear: vi.fn(),
  },
}));

vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: () => "Rust 操作失败",
}));

vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: (
    eventTypes: readonly string[],
    refresh: () => Promise<void>,
    options?: { paused?: boolean; entityId?: string },
  ) => eventRegistrations.push({ eventTypes, refresh, options }),
}));

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) => {
    if (key === "exchange-observation-workspaces") {
      return {
        data: [{
          id: "10000000-0000-0000-0000-000000000001",
          name: "当前 Workspace",
          revision: 1,
          listener_count: 1,
          enabled_listener_count: 1,
          selected: true,
        }],
        isLoading: false,
        refresh: queryMocks.workspaceRefresh,
      };
    }
    if (key.startsWith("exchange-observation-query:")) {
      return {
        data: exchangePage(),
        isLoading: false,
        refresh: queryMocks.pageRefresh,
      };
    }
    return {
      data: key.endsWith(":none") ? undefined : exchangeRecord(),
      isLoading: false,
      refresh: queryMocks.detailRefresh,
      invalidate: queryMocks.detailInvalidate,
    };
  },
}));

describe("ExchangeObservationView realtime refresh", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventRegistrations.length = 0;
  });

  it("refreshes the unified HTTP and Socket list when Rust records new evidence", () => {
    render(<WorkspaceNavigationProvider><ExchangeObservationView /></WorkspaceNavigationProvider>);

    expect(eventRegistrations).toContainEqual(expect.objectContaining({
      eventTypes: expect.arrayContaining(["exchange_observation_changed"]),
      refresh: queryMocks.pageRefresh,
    }));
  });

  it("refreshes only the open Exchange detail for its matching realtime event", async () => {
    const user = userEvent.setup();
    render(<WorkspaceNavigationProvider><ExchangeObservationView /></WorkspaceNavigationProvider>);

    await user.click(screen.getByRole("button", { name: /18:00:00/ }));

    expect(eventRegistrations).toContainEqual(expect.objectContaining({
      eventTypes: expect.arrayContaining(["exchange_observation_changed"]),
      refresh: queryMocks.detailRefresh,
      options: expect.objectContaining({ entityId: "exchange-1", paused: false }),
    }));
  });
});
