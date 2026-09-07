// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { bootstrap, setupListenerMocks, mocks, workspace, dynamicListener, fixedListener, ok, navigationMocks } from "./listeners-view.test-support";

vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceNavigation: () => navigationMocks,
  useWorkspaceQueryInvalidation: vi.fn(),
}));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: () => undefined,
  useBootstrap: () => ({ bootstrap }),
}));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));

import { ListenersView } from "./listeners-view";

describe("Listener Draft 结构转换组件竞态", () => {
  beforeEach(setupListenerMocks);

  it("默认按请求目标转发，并可在同一监听启用固定 Server", async () => {
    const fixed = fixedListener("listener-1", "默认代理监听", 8080, "");
    mocks.listenerDraftTransition
      .mockReturnValueOnce(ok(fixed))
      .mockReturnValueOnce(ok(dynamicListener()));
    const user = userEvent.setup();
    render(<ListenersView />);
    expect(await screen.findByText("1. 响应方式")).toBeVisible();
    const responseMode = screen.getByLabelText("HTTP 响应方式");
    expect(responseMode).toHaveTextContent("按原请求目标转发");
    expect(screen.getByText(/读取每个请求中的目标主机和端口/)).toBeVisible();
    expect(screen.getByRole("switch", { name: "为此监听启用 TLS" })).toBeVisible();
    expect(screen.queryByRole("textbox", { name: "固定 Server URL" })).not.toBeInTheDocument();
    await user.click(responseMode);
    await user.click(await screen.findByRole("option", { name: "转发到固定 Server" }));
    const serverUrl = await screen.findByRole("textbox", { name: "固定 Server URL" });
    await user.type(serverUrl, "https://server.test:443");
    expect(screen.getByText("固定 Server 目标")).toBeVisible();
    expect(screen.getByText(/仅用 Server URL 替换目标 host\/port/)).toBeVisible();
    expect(screen.getByText(/原请求 path 与 query 原样保留/)).toBeVisible();
    expect(screen.getByRole("switch", { name: "启用 HTTP Basic 认证" })).toBeVisible();
    expect(screen.getByRole("switch", { name: "为此监听启用 TLS" })).toBeVisible();
    expect(screen.queryByRole("switch", { name: "启用 allowlist MITM" })).not.toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "校验上游服务器主机名" })).toBeChecked();
    expect(screen.getByRole("button", { name: "导入 Server CA" })).toBeVisible();
    expect(screen.getByRole("button", { name: "导入客户端身份" })).toBeVisible();
    expect(screen.queryByRole("textbox", { name: /Body Codec 引用/ })).not.toBeInTheDocument();
    expect(screen.getByText(/自动模式遵循 Header；强制模式覆盖 charset/)).toBeVisible();
    expect(screen.getByRole("button", { name: /请求正文编码/ })).toBeVisible();
    expect(screen.getByRole("button", { name: /响应正文编码/ })).toBeVisible();
    await user.click(responseMode);
    await user.click(await screen.findByRole("option", { name: "按原请求目标转发" }));
    expect(screen.queryByRole("textbox", { name: "固定 Server URL" })).not.toBeInTheDocument();
    expect(responseMode).toHaveTextContent("按原请求目标转发");
  });

  it("叶字段编辑同步取消旧结构转换且迟到响应不能覆盖草稿", async () => {
    const fixed = fixedListener("listener-1", "默认代理监听", 8080, "https://server.test:443");
    const requestTarget = dynamicListener();
    let resolveTransition!: (listener: typeof requestTarget) => void;
    mocks.workspaceGet.mockReturnValue(ok({ ...workspace, listeners: [fixed] }));
    mocks.listenerDraftTransition.mockReturnValue(new Promise((resolve) => {
      resolveTransition = resolve;
    }));
    const user = userEvent.setup();
    render(<ListenersView />);

    await user.click(await screen.findByLabelText("HTTP 响应方式"));
    await user.click(await screen.findByRole("option", { name: "按原请求目标转发" }));
    const name = screen.getByRole("textbox", { name: "代理监听名称" });
    await user.clear(name);
    await user.type(name, "保留叶字段草稿");
    await act(async () => resolveTransition(requestTarget));

    expect(screen.getByRole("textbox", { name: "代理监听名称" })).toHaveValue("保留叶字段草稿");
    expect(screen.getByRole("textbox", { name: "固定 Server URL" })).toHaveValue("https://server.test:443");
    expect(screen.queryByText("正在切换模式…")).not.toBeInTheDocument();
  });

  it("固定 Server 关闭时保留 Basic 与 MITM 设置", async () => {
    render(<ListenersView />);
    expect(await screen.findByRole("switch", { name: "启用 HTTP Basic 认证" })).toBeVisible();
    expect(screen.getByRole("switch", { name: "启用 allowlist MITM" })).toBeVisible();
  });

});
