// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { bootstrap, setupListenerMocks, navigationMocks, mocks, workspace, fixedListener, ok } from "./listeners-view.test-support";

vi.mock("@/features/shell/workspace-navigation", () => ({ useWorkspaceNavigation: () => navigationMocks }));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: () => undefined,
  useBootstrap: () => ({ bootstrap }),
}));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));

import { ListenersView } from "./listeners-view";

describe("统一代理监听编辑器", () => {
  beforeEach(setupListenerMocks);

  it("保存请求和响应各自选择的自动或强制正文编码", async () => {
    const fixedWorkspace = {
      ...workspace,
      listeners: [{
        ...fixedListener("fixed-1", "交易", 16627, "https://server.test:443"),
        request_body_codec: "shift_jis" as const,
        response_body_codec: "utf8" as const,
      }],
    };
    mocks.workspaceGet.mockReturnValue(ok(fixedWorkspace));
    const user = userEvent.setup(); render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: /请求正文编码/ }));
    await user.click(await screen.findByRole("option", { name: "自动（读取 Content-Type charset）" }));
    await user.click(screen.getByRole("button", { name: /响应正文编码/ }));
    await user.click(await screen.findByRole("option", { name: "强制 Shift-JIS" }));
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));

    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    const savedListener = mocks.listenerSave.mock.calls[0][2];
    expect(savedListener.request_body_codec).toBe("auto");
    expect(savedListener.response_body_codec).toBe("shift_jis");
  });

  it("动态目标监听的 Basic 密码只进入 Rust 安全存储", async () => {
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("switch", { name: "启用 HTTP Basic 认证" }));
    await user.type(screen.getByRole("textbox", { name: "代理认证用户名" }), "operator");
    await user.type(screen.getByLabelText("代理认证密码"), "secret");
    await user.click(screen.getByRole("button", { name: "保护并引用" }));
    expect(mocks.workspaceSecretStoreBasic).toHaveBeenCalledWith("operator", "secret");
    expect(await screen.findByText(/system\/secret-ref-1/)).toBeVisible();
  });

  it("说明监听流量如何进入故障模拟", async () => {
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "去添加故障模拟" }));
    expect(navigationMocks.navigate).toHaveBeenCalledWith("/faults");
});
});
