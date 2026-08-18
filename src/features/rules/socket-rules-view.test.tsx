// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ProxyListener,
  SocketDocumentRuleDefinition,
  SocketRuleCapabilityCatalog,
} from "@/generated/rust-types";
import { SocketRulesView } from "./socket-rules-view";

const commandMocks = vi.hoisted(() => ({
  workspaceList: vi.fn(),
  workspaceGet: vi.fn(),
  socketRuleList: vi.fn(),
  socketRuleCapabilities: vi.fn(),
  socketRuleSave: vi.fn(),
  socketRuleToggle: vi.fn(),
  socketRuleDelete: vi.fn(),
  socketRuleParseValue: vi.fn(),
}));
const queryState = vi.hoisted(() => ({
  listeners: [] as unknown[],
  rules: [] as unknown[],
  capabilityError: undefined as string | undefined,
  blockedSource: undefined as undefined | "workspaces" | "workspace" | "rules",
  blockedState: "loading" as "loading" | "error",
  capabilities: new Map<string, unknown>(),
  refresh: vi.fn(),
  eventRefresh: undefined as undefined | (() => Promise<void>),
}));

vi.mock("@/generated/rust-types", () => ({ commands: commandMocks }));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: (_events: unknown, refresh: () => Promise<void>) => { queryState.eventRefresh = refresh; },
}));
vi.mock("@/lib/ipc/client", () => ({
  appErrorViewModel: (reason: unknown) => reason && typeof reason === "object" ? reason : undefined,
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => reason instanceof Error ? reason.message : "Rust 操作失败",
}));
vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) => {
    let data: unknown;
    let error: string | undefined;
    const source = key === "socket-rule-workspaces" ? "workspaces" : key.startsWith("socket-rule-workspace:") ? "workspace" : key.startsWith("socket-rule-list:") ? "rules" : undefined;
    if (key === "socket-rule-workspaces") {
      data = [{ id: "workspace-1", name: "工作区", revision: 1, listener_count: queryState.listeners.length, enabled_listener_count: queryState.listeners.length, selected: true }];
    } else if (key.startsWith("socket-rule-workspace:")) {
      data = { id: "workspace-1", name: "工作区", revision: 1, listeners: queryState.listeners, metadata_extractors: [], response_assertions: [], fault_presets: [], certificate_references: [] };
    } else if (key.startsWith("socket-rule-list:")) {
      data = queryState.rules;
    } else if (key.startsWith("socket-rule-capabilities:")) {
      data = queryState.capabilities.get(key);
      error = queryState.capabilityError;
    }
    const blocked = source != null && source === queryState.blockedSource;
    if (blocked && queryState.blockedState === "error") error = "事实源不可用";
    return { data, error, isLoading: blocked && queryState.blockedState === "loading", refresh: queryState.refresh, invalidate: vi.fn() };
  },
}));

const packageRef = { id: "iso8583", version: "1.2.3" };

function socketListener(id: string, mode: "relay" | "local" | "direct" = "relay"): ProxyListener {
  return {
    id,
    name: id,
    enabled: true,
    bind_address: "127.0.0.1",
    port: 9000,
    allowed_client_cidrs: [],
    connect_timeout_ms: 1_000,
    read_timeout_ms: 1_000,
    write_timeout_ms: 1_000,
    data_plane: {
      kind: "socket",
      settings: {
        topology: mode === "local"
          ? { mode: "local_responder", settings: { downstream_security: { mode: "tcp" } } }
          : { mode: "relay", settings: { upstream: { host: "example.test", port: 9001 }, security: { mode: "transparent" } } },
        maximum_connections: 8,
        processing: mode === "direct" ? { mode: "direct" } : {
          mode: "scripted",
          settings: {
            package: packageRef,
            upstream: { decode_enabled: mode !== "local", encode_enabled: true },
            downstream: { decode_enabled: true, encode_enabled: true },
          },
        },
      },
    },
  };
}

function httpListener(): ProxyListener {
  return { ...socketListener("http"), data_plane: { kind: "http", settings: {} } } as unknown as ProxyListener;
}

