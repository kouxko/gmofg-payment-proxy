// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UiEventEnvelope } from "@/generated/rust-types";
import {
  BootstrapProvider,
  useAppEventRefresh,
  useBootstrap,
} from "./bootstrap-context";

const clientMocks = vi.hoisted(() => ({
  appBootstrap: vi.fn(),
  subscribeToAppEvents: vi.fn(),
  unsubscribe: vi.fn().mockResolvedValue(undefined),
  eventHandler: undefined as ((event: UiEventEnvelope) => void) | undefined,
}));

vi.mock("@/lib/ipc/client", () => ({
  appBootstrap: clientMocks.appBootstrap,
  errorMessage: (reason: unknown) => String(reason),
  subscribeToAppEvents: vi.fn(
    async (_cursor: number, handler: (event: UiEventEnvelope) => void) => {
      clientMocks.eventHandler = handler;
      return {
        ack: {
          subscription_id: 9,
          accepted_after_event_id: 4,
          current_event_id: 4,
          snapshot_required: false,
        },
        unsubscribe: clientMocks.unsubscribe,
      };
    },
  ),
}));

function EventProbe({ refresh }: { refresh: () => Promise<void> }) {
  useAppEventRefresh(["session_updated"], refresh);
  const { proxy } = useBootstrap();
  return <div>{proxy?.state_text}</div>;
}

function RefreshProbe() {
  const { proxy, refresh } = useBootstrap();
  return (
    <>
      <div>{proxy?.state_text}</div>
      <button onClick={() => void refresh()}>刷新快照</button>
    </>
  );
}

function SettingsProbe() {
  const { bootstrap } = useBootstrap();
  return <div>{bootstrap?.settings?.stored.leaf_sans.join(",")}</div>;
}

describe("BootstrapProvider event distribution", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    clientMocks.eventHandler = undefined;
    clientMocks.appBootstrap.mockResolvedValue({
      proxy: { state_text: "运行中" },
      event_cursor: 4,
    });
  });

  it("dispatches a subscribed Rust event to page refresh handlers", async () => {
    const refresh = vi.fn().mockResolvedValue(undefined);
    const view = render(
      <BootstrapProvider>
        <EventProbe refresh={refresh} />
      </BootstrapProvider>,
    );

    expect(await screen.findByText("运行中")).toBeVisible();
    await waitFor(() => expect(clientMocks.eventHandler).toBeTypeOf("function"));

    clientMocks.eventHandler?.({
      event_id: 5,
      runtime_epoch: "epoch-1",
      occurred_at: "2026-07-28T00:00:00Z",
      entity_id: "session-1",
      entity_revision: 1,
      payload: {
        type: "session_updated",
        data: {} as never,
      },
    });

    await waitFor(() => expect(refresh).toHaveBeenCalledOnce());
    view.unmount();
    expect(clientMocks.unsubscribe).toHaveBeenCalledOnce();
  });

  it("does not let an older bootstrap response overwrite a newer Rust event", async () => {
    let finishRefresh!: (value: unknown) => void;
    clientMocks.appBootstrap
      .mockResolvedValueOnce({
        proxy: { state_text: "初始状态" },
        event_cursor: 4,
      })
      .mockReturnValueOnce(
        new Promise((resolve) => {
          finishRefresh = resolve;
        }),
      );
    const user = userEvent.setup();
    render(
      <BootstrapProvider>
        <RefreshProbe />
      </BootstrapProvider>,
    );

    expect(await screen.findByText("初始状态")).toBeVisible();
    await waitFor(() => expect(clientMocks.eventHandler).toBeTypeOf("function"));
    await user.click(screen.getByRole("button", { name: "刷新快照" }));

    clientMocks.eventHandler?.({
      event_id: 5,
      runtime_epoch: "epoch-1",
      occurred_at: "2026-07-28T00:00:00Z",
      entity_id: null,
      entity_revision: 2,
      payload: {
        type: "runtime_status_changed",
        data: { state_text: "事件新状态" } as never,
      },
    });
    expect(await screen.findByText("事件新状态")).toBeVisible();

    finishRefresh({
      proxy: { state_text: "迟到旧快照" },
      event_cursor: 4,
    });
    await waitFor(() =>
      expect(screen.queryByText("迟到旧快照")).not.toBeInTheDocument(),
    );
    expect(screen.getByText("事件新状态")).toBeVisible();
  });

  it("replaces the global settings snapshot from a normalized Rust event", async () => {
    clientMocks.appBootstrap.mockResolvedValueOnce({
      proxy: { state_text: "运行中" },
      settings: { stored: { leaf_sans: [] } },
      event_cursor: 4,
    });
    render(
      <BootstrapProvider>
        <SettingsProbe />
      </BootstrapProvider>,
    );

    await waitFor(() => expect(clientMocks.eventHandler).toBeTypeOf("function"));
    clientMocks.eventHandler?.({
      event_id: 5,
      runtime_epoch: null,
      occurred_at: "2026-07-30T00:00:00Z",
      entity_id: "settings",
      entity_revision: 2,
      payload: {
        type: "settings_changed",
        data: {
          stored: { leaf_sans: ["10.0.28.99"] },
        } as never,
      },
    });

    expect(await screen.findByText("10.0.28.99")).toBeVisible();
  });
});
