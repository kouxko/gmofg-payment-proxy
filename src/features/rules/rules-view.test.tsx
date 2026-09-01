// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RuleDefinition_Serialize, RuleEditorContext } from "@/generated/rust-types";
import { RulesView } from "./rules-view";
import { httpCondition, httpListener, httpRule } from "./rules-view-test-fixtures";

const commandMocks = vi.hoisted(() => ({
  workspaceList: vi.fn(), workspaceGet: vi.fn(), ruleDefinitionList: vi.fn(),
  ruleDefinitionGet: vi.fn(), ruleEditorContext: vi.fn(), ruleDefinitionSave: vi.fn(),
  ruleDefinitionToggle: vi.fn(), ruleDefinitionDelete: vi.fn(), ruleDefinitionCopy: vi.fn(),
  ruleDefinitionCreateFromExchangeObservation: vi.fn(), ruleDefinitionHttpConditionDraft: vi.fn(),
  ruleDefinitionNthHitConditionDraft: vi.fn(), ruleDefinitionActionDraft: vi.fn(),
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
    ], actions: [] },
    package: null, document_fields: [], document_common_actions: ["record_match"],
    new_rule_draft: { listener_id: httpListener.id, stage: "proxy_to_upstream", content: httpRule().content },
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
  });

  it("keeps the rule list and fixed editor visible in one workspace without an editor dialog", async () => {
    render(<RulesView />);

    expect(await screen.findByRole("button", { name: /HTTP combined/ })).toBeVisible();
    expect(screen.getByText("选择一条规则或新建规则进行编辑。")).toBeVisible();
    expect(screen.queryByRole("dialog", { name: /编辑规则/ })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /HTTP combined/ }));
    expect(await screen.findByDisplayValue("HTTP combined")).toBeVisible();
    expect(screen.getByText("所有条件固定为 AND；需要 OR 时请新建多条规则。")).toBeVisible();
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

  it("saves the flat conditions array unchanged", async () => {
    render(<RulesView />);
    await userEvent.click(await screen.findByRole("button", { name: /HTTP combined/ }));
    const save = await screen.findByRole("button", { name: "保存规则" });
    expect(save).toBeEnabled();
    await userEvent.click(save);

    await waitFor(() => expect(commandMocks.ruleDefinitionSave).toHaveBeenCalledWith(expect.objectContaining({
      draft: expect.objectContaining({ content: { type: "http", value: expect.objectContaining({ conditions: [httpCondition] }) } }),
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
