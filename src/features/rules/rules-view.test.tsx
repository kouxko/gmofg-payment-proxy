// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProxyListener, RuleDefinition_Serialize, RuleEditorContext } from "@/generated/rust-types";
import { RulesView } from "./rules-view";

const commandMocks = vi.hoisted(() => ({
  workspaceList: vi.fn(), workspaceGet: vi.fn(), ruleDefinitionList: vi.fn(),
  ruleDefinitionGet: vi.fn(), ruleEditorContext: vi.fn(), ruleDefinitionSave: vi.fn(),
  ruleDefinitionToggle: vi.fn(), ruleDefinitionDelete: vi.fn(), ruleParseDocumentValue: vi.fn(),
  ruleDefinitionCopy: vi.fn(), ruleDefinitionCreateFromExchangeObservation: vi.fn(),
  ruleDefinitionConditionDraft: vi.fn(), ruleDefinitionActionDraft: vi.fn(),
}));

const navigationState = vi.hoisted(() => ({
  searchParams: new URLSearchParams(),
  navigate: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({ commands: commandMocks }));
vi.mock("@/lib/ipc/client", () => ({
  appErrorViewModel: () => undefined,
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => reason instanceof Error ? reason.message : "Rust 操作失败",
}));
vi.mock("@/features/shell/bootstrap-context", () => ({ useAppEventRefresh: vi.fn() }));
vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceQueryInvalidation: vi.fn(),
  useWorkspaceNavigation: () => navigationState,
}));

const httpListener = listener("http-listener", "HTTP Listener", "http");
const socketListener = listener("socket-listener", "Socket Listener", "socket");
const httpCondition = { operator: "leaf" as const, children: { source: "http" as const, condition: { Field: { field: "PathOrRequestType" as const, operator: { Equals: "/" } } } } };
const documentCondition = (path = "/amount", value = 0) => ({ operator: "leaf" as const, children: { source: "document" as const, path, predicate: { type: "number" as const, value: { operator: "equal" as const, value } } } });
const lifecycle = { hit_count: 0, last_hit_at: null };

function listener(id: string, name: string, kind: "http" | "socket"): ProxyListener {
  return {
    id, name, enabled: true, bind_address: "127.0.0.1", port: kind === "http" ? 8080 : 9000,
    connect_timeout_ms: 1_000, read_timeout_ms: 1_000, write_timeout_ms: 1_000,
    data_plane: kind === "http"
      ? { kind: "http", settings: { body_processing: { mode: "plain" } } }
      : { kind: "socket", settings: { processing: { mode: "direct" } } },
  } as ProxyListener;
}

function httpRule(overrides: Partial<RuleDefinition_Serialize> = {}): RuleDefinition_Serialize {
  return {
    rule_id: "http-rule", revision: 3, name: "HTTP combined", enabled: true, priority: 50,
    created_order: 2, listener_id: httpListener.id, stage: "proxy_to_upstream", one_shot: false, lifecycle,
    content: { type: "http", value: {
      description: "headers and body", condition: httpCondition, actions: [{ source: "record_match" }], document: null,
    } },
    ...overrides,
  };
}

function socketRule(): RuleDefinition_Serialize {
  return {
    rule_id: "socket-rule", revision: 4, name: "Socket document", enabled: true, priority: 20,
    created_order: 1, listener_id: socketListener.id, stage: "proxy_to_app", one_shot: false, lifecycle,
    content: { type: "socket", value: {
      package: { id: "iso8583", version: "1.0.0" },
      condition: documentCondition(), actions: [{ source: "record_match" }],
    } },
  };
}

const httpContext: RuleEditorContext = {
  listener_id: httpListener.id,
  content: { type: "http", value: { stages: [{
    stage: "proxy_to_upstream",
    http: { stage: "request", match_field_kinds: ["path_or_request_type", "json_path"], actions: [{ kind: "set_header", terminal: false, traffic_direction: null }] },
    package: { id: "iso8583", version: "1.0.0" },
    document_fields: [], document_common_actions: ["record_match"],
    new_rule_draft: { rule_id: null, expected_revision: null, draft: {
      name: "新建 HTTP 规则", enabled: true, priority: 100, listener_id: httpListener.id,
      stage: "proxy_to_upstream", one_shot: false, content: { type: "http", value: {
        description: "", condition: httpCondition, actions: [{ source: "record_match" }],
        document: { package: { id: "iso8583", version: "1.0.0" } },
      } },
    } },
  }] } },
};

