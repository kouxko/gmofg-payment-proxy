// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RuleDefinitionSaveInput, RuleEditorContext } from "@/generated/rust-types";
import { RulesView } from "./rules-view";
import { httpCondition, httpListener, httpRule } from "./rules-view-test-fixtures";

const commandMocks = vi.hoisted(() => ({
  workspaceList: vi.fn(), workspaceGet: vi.fn(), ruleDefinitionList: vi.fn(),
  ruleDefinitionGet: vi.fn(), ruleEditorContext: vi.fn(), ruleDefinitionSave: vi.fn(),
  ruleDefinitionDelete: vi.fn(), ruleDefinitionCopy: vi.fn(),
  ruleDefinitionCreateFromExchangeObservation: vi.fn(), ruleDefinitionHttpConditionDraft: vi.fn(),
  ruleDefinitionActionDraft: vi.fn(), ruleDefinitionDocumentConditionDraft: vi.fn(),
  ruleDefinitionDocumentActionDraft: vi.fn(), ruleDefinitionDocumentCommonActionDraft: vi.fn(),
}));

const navigationState = vi.hoisted(() => ({ searchParams: new URLSearchParams(), navigate: vi.fn() }));
vi.mock("@/generated/rust-types", () => ({ commands: commandMocks }));
vi.mock("@/lib/ipc/client", () => ({
  appErrorViewModel: () => undefined,
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => reason instanceof Error ? reason.message : "Rust 操作失败",
}));
vi.mock("@/features/shell/bootstrap-context", () => ({ useAppEventRefresh: vi.fn() }));
vi.mock("@/features/shell/selected-workspace-context", () => ({
  useSelectedWorkspace: () => ({
    selected: { id: "workspace", name: "Workspace", revision: 1 },
    summaries: [], isLoading: false, error: undefined, refresh: vi.fn().mockResolvedValue(undefined),
  }),
}));
vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceQueryInvalidation: vi.fn(),
  useWorkspaceNavigation: () => navigationState,
}));

const context: RuleEditorContext = {
  listener_id: httpListener.id,
  local_document_types: [],
  document_condition_path: { wildcard_token: "*", wildcard_matches_exactly_one_level: true, multiple_matches_use_any: true },
  content: { type: "http", value: { stages: [{
    stage: "proxy_to_upstream",
    http: { stage: "proxy_to_upstream", match_fields: [
      { kind: "method", operators: ["equals"], selector: null },
      { kind: "request_target", operators: ["equals"], selector: null },
    ], actions: [{ kind: "jitter", terminal: false, traffic_direction: null, parameters_required: true }] },
    package: null, document_fields: [], document_common_actions: ["record_match"],
    new_rule_draft: { listener_id: httpListener.id, stage: "proxy_to_upstream", content: { type: "http", value: { description: "" } } },
  }] } },
};

