// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UiEventEnvelope } from "@/generated/rust-types";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { BootstrapProvider } from "./bootstrap-context";
import {
  useWorkspaceQueryInvalidation,
  WorkspaceNavigationProvider,
} from "./workspace-navigation";

type WorkspaceAuthority = {
  selectedId: string;
  summaries: Array<{ id: string; selected: boolean; listener_count: number }>;
  listenerCount: number;
};

const clientMocks = vi.hoisted(() => ({
  appBootstrap: vi.fn(),
  eventHandler: undefined as ((event: UiEventEnvelope) => void) | undefined,
  unsubscribe: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@/lib/ipc/client", () => ({
  appBootstrap: clientMocks.appBootstrap,
  errorMessage: (reason: unknown) => String(reason),
  subscribeToAppEvents: vi.fn(
    async (_cursor: number, handler: (event: UiEventEnvelope) => void) => {
      clientMocks.eventHandler = handler;
      return {
        ack: {
          subscription_id: 1,
          accepted_after_event_id: 1,
          current_event_id: 1,
          snapshot_required: false,
        },
        unsubscribe: clientMocks.unsubscribe,
      };
    },
  ),
}));

function workspaceEvent(eventId: number, workspaceId: string): UiEventEnvelope {
  return {
    event_id: eventId,
    runtime_epoch: null,
    occurred_at: `2026-08-28T00:00:0${eventId}Z`,
    entity_id: workspaceId,
    entity_revision: eventId,
    payload: {
      type: "workspace_changed",
      data: {
        workspace_id: workspaceId,
        kind: "updated",
        summary: null,
      },
    },
  };
}

function environmentCommitEvent(
  eventId: number,
  workspaceId: string,
): UiEventEnvelope {
  return {
    event_id: eventId,
    runtime_epoch: null,
    occurred_at: `2026-08-28T00:00:0${eventId}Z`,
    entity_id: workspaceId,
    entity_revision: eventId,
    payload: {
      type: "snapshot_required",
      data: { reason: "environment_configuration_committed" },
    },
  };
}

function WorkspaceFacts({
  authority,
  loadListeners,
}: {
  authority: WorkspaceAuthority;
  loadListeners?: () => Promise<number>;
}) {
  const workspaces = useIpcQuery("test-workspace-list", async () => [
    ...authority.summaries,
  ]);
  const workspaceId = workspaces.data?.find((item) => item.selected)?.id;
  const listeners = useIpcQuery(
    `test-workspace-listeners:${workspaceId ?? "none"}`,
    loadListeners ?? (async () => authority.listenerCount),
    undefined,
    { enabled: Boolean(workspaceId) },
  );
  const toolbar = useIpcQuery(
    `test-workspace-toolbar:${workspaceId ?? "none"}`,
    async () => authority.listenerCount,
    undefined,
    { enabled: Boolean(workspaceId) },
  );
  const ruleCapability = useIpcQuery(
    `test-workspace-rule-capability:${workspaceId ?? "none"}`,
    async () => authority.listenerCount > 0,
    undefined,
    { enabled: Boolean(workspaceId) },
  );

  useWorkspaceQueryInvalidation({
    workspaceId,
    collection: [workspaces],
    current: [listeners, toolbar, ruleCapability],
  });

  return (
    <>
      <output aria-label="入口列表">{listeners.data ?? "loading"}</output>
      <output aria-label="顶栏入口计数">{toolbar.data ?? "loading"}</output>
      <output aria-label="规则创建能力">
        {ruleCapability.data === undefined
          ? "loading"
          : ruleCapability.data
            ? "enabled"
            : "disabled"}
      </output>
      <output aria-label="Workspace 摘要">
        {workspaces.data?.map((item) => `${item.id}:${item.listener_count}`).join("|")}
      </output>
    </>
  );
}

function renderFacts(
  authority: WorkspaceAuthority,
  loadListeners?: () => Promise<number>,
) {
  return render(
    <WorkspaceNavigationProvider>
      <BootstrapProvider>
        <WorkspaceFacts authority={authority} loadListeners={loadListeners} />
      </BootstrapProvider>
    </WorkspaceNavigationProvider>,
  );
}

