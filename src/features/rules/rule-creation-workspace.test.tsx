// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ProxyListener,
  RuleDraft,
  RuleSummaryViewModel,
} from "@/generated/rust-types";
import {
  useWorkspaceNavigation,
  WorkspaceNavigationProvider,
} from "@/features/shell/workspace-navigation";
import { RulesView } from "./rules-view";

const commandMocks = vi.hoisted(() => ({
  ruleNewHttpDraft: vi.fn(),
}));
const sourceState = vi.hoisted(() => ({
  listeners: [] as ProxyListener[],
  channelCatalog: [] as Array<{ id: string; display_name: string }>,
  standardRules: [] as RuleSummaryViewModel[],
  loading: false,
  error: undefined as string | undefined,
}));

vi.mock("@/generated/rust-types", () => ({ commands: commandMocks }));
vi.mock("@/lib/ipc/client", () => ({
  appErrorViewModel: () => undefined,
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => reason instanceof Error ? reason.message : "Rust 操作失败",
}));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: vi.fn(),
  useBootstrap: () => ({ bootstrap: { channel_catalog: sourceState.channelCatalog } }),
}));
vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) => {
    const sourceQuery = key === "protocol-rule-workspaces"
      || key.startsWith("protocol-rule-workspace:")
      || key.startsWith("protocol-rule-list:");
    const base = {
      error: sourceQuery ? sourceState.error : undefined,
      isLoading: sourceQuery && sourceState.loading,
      refresh: vi.fn().mockResolvedValue(undefined),
      invalidate: vi.fn(),
    };
    if (key === "rule-list") return { ...base, data: sourceState.standardRules };
    if (key === "rule-capabilities") return { ...base, data: [] };
    if (key === "protocol-rule-workspaces") {
      return { ...base, data: [{ id: "workspace", selected: true }] };
    }
    if (key.startsWith("protocol-rule-workspace:")) {
      return { ...base, data: { listeners: sourceState.listeners } };
    }
    if (key.startsWith("protocol-rule-list:")) return { ...base, data: [] };
    return { ...base, data: undefined };
  },
}));
vi.mock("./protocol-rules-view", () => ({
  ProtocolRuleEditorView: ({ kind }: { kind: "http" | "socket" }) => (
    <output>{kind === "http" ? "Body 创建编辑器" : "Socket 创建编辑器"}</output>
  ),
}));
vi.mock("@/features/faults/faults-view", () => ({
  FaultPresetsView: () => <output>故障预设</output>,
}));

function httpListener(id: string, protocol: boolean): ProxyListener {
  return {
    id,
    name: id,
    enabled: true,
    bind_address: "127.0.0.1",
    port: 8080,
    connect_timeout_ms: 1_000,
    read_timeout_ms: 1_000,
    write_timeout_ms: 1_000,
    data_plane: {
      kind: "http",
      settings: {
        body_processing: protocol
          ? { mode: "protocol", package: { id: "pkg", version: "1.0.0" } }
          : { mode: "plain" },
      },
    },
  } as ProxyListener;
}

function socketListener(id: string, scripted: boolean): ProxyListener {
  return {
    ...httpListener(id, false),
    data_plane: {
      kind: "socket",
      settings: {
        processing: scripted
          ? { mode: "scripted", settings: { package: { id: "pkg", version: "1.0.0" } } }
          : { mode: "direct" },
      },
    },
  } as ProxyListener;
}

function NavigationProbe() {
  const { pathname, searchParams } = useWorkspaceNavigation();
  return <output aria-label="当前位置">{pathname}?{searchParams.toString()}</output>;
}

function renderWorkspace() {
  return render(
    <WorkspaceNavigationProvider>
      <NavigationProbe />
      <RulesView />
    </WorkspaceNavigationProvider>,
  );
}

async function openCreationDialog() {
  await userEvent.setup().click(screen.getByRole("button", { name: "新建规则" }));
}

