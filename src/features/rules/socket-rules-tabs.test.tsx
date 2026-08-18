// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { useEffect, useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { RulesView } from "./rules-view";

vi.mock("@/generated/rust-types", () => ({ commands: {} }));
vi.mock("@/lib/ipc/client", () => ({
  appErrorViewModel: () => undefined,
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: () => "Rust 操作失败",
}));
vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) => key === "rule-list"
    ? { data: [], error: undefined, isLoading: false, refresh: vi.fn() }
    : { data: undefined, error: undefined, isLoading: false, refresh: vi.fn(), invalidate: vi.fn() },
}));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: vi.fn(),
  useBootstrap: () => ({ bootstrap: { channel_catalog: [] } }),
}));
vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceNavigation: () => ({ pathname: "/rules", searchParams: new URLSearchParams(), navigate: vi.fn() }),
}));
vi.mock("./protocol-rules-view", () => ({
  ProtocolRulesView: ({ kind }: { kind: "http" | "socket" }) => {
    const [draftName, setDraftName] = useState("");
    const [lateResult, setLateResult] = useState(false);
    useEffect(() => {
      const task = window.setTimeout(() => setLateResult(true), 20);
      return () => window.clearTimeout(task);
    }, []);
    return (
      <section aria-label={`${kind} protocol rules mounted`} className="grid grid-cols-[minmax(520px,1fr)_620px] max-[1280px]:grid-cols-1">
        <h2>{kind === "http" ? "HTTP Body 报文规则" : "Socket 报文规则"}</h2>
        <button type="button">新建报文规则</button>
        <label>
          {kind} 草稿名称
          <input value={draftName} onChange={(event) => setDraftName(event.target.value)} />
        </label>
        {lateResult && <p>Socket 异步结果</p>}
      </section>
    );
  },
}));

describe("HTTP and Socket controlled rule tabs", () => {
  it("shows protocol tabs without a duplicate page title and keeps HTTP actions isolated", () => {
    render(<RulesView />);
    expect(screen.queryByRole("heading", { level: 1, name: "规则" })).toBeNull();
    expect(screen.getByRole("heading", { level: 2, name: "HTTP 拦截规则" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "HTTP" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("button", { name: "新建规则" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "导入规则" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "导出规则" })).toBeEnabled();
    expect(
      screen.getByText("暂无 HTTP 拦截规则，请选择新建规则开始配置"),
    ).toBeVisible();
    expect(screen.queryByLabelText("socket protocol rules mounted")).not.toBeInTheDocument();
  });

  it("switches locally from HTTP regular rules to HTTP Body message rules", async () => {
    const user = userEvent.setup();
    render(<RulesView />);

    expect(screen.getByRole("button", { name: "常规规则" })).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "Body 报文规则" }));

    expect(screen.getByLabelText("http protocol rules mounted")).toBeVisible();
    expect(screen.getByRole("heading", { level: 2, name: "HTTP Body 报文规则" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "导入规则" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("socket protocol rules mounted")).not.toBeInTheDocument();
  });

  it("links the selected tab and panel with the tablist ARIA contract", () => {
    render(<RulesView />);

    const tablist = screen.getByRole("tablist", { name: "规则协议" });
    const httpTab = screen.getByRole("tab", { name: "HTTP" });
    const panel = document.getElementById(httpTab.getAttribute("aria-controls")!);
    expect(tablist.className).not.toMatch(/(?:^|\s)(?:w-full|flex-1)(?:\s|$)/);
    expect(httpTab).toHaveAttribute("aria-controls", panel!.id);
    expect(panel!).toHaveAttribute("aria-labelledby", httpTab.id);
  });

  it("unmounts HTTP controls while Socket is selected and exposes only Socket actions", async () => {
    const user = userEvent.setup();
    render(<RulesView />);
    await user.click(screen.getByRole("tab", { name: "Socket" }));
    expect(screen.getByLabelText("socket protocol rules mounted")).toBeVisible();
    expect(screen.getByRole("heading", { level: 2, name: "Socket 报文规则" })).toBeVisible();
    expect(screen.getByRole("button", { name: "新建报文规则" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "导入规则" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "导出规则" })).not.toBeInTheDocument();
    expect(screen.queryByText(/HTTP Header|Cookie|状态码|JSONPath|请求体|响应体/)).toBeNull();
    await user.click(screen.getByRole("tab", { name: "HTTP" }));
    expect(screen.getByRole("heading", { level: 2, name: "HTTP 拦截规则" })).toBeVisible();
    expect(screen.queryByLabelText("socket protocol rules mounted")).not.toBeInTheDocument();
  });

  it.each([
    ["HTTP", "{ArrowRight}", "Socket"],
    ["HTTP", "{End}", "Socket"],
    ["Socket", "{ArrowLeft}", "HTTP"],
    ["Socket", "{Home}", "HTTP"],
  ])("moves from %s with %s to %s", async (start, key, expected) => {
    const user = userEvent.setup();
    render(<RulesView />);
    if (start === "Socket") await user.click(screen.getByRole("tab", { name: "Socket" }));
    screen.getByRole("tab", { name: start }).focus();

    await user.keyboard(key);

    expect(screen.getByRole("tab", { name: expected })).toHaveAttribute("aria-selected", "true");
  });

  it.each(["{Enter}", " "])("activates the focused Socket tab with %s", async (key) => {
    const user = userEvent.setup();
    render(<RulesView />);
    const socketTab = screen.getByRole("tab", { name: "Socket" });
    socketTab.focus();

    await user.keyboard(key);

    expect(socketTab).toHaveAttribute("aria-selected", "true");
  });

  it("resets an unmounted Socket draft and ignores its late async result", async () => {
    const user = userEvent.setup();
    render(<RulesView />);
    await user.click(screen.getByRole("tab", { name: "Socket" }));
    await user.type(screen.getByRole("textbox", { name: "socket 草稿名称" }), "socket-only");
    await user.click(screen.getByRole("tab", { name: "HTTP" }));

    await new Promise((resolve) => window.setTimeout(resolve, 30));
    expect(screen.queryByText("Socket 异步结果")).not.toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Socket" }));
    expect(screen.getByRole("textbox", { name: "socket 草稿名称" })).toHaveValue("");
  });

  it("keeps both protocol workspaces responsive with a single-column narrow breakpoint", async () => {
    const user = userEvent.setup();
    render(<RulesView />);

    expect(screen.getByRole("heading", { level: 2, name: "HTTP 拦截规则" }).closest("section"))
      .toHaveClass("max-[1280px]:grid-cols-1");
    await user.click(screen.getByRole("tab", { name: "Socket" }));
    expect(screen.getByLabelText("socket protocol rules mounted")).toHaveClass("max-[1280px]:grid-cols-1");
  });
});
