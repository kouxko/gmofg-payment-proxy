// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  workspaceList: vi.fn(),
  refreshForEvents: undefined as (() => Promise<void>) | undefined,
}));

vi.mock("@/generated/rust-types", () => ({
  commands: { workspaceList: mocks.workspaceList },
}));
vi.mock("./bootstrap-context", () => ({
  useAppEventRefresh: (
    _types: string[],
    refresh: () => Promise<void>,
  ) => { mocks.refreshForEvents = refresh; },
}));

import {
  SelectedWorkspaceProvider,
  useSelectedWorkspace,
} from "./selected-workspace-context";

function ok<T>(data: T) {
  return Promise.resolve({ status: "ok" as const, data });
}

function Probe() {
  const state = useSelectedWorkspace();
  return <div>{state.selected?.name ?? state.error ?? "none"}</div>;
}

describe("SelectedWorkspaceProvider", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.refreshForEvents = undefined;
  });

  it("fails closed when Rust does not identify exactly one selected workspace", async () => {
    mocks.workspaceList.mockReturnValue(ok([
      { id: "workspace-a", name: "A", revision: 1, selected: false },
      { id: "workspace-b", name: "B", revision: 1, selected: false },
    ]));

    render(<SelectedWorkspaceProvider><Probe /></SelectedWorkspaceProvider>);

    expect(await screen.findByText("当前 Workspace 选择状态无效。"))
      .toBeVisible();
    expect(screen.queryByText("A")).not.toBeInTheDocument();
  });

  it("keeps the external selection and rejects a late result from the prior refresh", async () => {
    let finishStale!: (value: ReturnType<typeof ok>) => void;
    mocks.workspaceList
      .mockReturnValueOnce(ok([
        { id: "workspace-a", name: "A", revision: 1, selected: true },
        { id: "workspace-b", name: "B", revision: 1, selected: false },
      ]))
      .mockReturnValueOnce(new Promise((resolve) => { finishStale = resolve; }))
      .mockReturnValueOnce(ok([
        { id: "workspace-a", name: "A", revision: 1, selected: false },
        { id: "workspace-b", name: "B", revision: 2, selected: true },
      ]));
    render(<SelectedWorkspaceProvider><Probe /></SelectedWorkspaceProvider>);
    expect(await screen.findByText("A")).toBeVisible();

    let staleRefresh!: Promise<void>;
    await act(async () => {
      staleRefresh = mocks.refreshForEvents!();
      await Promise.resolve();
      await mocks.refreshForEvents!();
    });
    expect(await screen.findByText("B")).toBeVisible();

    finishStale(ok([
      { id: "workspace-a", name: "过期 A", revision: 1, selected: true },
      { id: "workspace-b", name: "B", revision: 2, selected: false },
    ]));
    await act(async () => { await staleRefresh; });
    await waitFor(() => expect(screen.getByText("B")).toBeVisible());
    expect(screen.queryByText("过期 A")).not.toBeInTheDocument();
  });
});
