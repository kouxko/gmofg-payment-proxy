// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  bootstrap,
  dynamicListener,
  fixedListener,
  listenerOverview,
  listenerStatus,
  mocks,
  navigationMocks,
  ok,
  setupListenerMocks,
  workspace,
} from "./listeners-view.test-support";

const invalidation = vi.hoisted(() => ({
  current: undefined as undefined | Array<{ refresh: () => Promise<void> }>,
}));

vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceNavigation: () => navigationMocks,
  useWorkspaceQueryInvalidation: (
    { current }: { current: Array<{ refresh: () => Promise<void> }> },
  ) => {
    invalidation.current = current;
  },
}));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: () => undefined,
  useBootstrap: () => ({ bootstrap }),
}));
vi.mock("@/features/shell/selected-workspace-context", () => ({ useSelectedWorkspace: () => ({ selected: { id: "workspace-1", revision: 1 }, summaries: [], isLoading: false, refresh: vi.fn() }) }));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));

import { ListenersView } from "./listeners-view";

describe("Listener external refresh", () => {
  const listenerA = dynamicListener("listener-a", "监听 A", 8080);
  const listenerB = dynamicListener("listener-b", "监听 B", 8081);
  const base = { ...workspace, listeners: [listenerA, listenerB] };

  beforeEach(() => {
    setupListenerMocks();
    invalidation.current = undefined;
    mocks.workspaceGet.mockReturnValue(ok(base));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([
      listenerStatus("listener-a"),
      listenerStatus("listener-b"),
    ])));
  });

  it("preserves a local draft and blocks save and start when the same listener changed externally", async () => {
    const user = userEvent.setup();
    render(<ListenersView />);
    const name = await screen.findByRole("textbox", { name: "代理监听名称" });
    await user.clear(name);
    await user.type(name, "监听 A 本地草稿");
    const latest = {
      ...base,
      revision: 2,
      listeners: [{ ...listenerA, name: "监听 A 外部修改" }, listenerB],
    };
    mocks.workspaceGet.mockReturnValue(ok(latest));

    await act(async () => {
      await invalidation.current?.[0].refresh();
    });

    expect(name).toHaveValue("监听 A 本地草稿");
    expect(await screen.findByText("当前监听存在外部修改冲突")).toBeVisible();
    expect(screen.getByRole("button", { name: "保存当前监听" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "启动监听" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "删除监听" })).toBeDisabled();
    expect(mocks.listenerSave).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "校验当前监听" }));
    expect(await screen.findByText("结构有效，但须先解决外部修改冲突。"))
      .toBeVisible();
    expect(screen.getByRole("button", {
      name: "放弃当前监听草稿并加载最新配置",
    })).toBeEnabled();
    await user.click(screen.getByRole("button", {
      name: "放弃当前监听草稿并加载最新配置",
    }));

    expect(name).toHaveValue("监听 A 外部修改");
    expect(screen.queryByText("当前监听存在外部修改冲突")).not
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存当前监听" })).toBeEnabled();
  });

  it("refreshes another listener without blocking the current listener draft", async () => {
    const user = userEvent.setup();
    render(<ListenersView />);
    const name = await screen.findByRole("textbox", { name: "代理监听名称" });
    await user.clear(name);
    await user.type(name, "监听 A 本地草稿");
    const latest = {
      ...base,
      revision: 2,
      listeners: [listenerA, { ...listenerB, name: "监听 B 外部修改" }],
    };
    mocks.workspaceGet.mockReturnValue(ok(latest));

    await act(async () => {
      await invalidation.current?.[0].refresh();
    });

    expect(screen.queryByText("当前监听存在外部修改冲突")).not
      .toBeInTheDocument();
    expect(name).toHaveValue("监听 A 本地草稿");
    expect(screen.getByRole("button", { name: "保存当前监听" })).toBeEnabled();
    await user.click(screen.getByText("监听 B 外部修改"));
    expect(screen.getByRole("textbox", { name: "代理监听名称" })).toHaveValue(
      "监听 B 外部修改",
    );
    await user.click(screen.getByText("监听 A 本地草稿"));
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));

    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    expect(mocks.listenerSave).toHaveBeenCalledWith(
      "workspace-1",
      2,
      expect.objectContaining({ id: "listener-a", name: "监听 A 本地草稿" }),
      [],
    );
  });

  it("resolves only the conflicted listener and preserves another listener draft and certificate", async () => {
    const fixedB = fixedListener(
      "listener-b",
      "监听 B",
      8081,
      "https://server.test:443",
    );
    const certificateBase = { ...workspace, listeners: [listenerA, fixedB] };
    mocks.workspaceGet.mockReturnValue(ok(certificateBase));
    const user = userEvent.setup();
    render(<ListenersView />);

    const name = await screen.findByRole("textbox", { name: "代理监听名称" });
    await user.clear(name);
    await user.type(name, "监听 A 本地草稿");
    await user.click(screen.getByText("监听 B"));
    const nameB = screen.getByRole("textbox", { name: "代理监听名称" });
    await user.clear(nameB);
    await user.type(nameB, "监听 B 本地草稿");
    await user.click(screen.getByRole("button", { name: "导入 Server CA" }));
    await user.click(screen.getByRole("button", {
      name: "选择 CA 证书（.cer / .crt / .pem / .der）",
    }));
    await user.click(screen.getByText("监听 A 本地草稿"));
    const latest = {
      ...certificateBase,
      revision: 2,
      listeners: [{ ...listenerA, name: "监听 A 外部修改" }, fixedB],
    };
    mocks.workspaceGet.mockReturnValue(ok(latest));

    await act(async () => {
      await invalidation.current?.[0].refresh();
    });
    await user.click(screen.getByRole("button", {
      name: "放弃当前监听草稿并加载最新配置",
    }));

    expect(name).toHaveValue("监听 A 外部修改");
    await user.click(screen.getByText("监听 B 本地草稿"));
    expect(screen.getByRole("textbox", { name: "代理监听名称" })).toHaveValue(
      "监听 B 本地草稿",
    );
    expect(await screen.findByText("CN=测试上游 CA")).toBeVisible();
    expect(mocks.listenerCertificateDiscard).not.toHaveBeenCalled();
  });

  it("removes a dirty local listener only after the user accepts its external deletion", async () => {
    const user = userEvent.setup();
    render(<ListenersView />);
    const name = await screen.findByRole("textbox", { name: "代理监听名称" });
    await user.clear(name);
    await user.type(name, "监听 A 本地草稿");
    mocks.workspaceGet.mockReturnValue(ok({
      ...base,
      revision: 2,
      listeners: [listenerB],
    }));

    await act(async () => {
      await invalidation.current?.[0].refresh();
    });
    expect(name).toHaveValue("监听 A 本地草稿");
    await user.click(screen.getByRole("button", {
      name: "放弃当前监听草稿并加载最新配置",
    }));

    expect(screen.queryByText("监听 A 本地草稿")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "代理监听名称" })).toHaveValue(
      "监听 B",
    );
  });
});
