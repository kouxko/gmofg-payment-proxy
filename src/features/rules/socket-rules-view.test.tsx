// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProtocolDocumentRuleDefinition } from "@/generated/rust-types";
import { ProtocolRulesView, SocketRulesView } from "./socket-rules-view";
import {
  capability,
  deleted,
  httpListener,
  packageRef,
  rule,
  savedFromInput,
  socketListener,
} from "./socket-rules-view.test-support";

const commandMocks = vi.hoisted(() => ({
  workspaceList: vi.fn(),
  workspaceGet: vi.fn(),
  protocolRuleList: vi.fn(),
  protocolRuleCapabilities: vi.fn(),
  protocolRuleSave: vi.fn(),
  protocolRuleToggle: vi.fn(),
  protocolRuleDelete: vi.fn(),
  protocolRuleParseValue: vi.fn(),
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
    const source = key === "protocol-rule-workspaces" ? "workspaces" : key.startsWith("protocol-rule-workspace:") ? "workspace" : key.startsWith("protocol-rule-list:") ? "rules" : undefined;
    if (key === "protocol-rule-workspaces") {
      data = [{ id: "workspace-1", name: "工作区", revision: 1, listener_count: queryState.listeners.length, enabled_listener_count: queryState.listeners.length, selected: true }];
    } else if (key.startsWith("protocol-rule-workspace:")) {
      data = { id: "workspace-1", name: "工作区", revision: 1, listeners: queryState.listeners, metadata_extractors: [], response_assertions: [], fault_presets: [], certificate_references: [] };
    } else if (key.startsWith("protocol-rule-list:")) {
      data = queryState.rules;
    } else if (key.startsWith("protocol-rule-capabilities:")) {
      data = queryState.capabilities.get(key);
      error = queryState.capabilityError;
    }
    const blocked = source != null && source === queryState.blockedSource;
    if (blocked && queryState.blockedState === "error") error = "事实源不可用";
    return { data, error, isLoading: blocked && queryState.blockedState === "loading", refresh: queryState.refresh, invalidate: vi.fn() };
  },
}));

function installCapabilities() {
  for (const stage of ["app_to_proxy", "proxy_to_upstream", "upstream_to_proxy", "proxy_to_app"] as const) {
    queryState.capabilities.set(`protocol-rule-capabilities:relay:${stage}`, capability(stage));
  }
  queryState.capabilities.set("protocol-rule-capabilities:local:app_to_proxy", capability("app_to_proxy", 8));
  queryState.capabilities.set("protocol-rule-capabilities:local:proxy_to_app", capability("proxy_to_app", 8));
  for (const stage of ["app_to_proxy", "proxy_to_upstream", "upstream_to_proxy", "proxy_to_app"] as const) {
    queryState.capabilities.set(`protocol-rule-capabilities:http:${stage}`, capability(stage));
  }
}

