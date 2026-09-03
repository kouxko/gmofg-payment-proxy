// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RuleDefinition_Serialize, RuleEditorContext } from "@/generated/rust-types";
import { RulesView } from "./rules-view";
import { httpCondition, httpListener, httpRule, socketListener } from "./rules-view-test-fixtures";

const commandMocks = vi.hoisted(() => ({
  workspaceList: vi.fn(), workspaceGet: vi.fn(), ruleDefinitionList: vi.fn(),
  ruleDefinitionGet: vi.fn(), ruleEditorContext: vi.fn(), ruleDefinitionSave: vi.fn(),
  ruleDefinitionDelete: vi.fn(), ruleDefinitionCopy: vi.fn(),
  ruleDefinitionCreateFromExchangeObservation: vi.fn(), ruleDefinitionHttpConditionDraft: vi.fn(),
  ruleDefinitionActionDraft: vi.fn(),
  ruleDefinitionDocumentConditionDraft: vi.fn(), ruleDefinitionDocumentActionDraft: vi.fn(),
  ruleDefinitionDocumentCommonActionDraft: vi.fn(),
}));

const navigationState = vi.hoisted(() => ({ searchParams: new URLSearchParams(), navigate: vi.fn() }));
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

describe("RulesView inline editor", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    navigationState.searchParams = new URLSearchParams();
    commandMocks.workspaceList.mockResolvedValue([{ id: "workspace", name: "Workspace", selected: true }]);
    commandMocks.workspaceGet.mockResolvedValue({ id: "workspace", listeners: [httpListener] });
    commandMocks.ruleDefinitionList.mockResolvedValue([httpRule()]);
    commandMocks.ruleDefinitionGet.mockResolvedValue(httpRule());
    commandMocks.ruleEditorContext.mockResolvedValue(context);
    commandMocks.ruleDefinitionSave.mockImplementation(async (input) => ({ ...httpRule(), ...input.draft, revision: 4 }));
    commandMocks.ruleDefinitionHttpConditionDraft.mockResolvedValue(httpCondition);
    commandMocks.ruleDefinitionDocumentCommonActionDraft.mockResolvedValue({ source: "record_match" });
  });

  it("keeps the rule list and fixed editor visible in one workspace without an editor dialog", async () => {
    render(<RulesView />);

    expect(await screen.findByRole("button", { name: /HTTP combined/ })).toBeVisible();
    expect(screen.getByText("选择一条规则或新建规则进行编辑。")).toBeVisible();
    expect(screen.queryByRole("dialog", { name: /编辑规则/ })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /HTTP combined/ }));
    expect(await screen.findByDisplayValue("HTTP combined")).toBeVisible();
    expect(screen.getAllByTestId("condition-form")).toHaveLength(1);
    expect(screen.getAllByTestId("action-form")).toHaveLength(1);
  });

  it("starts rule creation inside the fixed editor without opening a dialog", async () => {
    const user = userEvent.setup();
    render(<RulesView />);

    await user.click(await screen.findByRole("button", { name: "新建规则" }));

    expect(screen.queryByRole("dialog", { name: "创建统一规则" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "新建规则" })).toBeVisible();
    expect(screen.getByRole("button", { name: /创建规则的 Listener/ })).toBeVisible();
  });

  it("keeps creation metadata and rule content in one continuous editor without an enter step", async () => {
    const user = userEvent.setup();
    render(<RulesView />);

    await user.click(await screen.findByRole("button", { name: "新建规则" }));
    const metadata = screen.getByTestId("rule-metadata-fields");
    await user.click(screen.getByRole("button", { name: /创建规则的 Listener/ }));
    await user.click(await screen.findByRole("option", { name: "HTTP Listener · HTTP" }));
    await user.click(within(metadata).getByRole("button", { name: /处理阶段/ }));
    await user.click(await screen.findByRole("option", { name: "Proxy → Server" }));

    expect(within(metadata).getByRole("textbox", { name: "规则名称" })).toHaveValue("");
    expect(await screen.findByRole("heading", { name: "HTTP 规则内容" })).toBeVisible();
    expect(screen.getByTestId("condition-form")).toBeVisible();
    expect(screen.getByTestId("action-form")).toBeVisible();
    expect(screen.getByRole("button", { name: "保存规则" })).toBeDisabled();

    await user.type(within(metadata).getByRole("textbox", { name: "规则名称" }), "Inline HTTP rule");
    const enabled = within(metadata).getByRole("switch", { name: "启用规则" });
    expect(enabled).not.toBeChecked();
    expect(screen.getAllByRole("switch", { name: "启用规则" })).toHaveLength(1);
    expect(screen.queryByRole("button", { name: /新规则是否启用/ })).not.toBeInTheDocument();
    await user.type(within(metadata).getByLabelText("阶段内优先级"), "10");
    await user.click(enabled);
    expect(enabled).toBeChecked();

    expect(screen.queryByRole("button", { name: /新规则是否单次/ })).not.toBeInTheDocument();
    expect(screen.queryByText(/是否单次|单次|持续/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "进入规则编辑器" })).not.toBeInTheDocument();
    expect(within(metadata).getByRole("textbox", { name: "规则名称" })).toHaveValue("Inline HTTP rule");
    await user.click(screen.getByRole("button", { name: /条件来源/ }));
    await user.click(await screen.findByRole("option", { name: "HTTP" }));
    await user.click(screen.getByRole("button", { name: /动作来源/ }));
    await user.click(await screen.findByRole("option", { name: "HTTP" }));
    expect(within(metadata).getByRole("textbox", { name: "规则名称" })).toHaveValue("Inline HTTP rule");
  });

  it("uses the same initially-disabled enable switch for Socket creation", async () => {
    const socketContext: RuleEditorContext = {
      listener_id: socketListener.id,
      local_document_types: [],
      document_condition_path: { wildcard_token: "*", wildcard_matches_exactly_one_level: true, multiple_matches_use_any: true },
      content: { type: "socket", value: { package: { id: "iso8583", version: "1.0.0" }, stages: [{
        stage: "proxy_to_app", document_fields: [], common_actions: ["record_match"],
        new_rule_draft: { listener_id: socketListener.id, stage: "proxy_to_app", content: { type: "socket", value: { package: { id: "iso8583", version: "1.0.0" } } } },
      }] } },
    };
    commandMocks.workspaceGet.mockResolvedValue({ id: "workspace", listeners: [socketListener] });
    commandMocks.ruleEditorContext.mockResolvedValue(socketContext);
    const user = userEvent.setup(); render(<RulesView />);

    await user.click(await screen.findByRole("button", { name: "新建规则" }));
    await user.click(screen.getByRole("button", { name: /创建规则的 Listener/ }));
    await user.click(await screen.findByRole("option", { name: "Socket Listener · Socket" }));
    const metadata = screen.getByTestId("rule-metadata-fields");
    await user.click(within(metadata).getByRole("button", { name: /处理阶段/ }));
    await user.click(await screen.findByRole("option", { name: "Proxy → App" }));

    const enabled = within(metadata).getByRole("switch", { name: "启用规则" });
    expect(enabled).not.toBeChecked();
    expect(screen.getAllByRole("switch", { name: "启用规则" })).toHaveLength(1);
    expect(screen.queryByRole("button", { name: /新规则是否启用|新规则是否单次/ })).not.toBeInTheDocument();
    expect(screen.queryByText(/是否单次|单次|持续/)).not.toBeInTheDocument();
  });

  it("shows both proxy directions in one list with a direction badge on each card", async () => {
    commandMocks.ruleDefinitionList.mockResolvedValue([
      httpRule({ rule_id: "upstream", name: "Request rule", stage: "proxy_to_upstream" }),
      httpRule({ rule_id: "app", name: "Response rule", stage: "proxy_to_app" }),
    ]);

    render(<RulesView />);

    expect(await screen.findByRole("button", { name: /Request rule.*上行/ })).toBeVisible();
    expect(screen.getByRole("button", { name: /Response rule.*下行/ })).toBeVisible();
    expect(screen.queryByTestId("rule-stage-heading")).not.toBeInTheDocument();
    expect(screen.queryByText("Proxy → Server")).not.toBeInTheDocument();
    expect(screen.queryByText("Proxy → App")).not.toBeInTheDocument();
  });

  it("materializes the single condition and action before saving", async () => {
    render(<RulesView />);
    await userEvent.click(await screen.findByRole("button", { name: /HTTP combined/ }));
    const save = await screen.findByRole("button", { name: "保存规则" });
    expect(save).toBeEnabled();
    await userEvent.click(save);

    await waitFor(() => expect(commandMocks.ruleDefinitionHttpConditionDraft).toHaveBeenCalledWith("request_target", null, "equals", "/", "proxy_to_upstream"));
    expect(commandMocks.ruleDefinitionDocumentCommonActionDraft).toHaveBeenCalledWith("record_match");
    await waitFor(() => expect(commandMocks.ruleDefinitionSave).toHaveBeenCalledWith(expect.objectContaining({
      draft: expect.objectContaining({ content: { type: "http", value: expect.objectContaining({ condition: httpCondition, action: { source: "record_match" } }) } }),
    })));
  });

  it("ignores an older rule selection response after a newer selection starts", async () => {
    const second: RuleDefinition_Serialize = { ...httpRule(), rule_id: "second", name: "Second" };
    commandMocks.ruleDefinitionList.mockResolvedValue([httpRule(), second]);
    let finishFirst!: (rule: RuleDefinition_Serialize) => void;
    commandMocks.ruleDefinitionGet
      .mockReturnValueOnce(new Promise((resolve) => { finishFirst = resolve; }))
      .mockResolvedValueOnce(second);
    render(<RulesView />);

    await userEvent.click(await screen.findByRole("button", { name: /HTTP combined/ }));
    await userEvent.click(screen.getByRole("button", { name: /Second/ }));
    expect(await screen.findByDisplayValue("Second")).toBeVisible();
    await act(async () => { finishFirst(httpRule()); await Promise.resolve(); });
    expect(screen.getByDisplayValue("Second")).toBeVisible();
  });
});
