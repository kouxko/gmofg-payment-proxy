// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  RuleDraft,
  RuleSummaryViewModel,
} from "@/generated/rust-types";
import { RulesView } from "./rules-view";

const commandMocks = vi.hoisted(() => ({
  ruleParseHeaderInput: vi.fn(),
  ruleParseByteInput: vi.fn(),
  ruleActionDraft: vi.fn(),
  ruleSave: vi.fn(),
}));
const queryMocks = vi.hoisted(() => ({
  listRefresh: vi.fn(),
  detailRefresh: vi.fn(),
  detailInvalidate: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: commandMocks,
}));

vi.mock("@/lib/ipc/client", () => ({
  appErrorViewModel: () => undefined,
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: () => "Rust 操作失败",
}));

vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: vi.fn(),
}));

vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceNavigation: () => ({
    pathname: "/rules",
    searchParams: new URLSearchParams(),
    navigate: vi.fn(),
  }),
}));

const summary: RuleSummaryViewModel = {
  rule_id: "rule-1",
  revision: 1,
  name: "Mock",
  enabled: true,
  priority: 10,
  creation_order: 1,
  channel_text: "交易",
  stage_text: "请求",
  match_summary: "全部",
  action_summary: "Mock 响应",
  hit_count: 0,
  last_hit_at: null,
  ui_tone: "positive",
};

const draft: RuleDraft = {
  rule_id: "rule-1",
  expected_revision: 1,
  name: "Mock",
  description: "",
  enabled: true,
  priority: 10,
  channel: "transaction",
  stage: "request",
  conditions: [],
  actions: [
    {
      type: "terminal",
      action: {
        type: "mock_response",
        status: 200,
        headers: [],
        shift_jis_body: [123, 125],
      },
    },
  ],
  one_shot: false,
};
const ruleView = { summary, draft };

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) =>
    key === "rule-list"
      ? {
          data: [summary],
          error: undefined,
          isLoading: false,
          refresh: queryMocks.listRefresh,
        }
      : {
          data: ruleView,
          error: undefined,
          isLoading: false,
          refresh: queryMocks.detailRefresh,
          invalidate: queryMocks.detailInvalidate,
        },
}));

describe("production RulesView async save guard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    queryMocks.listRefresh.mockResolvedValue(undefined);
    commandMocks.ruleParseByteInput.mockResolvedValue({
      bytes: [123, 125],
      normalized: "123, 125",
    });
  });

  it("does not save an old draft while the latest Rust parser is pending", async () => {
    let finishParse!: (value: unknown) => void;
    commandMocks.ruleParseHeaderInput.mockReturnValue(
      new Promise((resolve) => {
        finishParse = resolve;
      }),
    );
    commandMocks.ruleSave.mockImplementation(async (next: RuleDraft) => ({
      summary,
      draft: next,
    }));
    const user = userEvent.setup();
    render(<RulesView />);

    await user.click(await screen.findByRole("tab", { name: "执行动作" }));
    const headers = await screen.findByRole("textbox", {
      name: "响应 Header（每行 name: value）",
    });
    fireEvent.change(headers, {
      target: { value: "x-latest: 2" },
    });

    expect(
      await screen.findByRole("button", {
        name: "等待 Rust 解析输入",
      }),
    ).toBeDisabled();
    expect(commandMocks.ruleSave).not.toHaveBeenCalled();

    finishParse({
      headers: [["x-latest", "2"]],
      normalized: "x-latest: 2",
    });
    const save = await screen.findByRole("button", { name: "保存规则" });
    await waitFor(() => expect(save).toBeEnabled());
    await user.click(save);

    expect(commandMocks.ruleSave).toHaveBeenCalledWith(
      expect.objectContaining({
        actions: [
          {
            type: "terminal",
            action: expect.objectContaining({
              headers: [["x-latest", "2"]],
            }),
          },
        ],
      }),
    );
  });

  it("blocks save and ignores an older Rust action draft response", async () => {
    let finishFirst!: (value: unknown) => void;
    let finishSecond!: (value: unknown) => void;
    commandMocks.ruleActionDraft
      .mockReturnValueOnce(
        new Promise((resolve) => {
          finishFirst = resolve;
        }),
      )
      .mockReturnValueOnce(
        new Promise((resolve) => {
          finishSecond = resolve;
        }),
      );
    commandMocks.ruleSave.mockImplementation(async (next: RuleDraft) => ({
      summary,
      draft: next,
    }));
    const user = userEvent.setup();
    render(<RulesView />);

    await user.click(await screen.findByRole("tab", { name: "执行动作" }));
    await screen.findByRole("textbox", {
      name: "响应 Header（每行 name: value）",
    });
    const actionType = await screen.findByLabelText("动作类型");
    await user.click(actionType);
    await user.click(await screen.findByRole("option", { name: "延迟" }));
    await user.click(actionType);
    await user.click(await screen.findByRole("option", { name: "暂停并进入断点" }));

    expect(
      await screen.findByRole("button", {
        name: "等待 Rust 解析输入",
      }),
    ).toBeDisabled();
    expect(commandMocks.ruleSave).not.toHaveBeenCalled();

    finishSecond({ type: "pause" });
    const save = await screen.findByRole("button", { name: "保存规则" });
    await waitFor(() => expect(save).toBeEnabled());
    finishFirst({ type: "delay", milliseconds: 999 });
    await Promise.resolve();
    await user.click(save);

    expect(commandMocks.ruleSave).toHaveBeenCalledWith(
      expect.objectContaining({
        actions: [{ type: "pause" }],
      }),
    );
  });
});