function capability(direction: "upstream" | "downstream", schemaVersion = 7): SocketRuleCapabilityCatalog {
  return {
    package: packageRef,
    schema_version: schemaVersion,
    direction,
    fields: [
      { name: "message_type", label: "消息类型", type: "string", operators: ["equals"], actions: ["set_field"] },
      { name: "amount", label: "金额", type: "int", operators: ["equals"], actions: ["set_field"] },
    ],
    common_actions: ["record_match", "clear_document"],
  };
}

function rule(revision = 3): SocketDocumentRuleDefinition {
  return {
    rule_id: "rule-1",
    revision,
    name: "测试规则",
    enabled: true,
    priority: 10,
    created_order: 1,
    listener_id: "relay",
    package: packageRef,
    schema_version: 7,
    direction: "upstream",
    conditions: [{ operator: "equals", field: "message_type", value: { type: "string", value: "0200" } }],
    actions: [{ type: "record_match" }],
  };
}

function savedFromInput(input: Record<string, unknown>) {
  return { ...input, rule_id: input.rule_id ?? "rule-1", revision: input.expected_revision == null ? 1 : Number(input.expected_revision) + 1, created_order: 1 };
}

function deleted(ruleId = "rule-1", revision = 4) {
  return { success: true, cancelled: false, message: "deleted", ui_tone: "positive", entity_id: ruleId, revision, requires_restart: false };
}

function installCapabilities() {
  queryState.capabilities.set("socket-rule-capabilities:relay:upstream", capability("upstream"));
  queryState.capabilities.set("socket-rule-capabilities:relay:downstream", capability("downstream"));
  queryState.capabilities.set("socket-rule-capabilities:local:downstream", capability("downstream", 8));
}