const socketContext: RuleEditorContext = {
  listener_id: socketListener.id,
  content: { type: "socket", value: { package: { id: "iso8583", version: "1.0.0" }, stages: [{
    stage: "proxy_to_app", fields: [], common_actions: ["record_match"],
    new_rule_draft: { rule_id: null, expected_revision: null, draft: {
      name: "新建 Socket 规则", enabled: true, priority: 100, listener_id: socketListener.id,
      stage: "proxy_to_app", one_shot: false, content: { type: "socket", value: {
        package: { id: "iso8583", version: "1.0.0" }, condition: documentCondition(), actions: [{ source: "record_match" }],
      } },
    } },
  }] } },
};

describe("unified rule workspace", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    navigationState.searchParams = new URLSearchParams();
    commandMocks.workspaceList.mockResolvedValue([{ id: "workspace", selected: true }]);
    commandMocks.workspaceGet.mockResolvedValue({ id: "workspace", listeners: [httpListener, socketListener] });
    commandMocks.ruleDefinitionList.mockResolvedValue([socketRule(), httpRule()]);
    commandMocks.ruleDefinitionGet.mockImplementation(async (id: string) => id === "socket-rule" ? socketRule() : httpRule());
    commandMocks.ruleEditorContext.mockImplementation(async (id: string) => id === httpListener.id ? httpContext : socketContext);
    commandMocks.ruleDefinitionSave.mockImplementation(async (input) => ({
      rule_id: input.rule_id ?? "created-rule", revision: (input.expected_revision ?? 0) + 1,
      created_order: 3, ...input.draft,
    }));
    commandMocks.ruleDefinitionToggle.mockImplementation(async (_id, revision, enabled) => ({ ...httpRule(), revision: revision + 1, enabled }));
    commandMocks.ruleDefinitionDelete.mockResolvedValue({ success: true, cancelled: false, message: "规则已删除", ui_tone: "positive", entity_id: "http-rule", revision: 4, requires_restart: false });
    commandMocks.ruleDefinitionCopy.mockResolvedValue(httpRule({ rule_id: "http-rule-copy", revision: 1, name: "HTTP combined copy", created_order: 9 }));
    commandMocks.ruleDefinitionCreateFromExchangeObservation.mockResolvedValue({
      rule_id: null,
      expected_revision: null,
      draft: {
        ...httpContext.content.value.stages[0].new_rule_draft.draft,
        name: "Mock /checkout",
        enabled: false,
        content: {
          type: "http",
          value: {
            ...httpContext.content.value.stages[0].new_rule_draft.draft.content.value,
            actions: [{ source: "terminal", value: { MockResponse: { status: 201, headers: [["content-type", "application/json"]], body_bytes: [123, 125] } } }],
          },
        },
      },
    });
    commandMocks.ruleDefinitionConditionDraft.mockResolvedValue({ source: "http", condition: { Field: { field: "PathOrRequestType", operator: { Equals: "" } } } });
    commandMocks.ruleDefinitionActionDraft.mockResolvedValue({ SetHeader: { name: "x-proxy-test", value: "" } });
    commandMocks.ruleParseDocumentValue.mockResolvedValue(0);
  });

  it("uses one list and groups rules by the fixed pipeline stage order", async () => {
    render(<RulesView />);
    expect(await screen.findByRole("heading", { name: "规则" })).toBeVisible();
    expect(screen.getAllByTestId("rule-stage-heading").map((item) => item.textContent)).toEqual([
      "App → Proxy", "Proxy → Upstream", "Upstream → Proxy", "Proxy → App", "TLS 握手",
    ]);
    expect(screen.getByRole("button", { name: "新建规则" })).toBeVisible();
    expect(screen.queryByText("Body 报文规则")).not.toBeInTheDocument();
  });

  it("edits HTTP Header and Body in the same HTTP content shell", async () => {
    render(<RulesView />);
    await userEvent.setup().click(await screen.findByRole("button", { name: /HTTP combined/ }));
    expect(await screen.findByRole("heading", { name: "HTTP 规则内容" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "HTTP Header、URL 与请求信息" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "HTTP Body Document" })).toBeVisible();
    expect(screen.getByDisplayValue("HTTP Listener")).toBeVisible();
    expect(screen.queryByRole("combobox", { name: "Listener" })).not.toBeInTheDocument();
  });

  it("keeps Socket document-only and does not expose HTTP capabilities", async () => {
    render(<RulesView />);
    await userEvent.setup().click(await screen.findByRole("button", { name: /Socket document/ }));
    expect(await screen.findByRole("heading", { name: "Socket Document 规则内容" })).toBeVisible();
    expect(screen.getByText("iso8583@1.0.0")).toBeVisible();
    expect(screen.queryByText("HTTP Header、URL 与请求信息")).not.toBeInTheDocument();
    expect(screen.queryByText("Mock Response")).not.toBeInTheDocument();
    expect(screen.queryByText("one_shot")).not.toBeInTheDocument();
  });

  it("creates from Rust's tagged listener context and saves through the unified API", async () => {
    const user = userEvent.setup();
    render(<RulesView />);
    await user.click(await screen.findByRole("button", { name: "新建规则" }));
    await user.click(screen.getByRole("button", { name: /创建规则的 Listener/ }));
    await user.click(await screen.findByRole("option", { name: "HTTP Listener · HTTP" }));
    await user.click(await screen.findByRole("button", { name: "Proxy → Upstream" }));
    await user.clear(screen.getByRole("textbox", { name: "规则名称" }));
    await user.type(screen.getByRole("textbox", { name: "规则名称" }), "Combined rule");
    await user.click(screen.getByRole("button", { name: "保存规则" }));
    await waitFor(() => expect(commandMocks.ruleDefinitionSave).toHaveBeenCalledWith(expect.objectContaining({
      rule_id: null, expected_revision: null,
      draft: expect.objectContaining({ listener_id: httpListener.id, stage: "proxy_to_upstream", content: expect.objectContaining({ type: "http" }) }),
    })));
  });

  it("toggles and deletes through the unified revision-aware commands", async () => {
    const user = userEvent.setup();
    render(<RulesView />);
    await user.click(await screen.findByRole("button", { name: /HTTP combined/ }));
    await user.click(await screen.findByRole("switch", { name: "启用规则" }));
    expect(commandMocks.ruleDefinitionToggle).toHaveBeenCalledWith("http-rule", 3, false);
    await user.click(screen.getByRole("button", { name: "删除规则" }));
    await user.click(screen.getByRole("button", { name: "确认删除" }));
    expect(commandMocks.ruleDefinitionDelete).toHaveBeenCalledWith("http-rule", 4, true);
  });

  it("copies the selected unified definition through the Rust copy command", async () => {
    const user = userEvent.setup();
    render(<RulesView />);
    await user.click(await screen.findByRole("button", { name: /HTTP combined/ }));
    await user.click(await screen.findByRole("button", { name: "复制规则" }));

    expect(commandMocks.ruleDefinitionCopy).toHaveBeenCalledWith("http-rule");
    expect(await screen.findByDisplayValue("HTTP combined copy")).toBeVisible();
  });

  it("replays TASK-003 capture navigation into an unsaved disabled unified HTTP Mock draft", async () => {
    navigationState.searchParams = new URLSearchParams("exchangeId=exchange-7&responseEvent=4");
    render(<RulesView />);

    expect(commandMocks.ruleDefinitionCreateFromExchangeObservation).toHaveBeenCalledWith("exchange-7", 4);
    expect(await screen.findByDisplayValue("Mock /checkout")).toBeVisible();
    expect(screen.getByRole("switch", { name: "启用规则" })).not.toBeChecked();
    expect(screen.getByText("Mock Response")).toBeVisible();
    expect(navigationState.navigate).toHaveBeenCalledWith("/rules");
  });

  it("uses Rust-authoritative typed factories instead of inventing HTTP defaults", async () => {
    const user = userEvent.setup();
    render(<RulesView />);
    await user.click(await screen.findByRole("button", { name: /HTTP combined/ }));
    await user.click(await screen.findByRole("button", { name: "添加条件：字段" }));
    await user.click(await screen.findByRole("button", { name: "添加动作：Set Header" }));
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    expect(commandMocks.ruleDefinitionConditionDraft).toHaveBeenCalledWith("field", "request");
    expect(commandMocks.ruleDefinitionActionDraft).toHaveBeenCalledWith("set_header", "request");
    expect(commandMocks.ruleDefinitionSave).toHaveBeenCalledWith(expect.objectContaining({
      draft: expect.objectContaining({ content: { type: "http", value: expect.objectContaining({
        condition: { operator: "all", children: [httpCondition, { operator: "leaf", children: { source: "http", condition: { Field: { field: "PathOrRequestType", operator: { Equals: "" } } } } }] },
        actions: [{ source: "record_match" }, { source: "http", value: { SetHeader: { name: "x-proxy-test", value: "" } } }],
      }) } }),
    }));
  });

  it("discards a factory response from the previously selected rule", async () => {
    let finishOldRequest!: (value: unknown) => void;
    const secondRule = httpRule({ rule_id: "http-rule-b", revision: 7, name: "HTTP second", created_order: 8 });
    commandMocks.ruleDefinitionList.mockResolvedValue([httpRule(), secondRule]);
    commandMocks.ruleDefinitionGet.mockImplementation(async (id: string) => id === secondRule.rule_id ? secondRule : httpRule());
    commandMocks.ruleDefinitionConditionDraft.mockReturnValue(new Promise((resolve) => { finishOldRequest = resolve; }));
    const user = userEvent.setup();
    render(<RulesView />);

    await user.click(await screen.findByRole("button", { name: /HTTP combined/ }));
    await user.click(await screen.findByRole("button", { name: "添加条件：字段" }));
    await user.click(screen.getByRole("button", { name: /HTTP second/ }));
    expect(await screen.findByDisplayValue("HTTP second")).toBeVisible();

    await act(async () => {
      finishOldRequest({ source: "http", condition: { Field: { field: "PathOrRequestType", operator: { Equals: "old" } } } });
      await Promise.resolve();
    });
    expect(screen.getByDisplayValue("HTTP second")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "保存规则" }));
    expect(commandMocks.ruleDefinitionSave).toHaveBeenLastCalledWith(expect.objectContaining({
      rule_id: secondRule.rule_id,
      draft: expect.objectContaining({ content: { type: "http", value: expect.objectContaining({ condition: httpCondition }) } }),
    }));
  });

  it("merges out-of-order condition and action factory responses into the latest draft", async () => {
    let finishCondition!: (value: unknown) => void;
    let finishAction!: (value: unknown) => void;
    commandMocks.ruleDefinitionConditionDraft.mockReturnValue(new Promise((resolve) => { finishCondition = resolve; }));
    commandMocks.ruleDefinitionActionDraft.mockReturnValue(new Promise((resolve) => { finishAction = resolve; }));
    const user = userEvent.setup();
    render(<RulesView />);
    await user.click(await screen.findByRole("button", { name: /HTTP combined/ }));

    act(() => {
      fireEvent.click(screen.getByRole("button", { name: "添加条件：字段" }));
      fireEvent.click(screen.getByRole("button", { name: "添加动作：Set Header" }));
    });
    await act(async () => {
      finishAction({ SetHeader: { name: "x-latest", value: "1" } });
      await Promise.resolve();
      finishCondition({ source: "http", condition: { Field: { field: "PathOrRequestType", operator: { Equals: "/pay" } } } });
      await Promise.resolve();
    });
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    expect(commandMocks.ruleDefinitionSave).toHaveBeenLastCalledWith(expect.objectContaining({
      draft: expect.objectContaining({ content: { type: "http", value: expect.objectContaining({
        condition: { operator: "all", children: [httpCondition, { operator: "leaf", children: { source: "http", condition: { Field: { field: "PathOrRequestType", operator: { Equals: "/pay" } } } } }] },
        actions: [{ source: "record_match" }, { source: "http", value: { SetHeader: { name: "x-latest", value: "1" } } }],
      }) } }),
    }));
  });

  it("switches an HTTP rule between header-only and Rust's optional Document draft", async () => {
    commandMocks.ruleEditorContext.mockResolvedValue(httpContextWithOptionalDocument());
    const user = userEvent.setup();
    render(<RulesView />);
    await user.click(await screen.findByRole("button", { name: /HTTP combined/ }));

    expect(screen.getByText("当前规则仅处理 HTTP Header；可按 Rust 草稿启用 Body Document。" )).toBeVisible();
    await user.click(screen.getByRole("button", { name: "添加 HTTP Body Document" }));
    expect(screen.getByText("iso8583@1.0.0")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "移除 HTTP Body Document" }));
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    expect(commandMocks.ruleDefinitionSave).toHaveBeenLastCalledWith(expect.objectContaining({
      draft: expect.objectContaining({ content: { type: "http", value: expect.objectContaining({ document: null }) } }),
    }));
  });

  it("creates HTTP Document conditions and field/common actions from Rust capabilities and typed values", async () => {
    commandMocks.ruleEditorContext.mockResolvedValue(httpContextWithOptionalDocument());
    const user = userEvent.setup();
    render(<RulesView />);
    await user.click(await screen.findByRole("button", { name: /HTTP combined/ }));
    await user.click(screen.getByRole("button", { name: "添加 HTTP Body Document" }));
    await user.type(screen.getByRole("textbox", { name: "Document 值：Amount" }), "0");
    await user.click(screen.getByRole("button", { name: "添加条件：Amount equals" }));
    await user.click(screen.getByRole("button", { name: "添加动作：Set Amount" }));
    await user.click(screen.getByRole("button", { name: "添加动作：Clear Amount" }));
    await user.click(screen.getByRole("button", { name: "添加：记录命中" }));
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    expect(commandMocks.ruleParseDocumentValue).toHaveBeenCalledTimes(2);
    expect(commandMocks.ruleParseDocumentValue).toHaveBeenCalledWith("number", "0");
    expect(commandMocks.ruleDefinitionSave).toHaveBeenLastCalledWith(expect.objectContaining({
      draft: expect.objectContaining({ content: { type: "http", value: expect.objectContaining({
        document: { package: { id: "iso8583", version: "1.0.0" } },
        condition: { operator: "all", children: [httpCondition, documentCondition()] },
        actions: [
          { source: "record_match" },
          { source: "document", value: { type: "set", path: "/amount", value: 0 } },
          { source: "document", value: { type: "clear", path: "/amount" } },
          { source: "record_match" },
        ],
      }) } }),
    }));
  });

  it("preserves Socket Document conditions and Set/ClearField capabilities", async () => {
    commandMocks.ruleEditorContext.mockResolvedValue(socketContextWithFields());
    const user = userEvent.setup();
    render(<RulesView />);
    await user.click(await screen.findByRole("button", { name: /Socket document/ }));
    await user.type(screen.getByRole("textbox", { name: "Document 值：Amount" }), "0");
    await user.click(screen.getByRole("button", { name: "添加条件：Amount equals" }));
    await user.click(screen.getByRole("button", { name: "添加动作：Set Amount" }));
    await user.click(screen.getByRole("button", { name: "添加动作：Clear Amount" }));
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    expect(commandMocks.ruleDefinitionSave).toHaveBeenLastCalledWith(expect.objectContaining({
      rule_id: "socket-rule",
      draft: expect.objectContaining({ content: { type: "socket", value: expect.objectContaining({
        condition: { operator: "all", children: [documentCondition(), documentCondition()] },
        actions: [
          { source: "record_match" },
          { source: "document", value: { type: "set", path: "/amount", value: 0 } },
          { source: "document", value: { type: "clear", path: "/amount" } },
        ],
      }) } }),
    }));
  });

  it("blocks an HTTP stage whose Rust capabilities cannot edit the retained payload", async () => {
    const rule = httpRule({
      content: { type: "http", value: {
        description: "headers and body", condition: httpCondition, actions: [{ source: "http", value: { SetHeader: { name: "x-test", value: "1" } } }],
        document: null,
      } },
    });
    commandMocks.ruleDefinitionList.mockResolvedValue([rule]);
    commandMocks.ruleDefinitionGet.mockResolvedValue(rule);
    commandMocks.ruleEditorContext.mockResolvedValue(httpContextWithSecondStage([]));
    const user = userEvent.setup();
    render(<RulesView />);
    await user.click(await screen.findByRole("button", { name: /HTTP combined/ }));

    await user.click(screen.getByRole("button", { name: /处理阶段/ }));
    const blocked = await screen.findByRole("option", { name: /Proxy → App.*Set Header/ });
    expect(blocked).toHaveAttribute("aria-disabled", "true");
    expect(screen.getAllByText(/目标阶段不支持 HTTP 动作 Set Header/)).not.toHaveLength(0);
  });

  it("disables save when Rust no longer declares the selected stage payload compatible", async () => {
    const rule = httpRule({ content: { type: "http", value: {
      description: "", condition: httpCondition, actions: [{ source: "http", value: { SetHeader: { name: "x-test", value: "1" } } }],
      document: null,
    } } });
    const context = httpContextWithSecondStage([]);
    if (context.content.type !== "http" || !context.content.value.stages[0].http) throw new Error("HTTP context fixture is invalid");
    context.content.value.stages[0].http.actions = [];
    commandMocks.ruleDefinitionList.mockResolvedValue([rule]);
    commandMocks.ruleDefinitionGet.mockResolvedValue(rule);
    commandMocks.ruleEditorContext.mockResolvedValue(context);
    render(<RulesView />);

    await userEvent.setup().click(await screen.findByRole("button", { name: /HTTP combined/ }));
    expect(screen.getByRole("alert")).toHaveTextContent("当前阶段不可保存：目标阶段不支持 HTTP 动作 Set Header");
    expect(screen.getByRole("button", { name: "保存规则" })).toBeDisabled();
  });

  it("switches only to a compatible stage and preserves the complete HTTP payload", async () => {
    const retainedContent = {
      description: "preserve me", condition: { operator: "leaf" as const, children: { source: "http" as const, condition: { Field: { field: "PathOrRequestType" as const, operator: { Equals: "/pay" } } } } },
      actions: [{ source: "http" as const, value: { SetHeader: { name: "x-test", value: "1" } } }], document: null,
    };
    const retainedLifecycle = { one_shot: true, hit_count: 7, last_hit_at: "2026-08-28T00:00:00Z" };
    const rule = httpRule({ lifecycle: retainedLifecycle, content: { type: "http", value: retainedContent } });
    commandMocks.ruleDefinitionList.mockResolvedValue([rule]);
    commandMocks.ruleDefinitionGet.mockResolvedValue(rule);
    commandMocks.ruleEditorContext.mockResolvedValue(httpContextWithSecondStage(["set_header"]));
    const user = userEvent.setup();
    render(<RulesView />);
    await user.click(await screen.findByRole("button", { name: /HTTP combined/ }));

    await user.click(screen.getByRole("button", { name: /处理阶段/ }));
    await user.click(await screen.findByRole("option", { name: "Proxy → App" }));
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    expect(commandMocks.ruleDefinitionSave).toHaveBeenLastCalledWith(expect.objectContaining({
      draft: expect.objectContaining({ stage: "proxy_to_app", content: { type: "http", value: retainedContent } }),
    }));
  });
});