describe("listener-bound rule creation with real workspace navigation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    sourceState.listeners = [];
    sourceState.channelCatalog = [];
    sourceState.standardRules = [];
    sourceState.loading = false;
    sourceState.error = undefined;
    commandMocks.ruleNewHttpDraft.mockImplementation(async (listenerId: string): Promise<RuleDraft> => ({
      rule_id: null,
      expected_revision: null,
      name: "新建 HTTP 规则",
      description: "",
      enabled: true,
      priority: 100,
      channel: listenerId,
      stage: "request",
      conditions: [],
      actions: [],
      one_shot: false,
    }));
  });

  it("removes blank creation and explains every unavailable type with zero listeners", async () => {
    renderWorkspace();
    await openCreationDialog();

    expect(screen.queryByText("空白规则")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^HTTP 规则/ })).toBeDisabled();
    expect(screen.getAllByText("当前 Workspace 没有 HTTP Listener；请先创建 HTTP 入口。")).toHaveLength(2);
    expect(screen.getByRole("button", { name: /Body 报文规则/ })).toBeDisabled();
    expect(screen.getByText(/没有启用协议 Body 处理的 HTTP Listener/)).toBeVisible();
    expect(screen.getByRole("button", { name: /Socket 报文规则/ })).toBeDisabled();
    expect(screen.getByText(/没有启用报文处理的 Socket Listener/)).toBeVisible();
  });

  it("allows a plain HTTP listener only for a bound standard HTTP rule", async () => {
    sourceState.listeners = [httpListener("http-plain", false)];
    sourceState.channelCatalog = [{ id: "http-plain", display_name: "HTTP Plain" }];
    renderWorkspace();
    await openCreationDialog();

    const http = screen.getByRole("button", { name: /^HTTP 规则/ });
    expect(http).toBeEnabled();
    expect(screen.getByRole("button", { name: /Body 报文规则/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Socket 报文规则/ })).toBeDisabled();
    await userEvent.setup().click(http);
    expect(commandMocks.ruleNewHttpDraft).toHaveBeenCalledWith("http-plain");
  });

  it("enables Body only for protocol HTTP and Socket only for scripted Socket", async () => {
    sourceState.listeners = [
      httpListener("http-protocol", true),
      socketListener("socket-raw", false),
      socketListener("socket-scripted", true),
    ];
    sourceState.channelCatalog = [{ id: "http-protocol", display_name: "HTTP Protocol" }];
    renderWorkspace();
    await openCreationDialog();

    expect(screen.getByRole("button", { name: /^HTTP 规则/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: /Body 报文规则/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: /Socket 报文规则/ })).toBeEnabled();
    await userEvent.setup().click(screen.getByRole("button", { name: /Socket 报文规则/ }));
    expect(screen.getByLabelText("当前位置")).toHaveTextContent("/rules?category=socket&create=rule");
    expect(screen.getByText("Socket 创建编辑器")).toBeVisible();
  });

  it("keeps Socket raw unavailable and accepts a compatible listener arriving after refresh", async () => {
    sourceState.listeners = [socketListener("socket-raw", false)];
    const rendered = renderWorkspace();
    await openCreationDialog();
    expect(screen.getByRole("button", { name: /Socket 报文规则/ })).toBeDisabled();

    sourceState.listeners = [socketListener("socket-scripted", true)];
    rendered.rerender(
      <WorkspaceNavigationProvider>
        <NavigationProbe />
        <RulesView />
      </WorkspaceNavigationProvider>,
    );
    const socket = screen.getByRole("button", { name: /Socket 报文规则/ });
    expect(socket).toBeEnabled();
    await userEvent.setup().click(socket);
    expect(screen.getByLabelText("当前位置")).toHaveTextContent("category=socket&create=rule");
  });

  it("does not route or create while listener facts are loading or failed", async () => {
    sourceState.loading = true;
    const rendered = renderWorkspace();
    await openCreationDialog();
    expect(screen.getAllByText("正在读取当前 Workspace 的入口配置。").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: /Body 报文规则/ })).toBeDisabled();
    expect(screen.getByLabelText("当前位置")).not.toHaveTextContent("create=rule");

    sourceState.loading = false;
    sourceState.error = "unavailable";
    rendered.rerender(
      <WorkspaceNavigationProvider>
        <NavigationProbe />
        <RulesView />
      </WorkspaceNavigationProvider>,
    );
    expect(screen.getAllByText("入口配置读取失败，请刷新后重试。").length).toBeGreaterThan(0);
    expect(commandMocks.ruleNewHttpDraft).not.toHaveBeenCalled();
  });

  it("does not let a late HTTP draft replace a rule selected afterwards", async () => {
    const pending = deferred<RuleDraft>();
    sourceState.listeners = [httpListener("http-plain", false)];
    sourceState.standardRules = [{
      rule_id: "existing-rule",
      revision: 3,
      name: "Existing Rule",
      enabled: true,
      priority: 10,
      creation_order: 1,
      channel_text: "HTTP Plain",
      stage_text: "请求",
      match_summary: "全部",
      action_summary: "延迟",
      hit_count: 0,
      last_hit_at: null,
      ui_tone: "positive",
    }];
    commandMocks.ruleNewHttpDraft.mockReturnValue(pending.promise);
    renderWorkspace();

    await openCreationDialog();
    await userEvent.setup().click(screen.getByRole("button", { name: /^HTTP 规则/ }));
    await userEvent.setup().click(screen.getByText("Existing Rule"));
    pending.resolve({
      rule_id: null,
      expected_revision: null,
      name: "Late New Draft",
      description: "",
      enabled: true,
      priority: 100,
      channel: "http-plain",
      stage: "request",
      conditions: [],
      actions: [],
      one_shot: false,
    });
    await Promise.resolve();

    expect(screen.queryByDisplayValue("Late New Draft")).not.toBeInTheDocument();
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}