describe("Socket rules view state and command contracts", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    queryState.listeners = [socketListener("relay"), socketListener("local", "local"), socketListener("direct", "direct"), httpListener()];
    queryState.rules = [];
    queryState.capabilityError = undefined;
    queryState.blockedSource = undefined;
    queryState.blockedState = "loading";
    queryState.capabilities.clear();
    installCapabilities();
    queryState.refresh.mockResolvedValue(undefined);
    queryState.eventRefresh = undefined;
    commandMocks.socketRuleParseValue.mockImplementation(async (type: string, raw: string) => {
      if (type === "string") return { type, value: raw };
      if (type === "int") return { type, value: Number(raw) };
      if (type === "bool") return { type, value: raw === "true" };
      return { type, value: [] };
    });
    commandMocks.socketRuleSave.mockImplementation(async (input: Record<string, unknown>) => savedFromInput(input));
  });

  it("lists only Scripted Socket listeners in the creation selector", async () => {
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建 Socket 规则" }));
    await user.click(screen.getByLabelText("Socket Listener"));
    expect(await screen.findByRole("option", { name: /relay/ })).toBeVisible();
    expect(screen.getByRole("option", { name: /local/ })).toBeVisible();
    expect(screen.queryByRole("option", { name: /direct/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /http/ })).not.toBeInTheDocument();
  });

  it("clears the prior draft when switching listeners so capabilities cannot leak", async () => {
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建 Socket 规则" }));
    await user.click(screen.getByRole("button", { name: "添加条件" }));
    const relayValue = screen.getByRole("textbox", { name: "比较值" });
    await user.type(relayValue, "0200");
    await user.click(screen.getByLabelText("Socket Listener"));
    await user.click(await screen.findByRole("option", { name: /local/ }));
    expect(screen.queryByLabelText("Socket 方向")).not.toBeInTheDocument();
    expect(screen.getByText("Schema v8")).toBeVisible();
    expect(screen.getByText("空条件恒匹配。")).toBeVisible();
    expect(screen.queryByDisplayValue("0200")).not.toBeInTheDocument();
  });

  it("clears the prior draft when switching Relay direction", async () => {
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建 Socket 规则" }));
    await user.click(screen.getByRole("button", { name: "添加条件" }));
    await user.type(screen.getByRole("textbox", { name: "比较值" }), "0200");
    await user.click(screen.getByLabelText("Socket 方向"));
    await user.click(await screen.findByRole("option", { name: "downstream" }));
    expect(screen.getByText("空条件恒匹配。")).toBeVisible();
    expect(screen.queryByDisplayValue("0200")).not.toBeInTheDocument();
  });

  it("serializes a double-click save into one Rust command", async () => {
    let finish!: (value: SocketDocumentRuleDefinition) => void;
    commandMocks.socketRuleSave.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建 Socket 规则" }));
    await user.dblClick(screen.getByRole("button", { name: "保存 Socket 规则" }));
    expect(commandMocks.socketRuleSave).toHaveBeenCalledOnce();
    finish(savedFromInput(commandMocks.socketRuleSave.mock.calls[0][0]) as SocketDocumentRuleDefinition);
    await waitFor(() => expect(screen.getByText("编辑 Socket 规则")).toBeVisible());
  });

  it("sends the exact binding, ordered AND conditions, and Clear-then-Set actions", async () => {
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建 Socket 规则" }));

    await user.click(screen.getByRole("button", { name: "添加条件" }));
    await user.type(screen.getByRole("textbox", { name: "比较值" }), "0200");
    await user.click(screen.getByRole("button", { name: "添加条件" }));
    const conditionValues = screen.getAllByRole("textbox", { name: "比较值" });
    await user.clear(conditionValues[1]);
    await user.type(conditionValues[1], "100");

    await user.click(screen.getByRole("button", { name: "添加 ClearDocument" }));
    await user.click(screen.getByRole("button", { name: "添加 SetField" }));
    await user.type(screen.getByRole("textbox", { name: "设置值" }), "0210");
    await user.click(screen.getByRole("button", { name: "删除动作 1" }));
    await user.click(screen.getByRole("button", { name: "保存 Socket 规则" }));

    expect(commandMocks.socketRuleSave).toHaveBeenCalledWith({
      rule_id: null,
      expected_revision: null,
      enabled: true,
      priority: 100,
      listener_id: "relay",
      package: packageRef,
      schema_version: 7,
      direction: "upstream",
      conditions: [
        { operator: "equals", field: "message_type", value: { type: "string", value: "0200" } },
        { operator: "equals", field: "amount", value: { type: "int", value: 100 } },
      ],
      actions: [
        { type: "clear_document" },
        { type: "set_field", field: "message_type", value: { type: "string", value: "0210" } },
      ],
    });
  });

  it("keeps the edited draft visible when Rust returns field errors", async () => {
    commandMocks.socketRuleSave.mockRejectedValue({ field_errors: { actions: ["SetField 被后端拒绝"] } });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建 Socket 规则" }));
    await user.click(screen.getByRole("button", { name: "添加条件" }));
    await user.type(screen.getByRole("textbox", { name: "比较值" }), "0200");
    await user.click(screen.getByRole("button", { name: "保存 Socket 规则" }));
    expect(await screen.findByText("SetField 被后端拒绝")).toBeVisible();
    expect(screen.getByDisplayValue("0200")).toBeVisible();
  });

  it("keeps a conflicted draft until the user reloads the latest revision", async () => {
    queryState.rules = [rule(3)];
    commandMocks.socketRuleSave.mockRejectedValue({ field_errors: { expected_revision: ["规则已被其他窗口更新"] } });
    commandMocks.socketRuleList.mockResolvedValue([{ ...rule(4), priority: 99, conditions: [] }]);
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    const input = screen.getByRole("textbox", { name: "比较值" });
    await user.clear(input);
    await user.type(input, "0210");
    await user.click(screen.getByRole("button", { name: "保存 Socket 规则" }));
    expect(await screen.findByText("规则已被其他窗口更新")).toBeVisible();
    expect(screen.getByDisplayValue("0210")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "重新加载当前规则" }));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "优先级" })).toHaveValue("99"));
    expect(screen.getByText("空条件恒匹配。")).toBeVisible();
  });

  it("passes the current revision when toggling a rule", async () => {
    queryState.rules = [rule(3)];
    commandMocks.socketRuleToggle.mockResolvedValue({ ...rule(4), enabled: false });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("switch", { name: "停用 Socket 规则 rule-1" }));
    expect(commandMocks.socketRuleToggle).toHaveBeenCalledWith("rule-1", 3, false);
  });

  it("rejects a malformed toggle response and releases the mutation lock", async () => {
    queryState.rules = [rule(3)];
    commandMocks.socketRuleToggle
      .mockResolvedValueOnce({ rule_id: "broken" })
      .mockResolvedValueOnce({ ...rule(4), enabled: false });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    const toggle = screen.getByRole("switch", { name: "停用 Socket 规则 rule-1" });
    await user.click(toggle);
    await waitFor(() => expect(toggle).toBeEnabled());
    await user.click(toggle);
    expect(commandMocks.socketRuleToggle).toHaveBeenCalledTimes(2);
  });

  it("rejects a toggle response owned by another rule without refreshing", async () => {
    queryState.rules = [rule(3)];
    commandMocks.socketRuleToggle.mockResolvedValue({ ...rule(4), rule_id: "rule-other", enabled: false });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("switch", { name: "停用 Socket 规则 rule-1" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "新建 Socket 规则" })).toBeEnabled());
    expect(queryState.refresh).not.toHaveBeenCalled();
  });

  it("passes the current revision and explicit confirmation when deleting", async () => {
    queryState.rules = [rule(3)];
    commandMocks.socketRuleDelete.mockResolvedValue(deleted());
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    await user.click(screen.getByRole("button", { name: "删除规则" }));
    expect(commandMocks.socketRuleDelete).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "确认删除" }));
    expect(commandMocks.socketRuleDelete).toHaveBeenCalledWith("rule-1", 3, true);
  });

  it("presents a capability error and retries the capability query", async () => {
    queryState.capabilityError = "Schema 能力不可用";
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建 Socket 规则" }));
    expect(screen.getByText("Schema 能力不可用")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "重试" }));
    expect(queryState.refresh).toHaveBeenCalled();
  });

  it("rejects a non-array rule list payload", () => {
    queryState.rules = null as unknown as unknown[];
    render(<SocketRulesView />);
    expect(screen.getByText("Socket 规则列表包含无效数据，已拒绝显示。")).toBeVisible();
  });

  it("rejects malformed entries in a rule list payload", () => {
    queryState.rules = [rule(), { rule_id: "broken" }];
    render(<SocketRulesView />);
    expect(screen.getByText("Socket 规则列表包含无效数据，已拒绝显示。")).toBeVisible();
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  });

  it("rejects a duplicate rule id list as one invalid payload", () => {
    queryState.rules = [rule(3), { ...rule(4), priority: 99 }];
    render(<SocketRulesView />);
    expect(screen.getByText("Socket 规则列表包含无效数据，已拒绝显示。")).toBeVisible();
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  });

  it("rejects a malformed save response and keeps the draft", async () => {
    commandMocks.socketRuleSave.mockResolvedValue({ rule_id: "broken" });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建 Socket 规则" }));
    await user.click(screen.getByRole("button", { name: "保存 Socket 规则" }));
    expect(await screen.findByText("Socket 规则保存响应无效。")).toBeVisible();
    expect(screen.getByRole("heading", { name: "新建 Socket 规则" })).toBeVisible();
  });

  it("rejects a save response with a different binding identity", async () => {
    commandMocks.socketRuleSave.mockResolvedValue({
      ...rule(), listener_id: "local", schema_version: 8, direction: "downstream",
    });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建 Socket 规则" }));
    await user.click(screen.getByRole("button", { name: "保存 Socket 规则" }));
    expect(await screen.findByText("Socket 规则保存响应无效。")).toBeVisible();
    expect(screen.getByRole("heading", { name: "新建 Socket 规则" })).toBeVisible();
  });

  it("rejects a malformed reload list and keeps the selected draft", async () => {
    queryState.rules = [rule(3)];
    commandMocks.socketRuleList.mockResolvedValue([{ rule_id: "broken" }]);
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    await user.click(screen.getByRole("button", { name: "重新加载当前规则" }));
    expect(await screen.findByText("invalid socket rule list response")).toBeVisible();
    expect(screen.getByDisplayValue("0200")).toBeVisible();
  });

  it("rejects an unknown capability enum before rendering an editor", async () => {
    queryState.capabilities.set("socket-rule-capabilities:relay:upstream", {
      ...capability("upstream"),
      fields: [{ name: "amount", label: "金额", type: "decimal", operators: ["equals"], actions: ["set_field"] }],
    });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建 Socket 规则" }));
    expect(screen.getByText(/规则能力.*(?:未知字段类型|精确包版本或方向不一致)/)).toBeVisible();
    expect(screen.queryByRole("button", { name: "保存 Socket 规则" })).not.toBeInTheDocument();
  });

  it("keeps a saved draft when deletion fails", async () => {
    queryState.rules = [rule(3)];
    commandMocks.socketRuleDelete.mockRejectedValue({ field_errors: { expected_revision: ["删除版本冲突"] } });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    await user.click(screen.getByRole("button", { name: "删除规则" }));
    await user.click(screen.getByRole("button", { name: "确认删除" }));
    expect(await screen.findByText("删除版本冲突")).toBeVisible();
    expect(screen.getByText("编辑 Socket 规则")).toBeVisible();
  });

  it("restores focus to the editor region after confirmed deletion", async () => {
    queryState.rules = [rule(3)];
    commandMocks.socketRuleDelete.mockResolvedValue(deleted());
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    await user.click(screen.getByRole("button", { name: "删除规则" }));
    await user.click(screen.getByRole("button", { name: "确认删除" }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("region", { name: "Socket 规则编辑区" })));
    expect(screen.getByText("选择一条规则或新建规则进行编辑。")).toBeVisible();
  });

  it("keeps the rule selected when a delete response is not an exact success", async () => {
    queryState.rules = [rule(3)];
    commandMocks.socketRuleDelete.mockResolvedValue({ ...deleted(), entity_id: "other" });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    await user.click(screen.getByRole("button", { name: "删除规则" }));
    await user.click(screen.getByRole("button", { name: "确认删除" }));
    expect(await screen.findByText("Socket 规则删除响应无效。")).toBeVisible();
    expect(screen.getByText("编辑 Socket 规则")).toBeVisible();
  });

  it("clears only an incompatible creation draft after an external capability refresh", async () => {
    let finish!: (value: SocketDocumentRuleDefinition) => void;
    commandMocks.socketRuleSave.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    const user = userEvent.setup();
    const view = render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建 Socket 规则" }));
    await user.click(screen.getByRole("button", { name: "添加条件" }));
    await user.type(screen.getByRole("textbox", { name: "比较值" }), "0200");
    await user.click(screen.getByRole("button", { name: "保存 Socket 规则" }));
    const staleResponse = savedFromInput(commandMocks.socketRuleSave.mock.calls[0][0]) as SocketDocumentRuleDefinition;
    const changedListener = socketListener("relay");
    if (changedListener.data_plane.kind === "socket" && changedListener.data_plane.settings.processing?.mode === "scripted") {
      changedListener.data_plane.settings.processing.settings.package = { id: "iso8583", version: "2.0.0" };
    }
    queryState.listeners = [changedListener];
    queryState.capabilities.set("socket-rule-capabilities:relay:upstream", { ...capability("upstream", 8), package: { id: "iso8583", version: "2.0.0" } });
    await act(async () => { await queryState.eventRefresh?.(); });
    view.rerender(<SocketRulesView />);
    const refreshCount = queryState.refresh.mock.calls.length;
    finish(staleResponse);
    expect(await screen.findByText("Schema v8")).toBeVisible();
    await waitFor(() => expect(screen.queryByDisplayValue("0200")).not.toBeInTheDocument());
    expect(screen.getByText("空条件恒匹配。")).toBeVisible();
    expect(queryState.refresh).toHaveBeenCalledTimes(refreshCount);
  });

  it("blocks toggle, reload, and delete while a field parser is pending", async () => {
    commandMocks.socketRuleParseValue.mockReturnValue(new Promise(() => {}));
    queryState.rules = [rule(3)];
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    await user.type(screen.getByRole("textbox", { name: "比较值" }), "1");
    expect(screen.getByRole("switch", { name: "停用 Socket 规则 rule-1" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "重新加载当前规则" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "删除规则" })).toBeDisabled();
  });

  it.each(["workspaces", "workspace", "rules"] as const)("hides cached rules and editor actions while %s is loading or failed", async (source) => {
    queryState.rules = [rule(3)];
    const user = userEvent.setup();
    const view = render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    for (const state of ["loading", "error"] as const) {
      queryState.blockedSource = source;
      queryState.blockedState = state;
      view.rerender(<SocketRulesView />);
      expect(screen.queryAllByRole("listitem")).toHaveLength(0);
      expect(screen.getByRole("button", { name: "新建 Socket 规则" })).toBeDisabled();
      for (const name of ["保存 Socket 规则", "删除规则", "重新加载当前规则"]) expect(screen.queryByRole("button", { name })).not.toBeInTheDocument();
    }
  });
});