function httpContextWithOptionalDocument() {
  const stage = httpContext.content.type === "http" ? httpContext.content.value.stages[0] : undefined;
  if (!stage) throw new Error("HTTP context fixture is invalid");
  return {
    ...httpContext,
    content: { type: "http", value: { stages: [{
      ...stage,
      document_fields: [{ name: "/amount", label: "Amount", type: "number", operators: ["equals"], actions: ["set_field", "clear_field"] }],
    }] } },
  } as unknown as RuleEditorContext;
}

function socketContextWithFields() {
  const stage = socketContext.content.type === "socket" ? socketContext.content.value.stages[0] : undefined;
  if (!stage) throw new Error("Socket context fixture is invalid");
  return {
    ...socketContext,
    content: { type: "socket", value: { ...socketContext.content.value, stages: [{
      ...stage,
      fields: [{ name: "/amount", label: "Amount", type: "number", operators: ["equals"], actions: ["set_field", "clear_field"] }],
    }] } },
  } as RuleEditorContext;
}

function httpContextWithSecondStage(actionKinds: Array<"set_header">): RuleEditorContext {
  const stage = httpContext.content.type === "http" ? httpContext.content.value.stages[0] : undefined;
  if (!stage) throw new Error("HTTP context fixture is invalid");
  return {
    ...httpContext,
    content: { type: "http", value: { stages: [stage, {
      ...stage,
      stage: "proxy_to_app",
      http: {
        stage: "response",
        match_field_kinds: ["path_or_request_type", "json_path"],
        actions: actionKinds.map((kind) => ({ kind, terminal: false, traffic_direction: null })),
      },
      new_rule_draft: {
        ...stage.new_rule_draft,
        draft: { ...stage.new_rule_draft.draft, stage: "proxy_to_app" },
      },
    }] } },
  };
}
