// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
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
vi.mock("./socket-rules-view", () => ({
  SocketRulesView: () => <section aria-label="Socket rules mounted">Socket-only editor</section>,
}));

describe("HTTP and Socket controlled rule tabs", () => {
  it("mounts the established HTTP rule workspace by default", () => {
    render(<RulesView />);
    expect(screen.getByRole("tab", { name: "HTTP 规则" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("heading", { name: "拦截规则" })).toBeVisible();
    expect(screen.getByRole("button", { name: "新建规则" })).toBeEnabled();
    expect(screen.queryByLabelText("Socket rules mounted")).not.toBeInTheDocument();
  });

  it("unmounts HTTP state while Socket is selected and restores it on return", async () => {
    const user = userEvent.setup();
    render(<RulesView />);
    await user.click(screen.getByRole("tab", { name: "Socket 规则" }));
    expect(screen.getByLabelText("Socket rules mounted")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "拦截规则" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "HTTP 规则" }));
    expect(screen.getByRole("heading", { name: "拦截规则" })).toBeVisible();
    expect(screen.queryByLabelText("Socket rules mounted")).not.toBeInTheDocument();
  });
});