describe("external Workspace query invalidation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    clientMocks.eventHandler = undefined;
    clientMocks.appBootstrap.mockResolvedValue({
      product_name: "Intercept Proxy",
      event_cursor: 1,
    });
  });

  it("refreshes the open listener page, toolbar and rule capability for 0 to 2 to 0", async () => {
    const authority: WorkspaceAuthority = {
      selectedId: "workspace-1",
      summaries: [{ id: "workspace-1", selected: true, listener_count: 0 }],
      listenerCount: 0,
    };
    renderFacts(authority);

    await waitFor(() =>
      expect(screen.getByLabelText("入口列表")).toHaveTextContent("0"),
    );
    expect(screen.getByLabelText("顶栏入口计数")).toHaveTextContent("0");
    expect(screen.getByLabelText("规则创建能力")).toHaveTextContent("disabled");
    await waitFor(() => expect(clientMocks.eventHandler).toBeTypeOf("function"));

    authority.listenerCount = 2;
    authority.summaries = [{ id: "workspace-1", selected: true, listener_count: 2 }];
    clientMocks.eventHandler?.(environmentCommitEvent(2, "workspace-1"));

    await waitFor(() => expect(screen.getByLabelText("入口列表")).toHaveTextContent("2"));
    expect(screen.getByLabelText("顶栏入口计数")).toHaveTextContent("2");
    expect(screen.getByLabelText("规则创建能力")).toHaveTextContent("enabled");

    authority.listenerCount = 0;
    authority.summaries = [{ id: "workspace-1", selected: true, listener_count: 0 }];
    clientMocks.eventHandler?.(environmentCommitEvent(3, "workspace-1"));

    await waitFor(() => expect(screen.getByLabelText("入口列表")).toHaveTextContent("0"));
    expect(screen.getByLabelText("顶栏入口计数")).toHaveTextContent("0");
    expect(screen.getByLabelText("规则创建能力")).toHaveTextContent("disabled");
  });

  it("refreshes collection summaries but not current details for another Workspace", async () => {
    const loadListeners = vi.fn(async () => 0);
    const authority: WorkspaceAuthority = {
      selectedId: "workspace-1",
      summaries: [
        { id: "workspace-1", selected: true, listener_count: 0 },
        { id: "workspace-2", selected: false, listener_count: 0 },
      ],
      listenerCount: 0,
    };
    renderFacts(authority, loadListeners);

    await waitFor(() =>
      expect(screen.getByLabelText("入口列表")).toHaveTextContent("0"),
    );
    await waitFor(() => expect(clientMocks.eventHandler).toBeTypeOf("function"));
    const currentLoads = loadListeners.mock.calls.length;
    authority.summaries = [
      { id: "workspace-1", selected: true, listener_count: 0 },
      { id: "workspace-2", selected: false, listener_count: 2 },
    ];

    clientMocks.eventHandler?.(workspaceEvent(2, "workspace-2"));

    await waitFor(() =>
      expect(screen.getByLabelText("Workspace 摘要")).toHaveTextContent(
        "workspace-2:2",
      ),
    );
    expect(loadListeners).toHaveBeenCalledTimes(currentLoads);
    expect(screen.getByLabelText("入口列表")).toHaveTextContent("0");
  });

  it("invalidates an in-flight response as soon as a newer Workspace event arrives", async () => {
    const stale = deferred<number>();
    const fresh = deferred<number>();
    const loadListeners = vi
      .fn<() => Promise<number>>()
      .mockResolvedValueOnce(0)
      .mockReturnValueOnce(stale.promise)
      .mockReturnValueOnce(fresh.promise);
    const authority: WorkspaceAuthority = {
      selectedId: "workspace-1",
      summaries: [{ id: "workspace-1", selected: true, listener_count: 0 }],
      listenerCount: 0,
    };
    renderFacts(authority, loadListeners);

    await waitFor(() =>
      expect(screen.getByLabelText("入口列表")).toHaveTextContent("0"),
    );
    await waitFor(() => expect(clientMocks.eventHandler).toBeTypeOf("function"));

    clientMocks.eventHandler?.(workspaceEvent(2, "workspace-1"));
    await waitFor(() => expect(loadListeners).toHaveBeenCalledTimes(2));
    clientMocks.eventHandler?.(workspaceEvent(3, "workspace-1"));
    stale.resolve(2);

    await waitFor(() => expect(loadListeners).toHaveBeenCalledTimes(3));
    expect(screen.getByLabelText("入口列表")).not.toHaveTextContent("2");
    fresh.resolve(0);
    await waitFor(() => expect(screen.getByLabelText("入口列表")).toHaveTextContent("0"));
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}
