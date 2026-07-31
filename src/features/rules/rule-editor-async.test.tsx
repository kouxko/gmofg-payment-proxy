// @vitest-environment jsdom

/** 验证异步 Rust 草稿请求的代次淘汰、保存门禁与并发编辑保护。 */

import "@testing-library/jest-dom/vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RuleDraft } from "@/generated/rust-types";
import { ActionsEditor, ConditionsEditor } from "./rule-editor";

const commandMocks = vi.hoisted(() => ({
  ruleConditionDraft: vi.fn(),
  ruleParseHeaderInput: vi.fn(),
  ruleParseByteInput: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: commandMocks,
}));

vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: () => "Rust 校验失败",
}));

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
        body_bytes: [123, 125],
      },
    },
  ],
  one_shot: false,
};

describe("RULE-016/RULE-017 production rule editor async safety", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    commandMocks.ruleParseByteInput.mockResolvedValue({
      bytes: [123, 125],
      normalized: "123, 125",
    });
  });

  it("reports Rust parsing as pending and ignores an older response", async () => {
    let finishFirst!: (value: unknown) => void;
    let finishSecond!: (value: unknown) => void;
    commandMocks.ruleParseHeaderInput
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
    let currentDraft = draft;
    const onChange = vi.fn((change: RuleDraft | ((value: RuleDraft) => RuleDraft)) => {
      currentDraft =
        typeof change === "function" ? change(currentDraft) : change;
    });
    const onAsyncStateChange = vi.fn();
    render(
      <ActionsEditor
        draft={draft}
        fieldErrors={{}}
        onChange={onChange}
        onAsyncStateChange={onAsyncStateChange}
      />,
    );

    const headers = screen.getByRole("textbox", {
      name: "响应 Header（每行 name: value）",
    });
    fireEvent.change(headers, { target: { value: "x-first: 1" } });
    fireEvent.change(headers, { target: { value: "x-second: 2" } });
    expect(onAsyncStateChange).toHaveBeenCalledWith("rule-1:0:headers", {
      pending: true,
      invalid: false,
    });

    finishSecond({
      headers: [["x-second", "2"]],
      normalized: "x-second: 2",
    });
    await waitFor(() =>
      expect(currentDraft.actions[0]).toEqual(
        expect.objectContaining({
          type: "terminal",
          action: expect.objectContaining({
            headers: [["x-second", "2"]],
          }),
        }),
      ),
    );

    finishFirst({
      headers: [["x-first", "1"]],
      normalized: "x-first: 1",
    });
    await Promise.resolve();
    expect(currentDraft.actions[0]).toEqual(
      expect.objectContaining({
        type: "terminal",
        action: expect.objectContaining({
          headers: [["x-second", "2"]],
        }),
      }),
    );
  });

  it("merges Header and Body parser results into the latest action", async () => {
    let finishHeaders!: (value: unknown) => void;
    let finishBytes!: (value: unknown) => void;
    commandMocks.ruleParseHeaderInput.mockReturnValue(
      new Promise((resolve) => {
        finishHeaders = resolve;
      }),
    );
    commandMocks.ruleParseByteInput.mockReturnValue(
      new Promise((resolve) => {
        finishBytes = resolve;
      }),
    );
    let currentDraft = draft;
    const onChange = (
      change: RuleDraft | ((value: RuleDraft) => RuleDraft),
    ) => {
      currentDraft =
        typeof change === "function" ? change(currentDraft) : change;
    };
    render(
      <ActionsEditor
        draft={draft}
        fieldErrors={{}}
        onChange={onChange}
        onAsyncStateChange={vi.fn()}
      />,
    );

    const user = userEvent.setup();
    const status = screen.getByRole("textbox", { name: "HTTP 状态码" });
    await user.clear(status);
    await user.type(status, "503");
    fireEvent.blur(status);
    fireEvent.change(
      screen.getByRole("textbox", {
        name: "响应 Header（每行 name: value）",
      }),
      { target: { value: "x-latest: 2" } },
    );
    fireEvent.change(screen.getByLabelText("Shift-JIS Body 字节"), {
      target: { value: "65, 66" },
    });

    finishBytes({ bytes: [65, 66], normalized: "65, 66" });
    finishHeaders({
      headers: [["x-latest", "2"]],
      normalized: "x-latest: 2",
    });

    await waitFor(() =>
      expect(currentDraft.actions[0]).toEqual({
        type: "terminal",
        action: {
          type: "mock_response",
          status: 503,
          headers: [["x-latest", "2"]],
          body_bytes: [65, 66],
        },
      }),
    );
  });

  it.each([
    ["current", "删除条件 2", "terminal_ip"],
    ["preceding", "删除条件 1", "certificate_fingerprint"],
  ])(
    "discards a condition draft response after deleting the %s row",
    async (_, deleteLabel, remainingField) => {
      let finishDraft!: (value: unknown) => void;
      commandMocks.ruleConditionDraft.mockReturnValue(
        new Promise((resolve) => {
          finishDraft = resolve;
        }),
      );
      const conditionDraft: RuleDraft = {
        ...draft,
        conditions: [
          {
            type: "field",
            field: { type: "terminal_ip" },
            operator: { type: "equals", value: "10.0.0.1" },
          },
          {
            type: "field",
            field: { type: "certificate_fingerprint" },
            operator: { type: "contains", value: "abc" },
          },
        ],
      };
      let currentDraft = conditionDraft;
      function Harness() {
        const [value, setValue] = useState(conditionDraft);
        currentDraft = value;
        return (
          <ConditionsEditor
            draft={value}
            fieldErrors={{}}
            onChange={(change) =>
              setValue((current) =>
                typeof change === "function" ? change(current) : change,
              )
            }
            onAsyncStateChange={vi.fn()}
          />
        );
      }
      const user = userEvent.setup();
      render(<Harness />);

      await user.click(screen.getByLabelText("条件 2 类型"));
      await user.click(
        await screen.findByRole("option", { name: "第 N 次命中" }),
      );
      await user.click(screen.getByRole("button", { name: deleteLabel }));
      await waitFor(() => expect(currentDraft.conditions).toHaveLength(1));

      await act(async () => {
        finishDraft({ type: "nth_hit", count: 9 });
        await Promise.resolve();
      });

      expect(currentDraft.conditions).toHaveLength(1);
      expect(currentDraft.conditions[0]).toEqual(
        expect.objectContaining({
          type: "field",
          field: expect.objectContaining({ type: remainingField }),
        }),
      );
    },
  );
});
