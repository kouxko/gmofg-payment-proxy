/** 规则编辑器的 DTO 适配纯函数测试；规则合法性最终仍由 Rust 验证。 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RuleAction, RuleCondition } from "@/generated/rust-types";
import {
  actionKind,
  parseRuleByteInput,
  parseRuleHeaderInput,
  requestActionDraft,
  requestConditionDraft,
  requestMatchFieldDraft,
  requestMatchOperatorDraft,
} from "./rule-editor";

const commandMocks = vi.hoisted(() => ({
  ruleConditionDraft: vi.fn(),
  ruleActionDraft: vi.fn(),
  ruleMatchFieldDraft: vi.fn(),
  ruleMatchOperatorDraft: vi.fn(),
  ruleParseByteInput: vi.fn(),
  ruleParseHeaderInput: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: commandMocks,
}));

vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: () => "Rust 校验失败",
}));

describe("RULE-001 Rust-owned rule editor drafts", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("requests condition and action defaults from Rust", async () => {
    const condition: RuleCondition = { type: "nth_hit", count: 1 };
    const action: RuleAction = { type: "delay", milliseconds: 0 };
    commandMocks.ruleConditionDraft.mockResolvedValue(condition);
    commandMocks.ruleActionDraft.mockResolvedValue(action);

    await expect(requestConditionDraft("nth_hit")).resolves.toBe(condition);
    await expect(requestActionDraft("delay")).resolves.toBe(action);
    expect(commandMocks.ruleConditionDraft).toHaveBeenCalledWith("nth_hit");
    expect(commandMocks.ruleActionDraft).toHaveBeenCalledWith("delay");
  });

  it("requests field and operator defaults from Rust", async () => {
    commandMocks.ruleMatchFieldDraft.mockResolvedValue({
      type: "json_path",
      path: "$",
    });
    commandMocks.ruleMatchOperatorDraft.mockResolvedValue({
      type: "regex",
      pattern: "",
    });

    await expect(requestMatchFieldDraft("json_path")).resolves.toEqual({
      type: "json_path",
      path: "$",
    });
    await expect(requestMatchOperatorDraft("regex")).resolves.toEqual({
      type: "regex",
      pattern: "",
    });
  });

  it("maps nonterminal and response-corruption action discriminators", () => {
    expect(actionKind({ type: "delay", milliseconds: 25 })).toBe("delay");
    expect(
      actionKind({
        type: "terminal",
        action: { type: "incorrect_content_length", delta: 1 },
      }),
    ).toBe("incorrect_content_length");
    expect(
      actionKind({
        type: "terminal",
        action: { type: "truncate_response", bytes: 2 },
      }),
    ).toBe("truncate_response");
  });

  it("sends raw byte text to Rust without a TypeScript fallback", async () => {
    commandMocks.ruleParseByteInput.mockResolvedValue({
      bytes: [130, 160],
      normalized: "130, 160",
    });

    await expect(parseRuleByteInput("130, 160")).resolves.toEqual({
      bytes: [130, 160],
      normalized: "130, 160",
    });
    expect(commandMocks.ruleParseByteInput).toHaveBeenCalledWith("130, 160");
  });

  it("sends raw response headers to Rust without TypeScript parsing", async () => {
    commandMocks.ruleParseHeaderInput.mockResolvedValue({
      headers: [["content-type", "application/json"]],
      normalized: "content-type: application/json",
    });

    await expect(
      parseRuleHeaderInput("Content-Type: application/json"),
    ).resolves.toEqual({
      headers: [["content-type", "application/json"]],
      normalized: "content-type: application/json",
    });
    expect(commandMocks.ruleParseHeaderInput).toHaveBeenCalledWith(
      "Content-Type: application/json",
    );
  });
});
