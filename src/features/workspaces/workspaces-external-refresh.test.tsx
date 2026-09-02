// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UiEventEnvelope } from "@/generated/rust-types";
import { BootstrapProvider } from "@/features/shell/bootstrap-context";
import { WorkspacesView } from "./workspaces-view";

const testState = vi.hoisted(() => ({
  channel: undefined as
    | { onmessage: (event: UiEventEnvelope) => void }
    | undefined,
  summary: {
    id: "workspace-1",
    name: "API Lab",
    revision: 1,
    listener_count: 0,
    enabled_listener_count: 0,
    selected: true,
  },
  workspace: {
    id: "workspace-1",
    name: "API Lab",
    revision: 1,
    listeners: [] as object[],
    metadata_extractors: [],
    response_assertions: [],
    fault_presets: [],
    certificate_references: [],
  },
  detailError: undefined as unknown,
  commands: {
    workspaceList: vi.fn(),
    workspaceGet: vi.fn(),
    appBootstrap: vi.fn(),
    appSubscribeEvents: vi.fn(),
    appUnsubscribeEvents: vi.fn(),
  },
}));

vi.mock("@/generated/rust-types", () => ({ commands: testState.commands }));
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class<T> {
    onmessage: (message: T) => void = () => undefined;
  },
}));

function ok<T>(data: T) {
  return Promise.resolve({ status: "ok" as const, data });
}

function environmentCommitEvent(): UiEventEnvelope {
  return {
    event_id: 2,
    runtime_epoch: null,
    occurred_at: "2026-08-28T13:03:22Z",
    entity_id: "workspace-1",
    entity_revision: 2,
    payload: {
      type: "snapshot_required",
      data: { reason: "environment_configuration_committed" },
    },
  };
}

describe("Workspace management external refresh", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testState.channel = undefined;
    testState.summary = {
      id: "workspace-1",
      name: "API Lab",
      revision: 1,
      listener_count: 0,
      enabled_listener_count: 0,
      selected: true,
    };
    testState.workspace = {
      id: "workspace-1",
      name: "API Lab",
      revision: 1,
      listeners: [],
      metadata_extractors: [],
      response_assertions: [],
      fault_presets: [],
      certificate_references: [],
    };
    testState.detailError = undefined;
    testState.commands.workspaceList.mockImplementation(() =>
      ok([testState.summary]),
    );
    testState.commands.workspaceGet.mockImplementation(() =>
      testState.detailError
        ? Promise.reject(testState.detailError)
        : ok(testState.workspace),
    );
    testState.commands.appBootstrap.mockReturnValue(
      ok({ product_name: "Intercept Proxy", event_cursor: 1 }),
    );
    testState.commands.appSubscribeEvents.mockImplementation(
      (_afterEventId, channel) => {
        testState.channel = channel;
        return ok({
          subscription_id: 1,
          accepted_after_event_id: 1,
          current_event_id: 1,
          snapshot_required: false,
        });
      },
    );
    testState.commands.appUnsubscribeEvents.mockReturnValue(ok(null));
  });

  it("refreshes the open list and detail after snapshot_required", async () => {
    const user = userEvent.setup();
    render(
      <BootstrapProvider>
        <WorkspacesView />
      </BootstrapProvider>,
    );

    const table = await screen.findByLabelText("Workspace 列表");
    await waitFor(() =>
      expect(table).toHaveTextContent(/API Lab\s*0\s*0\s*1\s*当前/),
    );
    const description = screen.getByText("ID").closest("dl");
    expect(description).toHaveTextContent(/代理入口\s*0\s*版本\s*1/);
    const name = screen.getByLabelText("Workspace 名称");
    await user.clear(name);
    await user.type(name, "本地未保存名称");
    await waitFor(() => expect(testState.channel).toBeDefined());

    testState.summary = {
      ...testState.summary,
      listener_count: 1,
      revision: 2,
    };
    testState.workspace = {
      ...testState.workspace,
      listeners: [{}],
      revision: 2,
    };
    testState.channel?.onmessage(environmentCommitEvent());

    await waitFor(() =>
      expect(table).toHaveTextContent(/API Lab\s*1\s*0\s*2\s*当前/),
    );
    expect(name).toHaveValue("本地未保存名称");
    expect(description).toHaveTextContent(/代理入口\s*1\s*版本\s*2/);
  });

  it("marks retained detail as stale when an external refresh fails", async () => {
    render(
      <BootstrapProvider>
        <WorkspacesView />
      </BootstrapProvider>,
    );

    const description = await screen.findByText("ID").then((node) =>
      node.closest("dl"),
    );
    expect(description).toHaveTextContent(/代理入口\s*0\s*版本\s*1/);
    await waitFor(() => expect(testState.channel).toBeDefined());

    testState.detailError = {
      code: "WORKSPACE_READ_FAILED",
      message: "Workspace 读取失败",
      field_errors: {},
      entity_id: "workspace-1",
      runtime_epoch: null,
    };
    testState.channel?.onmessage(environmentCommitEvent());

    expect(
      await screen.findByText("Workspace 详情刷新失败"),
    ).toBeInTheDocument();
    expect(screen.getByText(/Workspace 读取失败/)).toBeInTheDocument();
    expect(screen.getByText(/以下为刷新前快照/)).toBeInTheDocument();
    expect(description).toHaveTextContent(/代理入口\s*0\s*版本\s*1/);
  });

  it("distinguishes an initial detail read failure from a stale snapshot", async () => {
    testState.detailError = {
      code: "WORKSPACE_READ_FAILED",
      message: "Workspace 读取失败",
      field_errors: {},
      entity_id: "workspace-1",
      runtime_epoch: null,
    };

    render(
      <BootstrapProvider>
        <WorkspacesView />
      </BootstrapProvider>,
    );

    expect(
      await screen.findByText("Workspace 详情读取失败"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/未能读取所选 Workspace 的详情/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/以下为刷新前快照/)).not.toBeInTheDocument();
    expect(
      screen.queryByText("选择一个 Workspace 查看详情。"),
    ).not.toBeInTheDocument();
    expect(screen.getByText("详情暂不可用，请重试。")).toBeInTheDocument();
  });
});
