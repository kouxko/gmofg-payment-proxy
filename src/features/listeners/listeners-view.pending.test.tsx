// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  bootstrap,
  listenerOverview,
  listenerStatus,
  mocks,
  navigationMocks,
  ok,
  setupListenerMocks,
  socketListener,
  workspace,
} from "./listeners-view.test-support";

vi.mock("@/features/shell/workspace-navigation", () => ({ useWorkspaceNavigation: () => navigationMocks }));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: () => undefined,
  useBootstrap: () => ({ bootstrap }),
}));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));

import { ListenersView } from "./listeners-view";

describe("统一代理监听编辑器的运行中状态", () => {
  beforeEach(setupListenerMocks);

  it("Socket 启动和停止请求 pending 时显示精确状态并禁止重复操作", async () => {
    const socket = socketListener("socket-1", "Socket 入口", 9000, "transparent");
    mocks.workspaceGet.mockReturnValue(ok({ ...workspace, listeners: [socket] }));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([listenerStatus("socket-1")])));
    mocks.listenerStart.mockReturnValue(new Promise(() => undefined));
    const user = userEvent.setup();
    const view = render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "启动监听" }));
    expect(await screen.findByRole("button", { name: "启动中…" })).toBeDisabled();
    expect(mocks.listenerStart).toHaveBeenCalledOnce();

    view.unmount();
    setupListenerMocks();
    mocks.workspaceGet.mockReturnValue(ok({ ...workspace, listeners: [socket] }));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([
      listenerStatus("socket-1", "running"),
    ])));
    mocks.listenerStop.mockReturnValue(new Promise(() => undefined));
    render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "停止监听" }));
    expect(await screen.findByRole("button", { name: "停止中…" })).toBeDisabled();
    expect(mocks.listenerStop).toHaveBeenCalledOnce();
  });
});
