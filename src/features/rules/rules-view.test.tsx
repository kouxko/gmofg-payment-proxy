// @vitest-environment jsdom

/** 验证规则列表、默认首选、启停/一次性开关、保存、复制与删除交互。 */

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  RuleDraft,
  RuleStageCapabilityViewModel,
  RuleSummaryViewModel,
} from "@/generated/rust-types";
import { RulesView } from "./rules-view";

const commandMocks = vi.hoisted(() => ({
  ruleParseHeaderInput: vi.fn(),
  ruleParseByteInput: vi.fn(),
  ruleConditionDraft: vi.fn(),
  ruleActionDraft: vi.fn(),
  ruleSave: vi.fn(),
  ruleToggle: vi.fn(),
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
  useBootstrap: () => ({
    bootstrap: {
      channel_catalog: [
        { id: "alpha", display_name: "Alpha" },
        { id: "beta", display_name: "Beta" },
        { id: "gamma", display_name: "Gamma" },
      ],
    },
  }),
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
  channel_text: "Alpha",
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
  channel: "alpha",
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
const ruleView = { summary, draft };
const capabilities: RuleStageCapabilityViewModel[] = [
  {
    stage: "request",
    match_field_kinds: [
      "terminal_ip",
      "certificate_fingerprint",
      "path_or_request_type",
      "json_path",
    ],
    actions: [
      { kind: "set_json_field", terminal: false, traffic_direction: null },
      { kind: "throttle", terminal: false, traffic_direction: "upstream" },
      { kind: "delay", terminal: false, traffic_direction: null },
      { kind: "pause", terminal: false, traffic_direction: null },
      { kind: "mock_response", terminal: true, traffic_direction: null },
    ],
  },
  {
    stage: "response",
    match_field_kinds: [
      "terminal_ip",
      "certificate_fingerprint",
      "path_or_request_type",
      "json_path",
    ],
    actions: [
      { kind: "set_json_field", terminal: false, traffic_direction: null },
      { kind: "throttle", terminal: false, traffic_direction: "downstream" },
      { kind: "delay", terminal: false, traffic_direction: null },
      { kind: "custom_http_status", terminal: false, traffic_direction: null },
      { kind: "invalid_json", terminal: true, traffic_direction: null },
    ],
  },
  {
    stage: "tls_handshake",
    match_field_kinds: ["certificate_fingerprint"],
    actions: [
      { kind: "reject_tls_handshake", terminal: true, traffic_direction: null },
    ],
  },
];

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) =>
    key === "rule-list"
      ? {
          data: [summary],
          error: undefined,
          isLoading: false,
          refresh: queryMocks.listRefresh,
        }
      : key === "rule-capabilities"
        ? {
            data: capabilities,
            error: undefined,
            isLoading: false,
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
    commandMocks.ruleConditionDraft.mockResolvedValue({
      type: "field",
      field: { type: "certificate_fingerprint" },
      operator: { type: "equals", value: "" },
    });
    commandMocks.ruleActionDraft.mockImplementation(
      async (kind: string, stage: string) => {
        if (kind === "throttle") {
          return {
            type: "throttle",
            bytes_per_second: 1024,
            chunk_bytes: 256,
            direction: stage === "response" ? "downstream" : "upstream",
          };
        }
        return { type: kind };
      },
    );
  });

  it("toggles one-shot by clicking the HeroUI switch control", async () => {
    commandMocks.ruleSave.mockImplementation(async (next: RuleDraft) => ({
      summary,
      draft: next,
    }));
    const user = userEvent.setup();
    render(<RulesView />);

    const oneShot = await screen.findByRole("switch", {
      name: "仅命中一次",
    });
    expect(oneShot).not.toBeChecked();

    const oneShotContent = oneShot.closest('[data-slot="switch-content"]');
    const oneShotControl = oneShotContent?.querySelector<HTMLElement>(
      '[data-slot="switch-control"]',
    );
    expect(oneShotControl).toBeTruthy();
    expect(oneShotContent).toContainElement(oneShotControl!);
    await user.click(oneShot);

    expect(oneShot).toBeChecked();
    await user.click(screen.getByRole("button", { name: "保存规则" }));
    expect(commandMocks.ruleSave).toHaveBeenCalledWith(
      expect.objectContaining({ one_shot: true }),
    );
  });

  it("does not offer response-only actions while editing a request-stage rule", async () => {
    const user = userEvent.setup();
    render(<RulesView />);

    await user.click(await screen.findByRole("tab", { name: "执行动作" }));
    await user.click(await screen.findByLabelText("动作类型"));

    expect(
      screen.queryByRole("option", { name: "自定义 HTTP 状态码" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("option", { name: "非法 JSON 响应" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: "Mock 响应" }),
    ).toBeVisible();
    await user.keyboard("{Escape}");
    expect(screen.getByRole("button", { name: "添加动作" })).toBeDisabled();
  });

  it("switches to the Rust response capability without exposing request terminals", async () => {
    const user = userEvent.setup();
    render(<RulesView />);

    await user.click(await screen.findByLabelText("规则阶段"));
    await user.click(await screen.findByRole("option", { name: "响应" }));
    await user.click(await screen.findByRole("tab", { name: "执行动作" }));
    await user.click(await screen.findByLabelText("动作类型"));

    expect(
      screen.getByRole("option", { name: "自定义 HTTP 状态码" }),
    ).toBeVisible();
    expect(
      screen.queryByRole("option", { name: "Mock 响应" }),
    ).not.toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(
      screen.getByText(
        "当前动作不支持所选阶段或所在位置，请改为下拉框中的可用动作。",
      ),
    ).toBeVisible();
  });

  it("offers only certificate matching in the TLS stage", async () => {
    const user = userEvent.setup();
    render(<RulesView />);

    await user.click(await screen.findByLabelText("规则阶段"));
    await user.click(await screen.findByRole("option", { name: "TLS 握手" }));
    await user.click(await screen.findByRole("tab", { name: "匹配条件" }));
    await user.click(await screen.findByRole("button", { name: "添加条件" }));
    await user.click(await screen.findByLabelText("匹配字段"));

    expect(
      screen.getByRole("option", { name: "证书指纹" }),
    ).toBeVisible();
    expect(screen.queryByRole("option", { name: "JSON Path" })).not.toBeInTheDocument();
    expect(commandMocks.ruleConditionDraft).toHaveBeenCalledWith(
      "field",
      "tls_handshake",
    );
  });

  it("uses the stage-fixed traffic direction instead of offering an invalid choice", async () => {
    const user = userEvent.setup();
    render(<RulesView />);

    await user.click(await screen.findByRole("tab", { name: "执行动作" }));
    await user.click(await screen.findByLabelText("动作类型"));
    await user.click(await screen.findByRole("option", { name: "带宽限速" }));

    expect(
      await screen.findByText("流量方向由阶段固定：上行 Proxy → Server"),
    ).toBeVisible();
    expect(screen.queryByLabelText("流量方向")).not.toBeInTheDocument();
    expect(commandMocks.ruleActionDraft).toHaveBeenCalledWith(
      "throttle",
      "request",
    );
  });

  it("toggles the draft enabled state by clicking its visible HeroUI control", async () => {
    commandMocks.ruleSave.mockImplementation(async (next: RuleDraft) => ({
      summary,
      draft: next,
    }));
    const user = userEvent.setup();
    render(<RulesView />);

    const enabled = await screen.findByRole("switch", {
      name: "启用规则",
    });
    expect(enabled).toBeChecked();

    const enabledContent = enabled.closest('[data-slot="switch-content"]');
    const enabledControl = enabledContent?.querySelector<HTMLElement>(
      '[data-slot="switch-control"]',
    );
    expect(enabledControl).toBeTruthy();
    expect(enabledContent).toContainElement(enabledControl!);
    await user.click(enabled);

    expect(enabled).not.toBeChecked();
    await user.click(screen.getByRole("button", { name: "保存规则" }));
    expect(commandMocks.ruleSave).toHaveBeenCalledWith(
      expect.objectContaining({ enabled: false }),
    );
  });

  it("toggles a saved rule by clicking the table switch control", async () => {
    commandMocks.ruleToggle.mockResolvedValue({
      summary: { ...summary, enabled: false, revision: 2 },
      draft: { ...draft, enabled: false, expected_revision: 2 },
    });
    const user = userEvent.setup();
    render(<RulesView />);

    const tableSwitch = await screen.findByRole("switch", {
      name: "停用规则 Mock",
    });
    const tableContent = tableSwitch.closest('[data-slot="switch-content"]');
    const tableControl = tableContent?.querySelector<HTMLElement>(
      '[data-slot="switch-control"]',
    );
    expect(tableControl).toBeTruthy();
    expect(tableContent).toContainElement(tableControl!);
    await user.click(tableSwitch);

    expect(commandMocks.ruleToggle).toHaveBeenCalledWith("rule-1", 1, false);
    expect(queryMocks.listRefresh).toHaveBeenCalled();
  });

  it("renders every generic product channel from the Rust bootstrap catalog", async () => {
    const user = userEvent.setup();
    render(<RulesView />);

    await user.click(await screen.findByLabelText("规则通道"));

    expect(await screen.findByRole("option", { name: "Alpha" })).toBeVisible();
    expect(screen.getByRole("option", { name: "Beta" })).toBeVisible();
    expect(screen.getByRole("option", { name: "Gamma" })).toBeVisible();
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
        name: "正在解析输入",
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
        name: "正在解析输入",
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
