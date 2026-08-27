// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import type { ProtocolDocumentRuleDefinition } from "@/generated/rust-types";
import {
  deleted,
  editorContext,
  packageRef,
  rule,
  savedFromInput,
  socketListener,
} from "./socket-rules-view.test-support";
import {
  getCommandMocks,
  getQueryState,
  HttpRulesView,
  resetSocketRulesViewTestState,
  SocketRulesView,
} from "./socket-rules-view.test-runtime";

const commandMocks = getCommandMocks();
const queryState = getQueryState();

describe("Socket rules view state and command contracts", () => {
  beforeEach(resetSocketRulesViewTestState);

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
    render(<HttpRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    await user.click(screen.getByLabelText("协议入口"));
    expect(await screen.findByRole("option", { name: /http.*HTTP/ })).toBeVisible();
    expect(screen.queryByRole("option", { name: /relay.*Socket/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /local.*本机应答/ })).not.toBeInTheDocument();
  });

  it("renders only the stages returned by the Rust editor context", async () => {
    queryState.capabilities.set(
      "protocol-rule-editor-context:relay",
      editorContext("relay", [{ stage: "proxy_to_app" }]),
    );
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    await user.click(screen.getByLabelText("报文处理阶段"));
    expect(await screen.findByRole("option", { name: "代理 → 应用" })).toBeVisible();
    expect(screen.queryByRole("option", { name: "应用 → 代理" })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "代理 → 上游服务" })).not.toBeInTheDocument();
  });

  it("uses the Rust-provided new-rule draft without replacing its defaults", async () => {
    const context = editorContext("relay", [{ stage: "app_to_proxy" }], "clear_document");
    context.stages[0].new_rule_draft.name = "Rust 草稿";
    context.stages[0].new_rule_draft.priority = 321;
    queryState.capabilities.set("protocol-rule-editor-context:relay", context);
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    expect(screen.getByRole("textbox", { name: "规则名称" })).toHaveValue("Rust 草稿");
    expect(screen.getByRole("textbox", { name: "优先级" })).toHaveValue("321");
    await user.click(screen.getByRole("button", { name: "保存报文规则" }));
    expect(commandMocks.protocolRuleSave).toHaveBeenCalledWith(expect.objectContaining({
      name: "Rust 草稿",
      priority: 321,
      actions: [{ type: "clear_document" }],
    }));
  });

  it("shows a Rust context load failure without creating a local fallback draft", async () => {
    queryState.capabilityError = "Rust 编辑上下文不可用";
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    expect(await screen.findByText("Rust 编辑上下文不可用")).toBeVisible();
    expect(screen.queryByRole("button", { name: "保存报文规则" })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "规则名称" })).not.toBeInTheDocument();
  });

  it("fails closed when a late context response belongs to the previous listener", async () => {
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    await user.click(screen.getByLabelText("协议入口"));
    queryState.capabilities.set(
      "protocol-rule-editor-context:local",
      editorContext("relay", [{ stage: "app_to_proxy" }]),
    );
    await user.click(await screen.findByRole("option", { name: /local/ }));
    expect(await screen.findByText("规则编辑上下文与当前入口或协议版本不一致。")).toBeVisible();
    expect(screen.queryByRole("button", { name: "保存报文规则" })).not.toBeInTheDocument();
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
    render(<HttpRulesView />);
    expect(screen.getByText("HTTP 金额规则")).toBeVisible();
    expect(screen.queryByText("Socket 金额规则")).not.toBeInTheDocument();
  });

  it("saves an HTTP protocol rule through the existing protocolRuleSave facade", async () => {
    const user = userEvent.setup();
    render(<HttpRulesView />);
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

    const addCondition = screen.getByRole("button", { name: "添加条件" });
    await user.click(addCondition);
    await user.type(screen.getByRole("textbox", { name: "比较值" }), "0200");
    await waitFor(() => expect(addCondition).toBeEnabled());
    await user.click(addCondition);
    const conditionValues = screen.getAllByRole("textbox", { name: "比较值" });
    await user.clear(conditionValues[1]);
    await user.type(conditionValues[1], "100");

    const addClearDocument = screen.getByRole("button", { name: "添加：清空全部字段" });
    await waitFor(() => expect(addClearDocument).toBeEnabled());
    await user.click(addClearDocument);
    await user.click(screen.getByRole("button", { name: "添加：设置字段" }));
    await user.type(screen.getByRole("textbox", { name: "设置值" }), "0210");
    const deleteFirstAction = screen.getByRole("button", { name: "删除动作 1" });
    await waitFor(() => expect(deleteFirstAction).toBeEnabled());
    await user.click(deleteFirstAction);
    const saveButton = screen.getByRole("button", { name: "保存报文规则" });
    await waitFor(() => expect(saveButton).toBeEnabled());
    await user.click(saveButton);

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
    const saveButton = screen.getByRole("button", { name: "保存报文规则" });
    await waitFor(() => expect(saveButton).toBeEnabled());
    await user.click(saveButton);
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
    const saveButton = screen.getByRole("button", { name: "保存报文规则" });
    await waitFor(() => expect(saveButton).toBeEnabled());
    await user.click(saveButton);
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
    const malformed = editorContext("relay", [{ stage: "app_to_proxy" }]);
    malformed.stages[0].fields = [
      { name: "amount", label: "金额", type: "decimal", operators: ["equals"], actions: ["set_field"] } as never,
    ];
    queryState.capabilities.set("protocol-rule-editor-context:relay", malformed);
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: "新建报文规则" }));
    expect(screen.getByText(/规则能力.*未知字段类型/)).toBeVisible();
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
    const saveButton = screen.getByRole("button", { name: "保存报文规则" });
    await waitFor(() => expect(saveButton).toBeEnabled());
    await user.click(saveButton);
    const staleResponse = savedFromInput(commandMocks.protocolRuleSave.mock.calls[0][0]) as ProtocolDocumentRuleDefinition;
    const changedListener = socketListener("relay");
    if (changedListener.data_plane.kind === "socket" && changedListener.data_plane.settings.processing?.mode === "scripted") {
      changedListener.data_plane.settings.processing.settings.package = { id: "iso8583", version: "2.0.0" };
    }
    queryState.listeners = [changedListener];
    const changedContext = editorContext("relay", [
      { stage: "app_to_proxy", schemaVersion: 8 },
      { stage: "proxy_to_upstream", schemaVersion: 8 },
      { stage: "upstream_to_proxy", schemaVersion: 8 },
      { stage: "proxy_to_app", schemaVersion: 8 },
    ]);
    changedContext.package = { id: "iso8583", version: "2.0.0" };
    for (const item of changedContext.stages) {
      item.new_rule_draft.package = changedContext.package;
    }
    queryState.capabilities.set("protocol-rule-editor-context:relay", changedContext);
    await act(async () => { await queryState.eventRefresh?.(); });
    view.rerender(<SocketRulesView />);
    const refreshCount = queryState.refresh.mock.calls.length;
    finish(staleResponse);
    expect(await screen.findByText("字段结构 v8")).toBeVisible();
    await waitFor(() => expect(screen.queryByDisplayValue("0200")).not.toBeInTheDocument());
    expect(screen.getByText("空条件恒匹配。")).toBeVisible();
    expect(queryState.refresh).toHaveBeenCalledTimes(refreshCount);
  });

  it("blocks only save while a field parser is pending", async () => {
    commandMocks.protocolRuleParseValue.mockReturnValue(new Promise(() => {}));
    queryState.rules = [rule(3)];
    const user = userEvent.setup();
    render(<SocketRulesView />);
    await user.click(screen.getByRole("button", { name: /relay/ }));
    await user.type(screen.getByRole("textbox", { name: "比较值" }), "1");
    expect(screen.getByRole("button", { name: "保存报文规则" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "停用报文规则 rule-1" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "重新加载当前规则" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "删除规则" })).toBeEnabled();
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
