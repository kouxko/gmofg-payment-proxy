// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { useEffect, useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RulesView } from "./rules-view";

const navigationMocks = vi.hoisted(() => ({
  navigate: vi.fn(),
  searchParams: new URLSearchParams(),
}));

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
  useWorkspaceNavigation: () => ({ pathname: "/rules", searchParams: navigationMocks.searchParams, navigate: navigationMocks.navigate }),
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
  ProtocolRuleEditorView: ({ kind }: { kind: "http" | "socket" }) => (
    <section aria-label={`${kind} protocol editor mounted`}>
      <h2>{kind === "http" ? "HTTP Body 编辑器" : "Socket 编辑器"}</h2>
    </section>
  ),
}));
vi.mock("@/features/faults/faults-view", () => ({
  FaultPresetsView: () => (
    <section aria-label="HTTP 故障预设已挂载">
      <h2>HTTP 故障预设</h2>
    </section>
  ),
}));

describe("unified HTTP and Socket rule workspace", () => {
  beforeEach(() => {
    navigationMocks.navigate.mockClear();
    navigationMocks.searchParams = new URLSearchParams();
  });

  it("shows one rule workspace instead of separate HTTP and Socket areas", () => {
    render(<RulesView />);
    expect(screen.queryByRole("heading", { level: 1, name: "规则" })).toBeNull();
    expect(screen.getByRole("heading", { level: 2, name: "规则" })).toBeVisible();
    expect(screen.queryByRole("heading", { level: 2, name: "HTTP 规则" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { level: 2, name: "Socket 报文规则" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("socket protocol rules mounted")).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "HTTP" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Socket" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "新建规则" })).toBeEnabled();
    expect(screen.getAllByRole("button", { name: "新建规则" })).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "导入规则" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "导出规则" })).not.toBeInTheDocument();
    expect(
      screen.getByText("暂无规则，请选择新建规则开始配置"),
    ).toBeVisible();
  });

  it("uses the new-rule dialog as the only HTTP rule-type chooser", async () => {
    const user = userEvent.setup();
    render(<RulesView />);

    expect(screen.queryByRole("tab", { name: "常规规则" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Body 报文规则" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "新建规则" }));
    expect(screen.getByRole("button", { name: /空白规则/ })).toHaveAttribute("slot", "close");
    expect(screen.getByRole("button", { name: /Body 报文规则/ })).toHaveAttribute("slot", "close");
    expect(screen.getByRole("button", { name: /Socket 报文规则/ })).toHaveAttribute("slot", "close");
    expect(screen.getByRole("button", { name: /从故障预设创建/ })).toHaveAttribute("slot", "close");
  });

  it("offers blank, Body and fault-preset creation from the regular new-rule action", async () => {
    const user = userEvent.setup();
    render(<RulesView />);

    await user.click(screen.getByRole("button", { name: "新建规则" }));
    expect(screen.getByRole("button", { name: /空白规则/ })).toBeVisible();
    expect(screen.getByRole("button", { name: /Body 报文规则/ })).toBeVisible();
    await user.click(screen.getByRole("button", { name: /从故障预设创建/ }));
    expect(screen.getByLabelText("HTTP 故障预设已挂载")).toBeVisible();

    expect(screen.queryByRole("tab", { name: "故障预设" })).not.toBeInTheDocument();
  });

  it("returns to the standard workspace when blank creation starts from a protocol editor", async () => {
    const user = userEvent.setup();
    navigationMocks.searchParams = new URLSearchParams("category=socket&ruleId=socket-1");
    render(<RulesView />);

    await user.click(screen.getByRole("button", { name: "新建规则" }));
    await user.click(screen.getByRole("button", { name: /空白规则/ }));

    expect(navigationMocks.navigate).toHaveBeenCalledWith("/rules");
  });

  it("routes Body creation to the Body workspace in create mode", async () => {
    const user = userEvent.setup();
    render(<RulesView />);

    await user.click(screen.getByRole("button", { name: "新建规则" }));
    await user.click(screen.getByRole("button", { name: /Body 报文规则/ }));

    expect(navigationMocks.navigate).toHaveBeenCalledWith(
      "/rules?category=body&create=rule",
    );
  });

  it("routes Socket creation to the same workspace in create mode", async () => {
    const user = userEvent.setup();
    render(<RulesView />);

    await user.click(screen.getByRole("button", { name: "新建规则" }));
    await user.click(screen.getByRole("button", { name: /Socket 报文规则/ }));

    expect(navigationMocks.navigate).toHaveBeenCalledWith(
      "/rules?category=socket&create=rule",
    );
    expect(screen.queryByRole("button", { name: /Socket 报文规则/ }))
      .not.toBeInTheDocument();

    expect(screen.queryByText("选择规则创建方式")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "新建规则" }));
    expect(screen.getByRole("button", { name: /Socket 报文规则/ })).toBeVisible();
  });

  it("opens the Socket editor in the shared right-hand editor area", () => {
    navigationMocks.searchParams = new URLSearchParams("category=socket&ruleId=socket-1");
    render(<RulesView />);

    expect(screen.getByLabelText("socket protocol editor mounted")).toBeVisible();
    expect(screen.queryByLabelText("socket protocol rules mounted")).not.toBeInTheDocument();
  });

  it("uses one scrollable workspace instead of a protocol tablist", () => {
    render(<RulesView />);
    expect(screen.queryByRole("tablist", { name: "规则协议" })).not.toBeInTheDocument();
    expect(screen.getByLabelText("统一规则工作区")).toHaveClass("overflow-auto");
  });

  it("keeps the unified workspace responsive with a single-column narrow breakpoint", () => {
    render(<RulesView />);

    expect(screen.getByRole("heading", { level: 2, name: "规则" }).closest("section"))
      .toHaveClass(
        "grid-cols-[minmax(600px,1fr)_560px]",
        "max-[1280px]:grid-cols-1",
      );
  });
});