describe("RulesView editor session ownership", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    navigationState.searchParams = new URLSearchParams();
    commandMocks.workspaceList.mockResolvedValue([{ id: "workspace", name: "Workspace", selected: true }]);
    commandMocks.workspaceGet.mockResolvedValue({ id: "workspace", listeners: [httpListener] });
    commandMocks.ruleDefinitionList.mockResolvedValue([httpRule()]);
    commandMocks.ruleDefinitionGet.mockResolvedValue(httpRule());
    commandMocks.ruleEditorContext.mockResolvedValue(context);
    commandMocks.ruleDefinitionHttpConditionDraft.mockResolvedValue(httpCondition);
    commandMocks.ruleDefinitionDocumentCommonActionDraft.mockResolvedValue({ source: "record_match" });
  });

  it("abandons a materializing creation when New starts another editor session", async () => {
    let completeCondition!: (value: typeof httpCondition) => void;
    commandMocks.ruleDefinitionHttpConditionDraft.mockReturnValue(new Promise((resolve) => { completeCondition = resolve; }));
    const user = userEvent.setup();
    render(<RulesView />);
    await configureCreation(user, "First creation");

    await user.click(screen.getByRole("button", { name: "保存规则" }));
    await waitFor(() => expect(commandMocks.ruleDefinitionHttpConditionDraft).toHaveBeenCalledOnce());
    await user.click(screen.getByRole("button", { name: "新建规则" }));
    expect(screen.getByRole("textbox", { name: "规则名称" })).toHaveValue("");
    await act(async () => { completeCondition(httpCondition); await Promise.resolve(); });

    expect(commandMocks.ruleDefinitionSave).not.toHaveBeenCalled();
  });

  it("abandons capture draft A when capture draft B has the same rule identity", async () => {
    const draftA = captureDraft("Capture A");
    const draftB = captureDraft("Capture B");
    navigationState.searchParams = new URLSearchParams("exchangeId=A&responseEvent=0");
    commandMocks.ruleDefinitionCreateFromExchangeObservation.mockImplementation(async (exchangeId) => exchangeId === "B" ? draftB : draftA);
    let completeCondition!: (value: typeof httpCondition) => void;
    commandMocks.ruleDefinitionHttpConditionDraft.mockReturnValue(new Promise((resolve) => { completeCondition = resolve; }));
    const view = render(<RulesView />);

    const save = await screen.findByRole("button", { name: "保存规则" });
    await waitFor(() => expect(screen.getByRole("textbox", { name: "规则名称" })).toHaveValue("Capture A"));
    await userEvent.click(save);
    await waitFor(() => expect(commandMocks.ruleDefinitionHttpConditionDraft).toHaveBeenCalledOnce());
    navigationState.searchParams = new URLSearchParams("exchangeId=B&responseEvent=0");
    view.rerender(<RulesView />);
    await waitFor(() => expect(screen.getByRole("textbox", { name: "规则名称" })).toHaveValue("Capture B"));
    await act(async () => { completeCondition(httpCondition); await Promise.resolve(); });

    expect(commandMocks.ruleDefinitionSave).not.toHaveBeenCalled();
  });

  it("review reproduction: capture replacement and old materialization settle in one React batch", async () => {
    const draftA = captureDraft("Capture A");
    const draftB = captureDraft("Capture B");
    navigationState.searchParams = new URLSearchParams("exchangeId=A&responseEvent=0");
    commandMocks.ruleDefinitionCreateFromExchangeObservation.mockImplementation(async (exchangeId) => exchangeId === "B" ? draftB : draftA);
    let contextCalls = 0;
    let completeContextB!: (value: RuleEditorContext) => void;
    commandMocks.ruleEditorContext.mockImplementation(() => {
      contextCalls += 1;
      if (contextCalls === 2) return new Promise((resolve) => { completeContextB = resolve; });
      return Promise.resolve(context);
    });
    let completeCondition!: (value: typeof httpCondition) => void;
    commandMocks.ruleDefinitionHttpConditionDraft.mockReturnValue(new Promise((resolve) => { completeCondition = resolve; }));
    const view = render(<RulesView />);

    const save = await screen.findByRole("button", { name: "保存规则" });
    await waitFor(() => expect(screen.getByRole("textbox", { name: "规则名称" })).toHaveValue("Capture A"));
    await userEvent.click(save);
    await waitFor(() => expect(commandMocks.ruleDefinitionHttpConditionDraft).toHaveBeenCalledOnce());
    navigationState.searchParams = new URLSearchParams("exchangeId=B&responseEvent=0");
    view.rerender(<RulesView />);
    await waitFor(() => expect(contextCalls).toBe(2));

    await act(async () => {
      completeContextB(context);
      completeCondition(httpCondition);
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(screen.getByRole("textbox", { name: "规则名称" })).toHaveValue("Capture B");
    expect(commandMocks.ruleDefinitionSave).not.toHaveBeenCalled();
  });
});

async function configureCreation(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.click(await screen.findByRole("button", { name: "新建规则" }));
  await user.click(screen.getByRole("button", { name: /创建规则的 Listener/ }));
  await user.click(await screen.findByRole("option", { name: "HTTP Listener · HTTP" }));
  await user.click(screen.getByRole("button", { name: /处理阶段/ }));
  await user.click(await screen.findByRole("option", { name: "Proxy → Server" }));
  await user.type(screen.getByRole("textbox", { name: "规则名称" }), name);
  await user.click(screen.getByRole("button", { name: /Increase 阶段内优先级/ }));
  await user.click(screen.getByRole("tab", { name: "匹配条件" }));
  await user.click(screen.getByRole("button", { name: /条件来源/ }));
  await user.click(await screen.findByRole("option", { name: "HTTP" }));
  await user.click(screen.getByRole("button", { name: /HTTP 匹配字段/ }));
  await user.click(await screen.findByRole("option", { name: /Path/ }));
  await user.click(screen.getByRole("button", { name: /HTTP 匹配操作符/ }));
  await user.click(await screen.findByRole("option", { name: "equals" }));
  await user.type(screen.getByRole("textbox", { name: "HTTP 匹配值" }), "/first");
  await user.click(screen.getByRole("tab", { name: "执行动作" }));
  await user.click(screen.getByRole("button", { name: /动作来源/ }));
  await user.click(await screen.findByRole("option", { name: "通用" }));
  await user.click(screen.getByRole("button", { name: /通用动作/ }));
  await user.click(await screen.findByRole("option", { name: "记录命中" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "保存规则" })).toBeEnabled());
}

function captureDraft(name: string): RuleDefinitionSaveInput {
  const rule = httpRule({ name });
  return {
    rule_id: null,
    expected_revision: null,
    draft: {
      name: rule.name,
      enabled: rule.enabled,
      priority: rule.priority,
      listener_id: rule.listener_id,
      stage: rule.stage,
      content: rule.content,
    },
  };
}