describe("Socket rules view state and command contracts", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    queryState.listeners = [socketListener("relay"), socketListener("local", "local"), socketListener("direct", "direct"), httpListener(), httpListener("plain-http", false)];
    queryState.rules = [];
    queryState.capabilityError = undefined;
    queryState.blockedSource = undefined;
    queryState.blockedState = "loading";
    queryState.capabilities.clear();
    installCapabilities();
    queryState.refresh.mockResolvedValue(undefined);
    queryState.eventRefresh = undefined;
    commandMocks.protocolRuleParseValue.mockImplementation(async (type: string, raw: string) => {
      if (type === "string") return { type, value: raw };
      if (type === "int") return { type, value: Number(raw) };
      if (type === "bool") return { type, value: raw === "true" };
      return { type, value: [] };
    });
    commandMocks.protocolRuleSave.mockImplementation(async (input: Record<string, unknown>) => savedFromInput(input));
  });

  it("lists only scripted Socket entries in the Socket workspace", async () => {
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    await user.click(screen.getByLabelText("协议入口"));
    expect(await screen.findByRole("option", { name: /relay.*Socket/ })).toBeVisible();
    expect(screen.getByRole("option", { name: /local.*本机应答/ })).toBeVisible();
    expect(screen.queryByRole("option", { name: /http.*HTTP/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /direct/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /plain-http/ })).not.toBeInTheDocument();
  });

  it("lists only HTTP protocol entries in the HTTP Body workspace", async () => {
    const user = userEvent.setup();
    render(<ProtocolRulesView kind="http" />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    await user.click(screen.getByLabelText("协议入口"));
    expect(await screen.findByRole("option", { name: /http.*HTTP/ })).toBeVisible();
    expect(screen.queryByRole("option", { name: /relay.*Socket/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /local.*本机应答/ })).not.toBeInTheDocument();
  });

  it("uses the shared rule workspace columns for protocol rules", () => {
    render(<SocketRulesView />);

    expect(screen.getByRole("heading", { name: "Socket 报文规则" }).closest("section")?.parentElement)
      .toHaveClass(
        "grid-cols-[minmax(600px,1fr)_560px]",
        "max-[1280px]:grid-cols-1",
      );
  });

  it("isolates mixed HTTP and Socket rules by their owning entry", () => {
    queryState.rules = [
      { ...rule(), rule_id: "socket-rule", name: "Socket 金额规则" },
      { ...rule(), rule_id: "http-rule", name: "HTTP 金额规则", listener_id: "http", created_order: 2 },
    ];

    const socketView = render(<SocketRulesView />);
    expect(screen.getByText("Socket 金额规则")).toBeVisible();
    expect(screen.queryByText("HTTP 金额规则")).not.toBeInTheDocument();

    socketView.unmount();
    render(<ProtocolRulesView kind="http" />);
    expect(screen.getByText("HTTP 金额规则")).toBeVisible();
    expect(screen.queryByText("Socket 金额规则")).not.toBeInTheDocument();
  });

  it("saves an HTTP protocol rule through the existing protocolRuleSave facade", async () => {
    const user = userEvent.setup();
    render(<ProtocolRulesView kind="http" />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "保存报文规则" })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: "保存报文规则" }));
    expect(commandMocks.protocolRuleSave).toHaveBeenCalledWith(expect.objectContaining({
      listener_id: "http",
      package: packageRef,
      schema_version: 7,
      stage: "app_to_proxy",
      actions: [{ type: "record_match" }],
    }));
  });

  it("clears the prior draft when switching listeners so capabilities cannot leak", async () => {
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    await user.click(screen.getByRole("button", { name: "添加条件" }));
    const relayValue = screen.getByRole("textbox", { name: "比较值" });
    await user.type(relayValue, "0200");
    await user.click(screen.getByLabelText("协议入口"));
    await user.click(await screen.findByRole("option", { name: /local/ }));
    expect(screen.getByLabelText("报文处理阶段")).toBeVisible();
    expect(screen.getByText("字段结构 v8")).toBeVisible();
    expect(screen.getByText("空条件恒匹配。")).toBeVisible();
    expect(screen.queryByDisplayValue("0200")).not.toBeInTheDocument();
  });

  it("clears the prior draft when switching relay processing stage", async () => {
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    await user.click(screen.getByRole("button", { name: "添加条件" }));
    await user.type(screen.getByRole("textbox", { name: "比较值" }), "0200");
    await user.click(screen.getByLabelText("报文处理阶段"));
    await user.click(await screen.findByRole("option", { name: "代理 → 上游服务" }));
    expect(screen.getByText("空条件恒匹配。")).toBeVisible();
    expect(screen.queryByDisplayValue("0200")).not.toBeInTheDocument();
  });

  it("serializes a double-click save into one Rust command", async () => {
    let finish!: (value: ProtocolDocumentRuleDefinition) => void;
    commandMocks.protocolRuleSave.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    await user.dblClick(screen.getByRole("button", { name: "保存报文规则" }));
    expect(commandMocks.protocolRuleSave).toHaveBeenCalledOnce();
    finish(savedFromInput(commandMocks.protocolRuleSave.mock.calls[0][0]) as ProtocolDocumentRuleDefinition);
    await waitFor(() => expect(screen.getByText("编辑规则")).toBeVisible());
  });

  it("sends the exact binding, ordered AND conditions, and Clear-then-Set actions", async () => {
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));

    await user.click(screen.getByRole("button", { name: "添加条件" }));
    await user.type(screen.getByRole("textbox", { name: "比较值" }), "0200");
    await user.click(screen.getByRole("button", { name: "添加条件" }));
    const conditionValues = screen.getAllByRole("textbox", { name: "比较值" });
    await user.clear(conditionValues[1]);
    await user.type(conditionValues[1], "100");

    await user.click(screen.getByRole("button", { name: "添加：清空全部字段" }));
    await user.click(screen.getByRole("button", { name: "添加：设置字段" }));
    await user.type(screen.getByRole("textbox", { name: "设置值" }), "0210");
    await user.click(screen.getByRole("button", { name: "删除动作 1" }));
    await user.click(screen.getByRole("button", { name: "保存报文规则" }));

    expect(commandMocks.protocolRuleSave).toHaveBeenCalledWith({
      rule_id: null,
      expected_revision: null,
      name: "新规则",
      enabled: true,
      priority: 100,
      listener_id: "relay",
      package: packageRef,
      schema_version: 7,
      stage: "app_to_proxy",
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
    commandMocks.protocolRuleSave.mockRejectedValue({ field_errors: { actions: ["SetField 被后端拒绝"] } });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    await user.click(screen.getByRole("button", { name: "添加条件" }));
    await user.type(screen.getByRole("textbox", { name: "比较值" }), "0200");
    await user.click(screen.getByRole("button", { name: "保存报文规则" }));
    expect(await screen.findByText("SetField 被后端拒绝")).toBeVisible();
    expect(screen.getByDisplayValue("0200")).toBeVisible();
  });

  it("keeps a conflicted draft until the user reloads the latest revision", async () => {
    queryState.rules = [rule(3)];
    commandMocks.protocolRuleSave.mockRejectedValue({ field_errors: { expected_revision: ["规则已被其他窗口更新"] } });
    commandMocks.protocolRuleList.mockResolvedValue([{ ...rule(4), priority: 99, conditions: [] }]);
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    const input = screen.getByRole("textbox", { name: "比较值" });
    await user.clear(input);
    await user.type(input, "0210");
    await user.click(screen.getByRole("button", { name: "保存报文规则" }));
    expect(await screen.findByText("规则已被其他窗口更新")).toBeVisible();
    expect(screen.getByDisplayValue("0210")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "重新加载当前规则" }));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "优先级" })).toHaveValue("99"));
    expect(screen.getByText("空条件恒匹配。")).toBeVisible();
  });

  it("passes the current revision when toggling a rule", async () => {
    queryState.rules = [rule(3)];
    commandMocks.protocolRuleToggle.mockResolvedValue({ ...rule(4), enabled: false });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("switch", { name: "停用报文规则 rule-1" }));
    expect(commandMocks.protocolRuleToggle).toHaveBeenCalledWith("rule-1", 3, false);
  });

  it("rejects a malformed toggle response and releases the mutation lock", async () => {
    queryState.rules = [rule(3)];
    commandMocks.protocolRuleToggle
      .mockResolvedValueOnce({ rule_id: "broken" })
      .mockResolvedValueOnce({ ...rule(4), enabled: false });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    const toggle = screen.getByRole("switch", { name: "停用报文规则 rule-1" });
    await user.click(toggle);
    await waitFor(() => expect(toggle).toBeEnabled());
    await user.click(toggle);
    expect(commandMocks.protocolRuleToggle).toHaveBeenCalledTimes(2);
  });

  it("rejects a toggle response owned by another rule without refreshing", async () => {
    queryState.rules = [rule(3)];
    commandMocks.protocolRuleToggle.mockResolvedValue({ ...rule(4), rule_id: "rule-other", enabled: false });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("switch", { name: "停用报文规则 rule-1" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "新建报文规则" })).toBeEnabled());
    expect(queryState.refresh).not.toHaveBeenCalled();
  });

  it("passes the current revision and explicit confirmation when deleting", async () => {
    queryState.rules = [rule(3)];
    commandMocks.protocolRuleDelete.mockResolvedValue(deleted());
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    await user.click(screen.getByRole("button", { name: "删除规则" }));
    expect(commandMocks.protocolRuleDelete).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "确认删除" }));
    expect(commandMocks.protocolRuleDelete).toHaveBeenCalledWith("rule-1", 3, true);
  });

  it("presents a capability error and retries the capability query", async () => {
    queryState.capabilityError = "Schema 能力不可用";
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    expect(screen.getByText("Schema 能力不可用")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "重试" }));
    expect(queryState.refresh).toHaveBeenCalled();
  });

  it("rejects a non-array rule list payload", () => {
    queryState.rules = null as unknown as unknown[];
    render(<SocketRulesView />);
    expect(screen.getByText("报文规则列表包含无效数据，已拒绝显示。")).toBeVisible();
  });

  it("rejects malformed entries in a rule list payload", () => {
    queryState.rules = [rule(), { rule_id: "broken" }];
    render(<SocketRulesView />);
    expect(screen.getByText("报文规则列表包含无效数据，已拒绝显示。")).toBeVisible();
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  });

  it("rejects a duplicate rule id list as one invalid payload", () => {
    queryState.rules = [rule(3), { ...rule(4), priority: 99 }];
    render(<SocketRulesView />);
    expect(screen.getByText("报文规则列表包含无效数据，已拒绝显示。")).toBeVisible();
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  });

  it("rejects a malformed save response and keeps the draft", async () => {
    commandMocks.protocolRuleSave.mockResolvedValue({ rule_id: "broken" });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    await user.click(screen.getByRole("button", { name: "保存报文规则" }));
    expect(await screen.findByText("报文规则保存响应无效。")).toBeVisible();
    expect(screen.getByRole("heading", { name: "新建规则" })).toBeVisible();
  });

  it("rejects a save response with a different binding identity", async () => {
    commandMocks.protocolRuleSave.mockResolvedValue({
      ...rule(), listener_id: "local", schema_version: 8, stage: "proxy_to_app",
    });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    await user.click(screen.getByRole("button", { name: "保存报文规则" }));
    expect(await screen.findByText("报文规则保存响应无效。")).toBeVisible();
    expect(screen.getByRole("heading", { name: "新建规则" })).toBeVisible();
  });

  it("rejects a malformed reload list and keeps the selected draft", async () => {
    queryState.rules = [rule(3)];
    commandMocks.protocolRuleList.mockResolvedValue([{ rule_id: "broken" }]);
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    await user.click(screen.getByRole("button", { name: "重新加载当前规则" }));
    expect(await screen.findByText("协议规则列表响应无效")).toBeVisible();
    expect(screen.getByDisplayValue("0200")).toBeVisible();
  });

  it("rejects an unknown capability enum before rendering an editor", async () => {
    queryState.capabilities.set("protocol-rule-capabilities:relay:app_to_proxy", {
      ...capability("app_to_proxy"),
      fields: [{ name: "amount", label: "金额", type: "decimal", operators: ["equals"], actions: ["set_field"] }],
    });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    expect(screen.getByText(/规则能力.*(?:未知字段类型|精确包版本或方向不一致)/)).toBeVisible();
    expect(screen.queryByRole("button", { name: "保存报文规则" })).not.toBeInTheDocument();
  });

  it("keeps a saved draft when deletion fails", async () => {
    queryState.rules = [rule(3)];
    commandMocks.protocolRuleDelete.mockRejectedValue({ field_errors: { expected_revision: ["删除版本冲突"] } });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    await user.click(screen.getByRole("button", { name: "删除规则" }));
    await user.click(screen.getByRole("button", { name: "确认删除" }));
    expect(await screen.findByText("删除版本冲突")).toBeVisible();
    expect(screen.getByText("编辑规则")).toBeVisible();
  });

  it("restores focus to the editor region after confirmed deletion", async () => {
    queryState.rules = [rule(3)];
    commandMocks.protocolRuleDelete.mockResolvedValue(deleted());
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    await user.click(screen.getByRole("button", { name: "删除规则" }));
    await user.click(screen.getByRole("button", { name: "确认删除" }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("region", { name: "报文规则编辑区" })));
    expect(screen.getByText("选择一条规则或新建规则进行编辑。")).toBeVisible();
  });

  it("keeps the rule selected when a delete response is not an exact success", async () => {
    queryState.rules = [rule(3)];
    commandMocks.protocolRuleDelete.mockResolvedValue({ ...deleted(), entity_id: "other" });
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    await user.click(screen.getByRole("button", { name: "删除规则" }));
    await user.click(screen.getByRole("button", { name: "确认删除" }));
    expect(await screen.findByText("报文规则删除响应无效。")).toBeVisible();
    expect(screen.getByText("编辑规则")).toBeVisible();
  });

  it("clears only an incompatible creation draft after an external capability refresh", async () => {
    let finish!: (value: ProtocolDocumentRuleDefinition) => void;
    commandMocks.protocolRuleSave.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    const user = userEvent.setup();
    const view = render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    await user.click(screen.getByRole("button", { name: "添加条件" }));
    await user.type(screen.getByRole("textbox", { name: "比较值" }), "0200");
    await user.click(screen.getByRole("button", { name: "保存报文规则" }));
    const staleResponse = savedFromInput(commandMocks.protocolRuleSave.mock.calls[0][0]) as ProtocolDocumentRuleDefinition;
    const changedListener = socketListener("relay");
    if (changedListener.data_plane.kind === "socket" && changedListener.data_plane.settings.processing?.mode === "scripted") {
      changedListener.data_plane.settings.processing.settings.package = { id: "iso8583", version: "2.0.0" };
    }
    queryState.listeners = [changedListener];
    queryState.capabilities.set("protocol-rule-capabilities:relay:app_to_proxy", { ...capability("app_to_proxy", 8), package: { id: "iso8583", version: "2.0.0" } });
    await act(async () => { await queryState.eventRefresh?.(); });
    view.rerender(<SocketRulesView />);
    const refreshCount = queryState.refresh.mock.calls.length;
    finish(staleResponse);
    expect(await screen.findByText("字段结构 v8")).toBeVisible();
    await waitFor(() => expect(screen.queryByDisplayValue("0200")).not.toBeInTheDocument());
    expect(screen.getByText("空条件恒匹配。")).toBeVisible();
    expect(queryState.refresh).toHaveBeenCalledTimes(refreshCount);
  });

  it("blocks toggle, reload, and delete while a field parser is pending", async () => {
    commandMocks.protocolRuleParseValue.mockReturnValue(new Promise(() => {}));
    queryState.rules = [rule(3)];
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    await user.type(screen.getByRole("textbox", { name: "比较值" }), "1");
    expect(screen.getByRole("switch", { name: "停用报文规则 rule-1" })).toBeDisabled();
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
      expect(screen.getByRole("button", { name: "新建报文规则" })).toBeDisabled();
      for (const name of ["保存报文规则", "删除规则", "重新加载当前规则"]) expect(screen.queryByRole("button", { name })).not.toBeInTheDocument();
    }
  });
});
