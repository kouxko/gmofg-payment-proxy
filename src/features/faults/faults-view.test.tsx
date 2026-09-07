// @vitest-environment jsdom

/** 验证故障模板选择和参数提交，不在前端重复测试故障引擎本身。 */

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FaultTemplateViewModel } from "@/generated/rust-types";
import { FaultPresetsView } from "./faults-view";

const commandMocks = vi.hoisted(() => ({
  faultTemplateList: vi.fn(),
  faultConfigure: vi.fn(),
}));

const refreshMocks = vi.hoisted(() => ({
  templates: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: commandMocks,
}));

vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: () => "error",
}));

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: () => ({
    data: templates,
    error: undefined,
    isLoading: false,
    refresh: refreshMocks.templates,
  }),
}));

vi.mock("@/features/shell/bootstrap-context", () => ({
  useBootstrap: () => ({
    bootstrap: {
      channel_catalog: [
        { id: "api-primary", display_name: "主接口" },
        { id: "api-secondary", display_name: "辅助接口" },
      ],
    },
  }),
}));

const templates: FaultTemplateViewModel[] = [
  {
    template_id: "mock_json",
    name: "Mock JSON",
    stage_text: "请求阶段",
    behavior_text: "绕过上游并返回 Mock",
    affected_party_text: "客户端",
    default_channel: "api-primary",
    default_priority: 100,
    default_parameters: {
      status: { kind: "integer", value: 200 },
      body: { kind: "json", value: "{}" },
    },
    parameter_schema: [
      {
        key: "status",
        label: "HTTP 状态码",
        description: "Mock 响应的 HTTP 状态码。",
        kind: "integer",
        required: true,
        minimum: 100,
        maximum: 599,
        multiline: false,
      },
      {
        key: "body",
        label: "JSON Body",
        description: "必须是合法 JSON。",
        kind: "json",
        required: true,
        minimum: null,
        maximum: null,
        multiline: true,
      },
    ],
    risk_text: "高",
    ui_tone: "danger",
  },
];

describe("HTTP fault presets", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    commandMocks.faultConfigure.mockResolvedValue({
      rule_id: "rule-1",
      template_name: "Mock JSON",
      target_summary: "全部请求",
      priority: 100,
      hit_count: 0,
      enabled: true,
      status_text: "活动中",
      ui_tone: "warning",
      revision: 1,
    });
  });

  it("selects the first template by default and uses the row as the configuration action", () => {
    render(<FaultPresetsView />);

    expect(
      screen.getByRole("row", { name: /Mock JSON/ }),
    ).toHaveAttribute("aria-selected", "true");
    expect(
      screen.getByRole("heading", {
        name: "配置模板：Mock JSON",
      }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "配置" }),
    ).not.toBeInTheDocument();
  });

  it("renders schema-driven fields and submits tagged typed defaults", async () => {
    const user = userEvent.setup();
    render(<FaultPresetsView />);

    expect(
      screen.getByRole("textbox", { name: "HTTP 状态码" }),
    ).toHaveValue("200");
    expect(
      screen.getByRole("textbox", { name: "JSON Body" }),
    ).toHaveValue("{}");
    expect(screen.getByLabelText("代理通道")).toBeInTheDocument();
    expect(screen.queryByText("第 N 次命中")).not.toBeInTheDocument();
    await user.click(screen.getByLabelText("代理通道"));
    expect(
      await screen.findByRole("option", { name: "辅助接口" }),
    ).toBeVisible();
    await user.click(screen.getByRole("option", { name: "主接口" }));

    await user.click(screen.getByRole("button", { name: "创建故障规则" }));

    expect(commandMocks.faultConfigure).toHaveBeenCalledWith(
      expect.objectContaining({
        template_id: "mock_json",
        channel: "api-primary",
        parameters: {
          status: { kind: "integer", value: 200 },
          body: { kind: "json", value: "{}" },
        },
      }),
    );
    expect(commandMocks.faultConfigure.mock.calls[0]?.[0]).not.toHaveProperty("nth_hit");
  });

  it("submits the explicitly selected secondary channel", async () => {
    const user = userEvent.setup();
    render(<FaultPresetsView />);

    await user.click(screen.getByLabelText("代理通道"));
    await user.click(
      await screen.findByRole("option", { name: "辅助接口" }),
    );
    await user.click(screen.getByRole("button", { name: "创建故障规则" }));

    expect(commandMocks.faultConfigure).toHaveBeenCalledWith(
      expect.objectContaining({
        template_id: "mock_json",
        channel: "api-secondary",
      }),
    );
  });

  it("returns the created ordinary rule identity to the rules workspace", async () => {
    const user = userEvent.setup();
    const onRuleCreated = vi.fn();
    render(<FaultPresetsView onRuleCreated={onRuleCreated} />);

    await user.click(screen.getByRole("button", { name: "创建故障规则" }));

    expect(onRuleCreated).toHaveBeenCalledWith("rule-1");
    expect(
      screen.queryByRole("heading", { name: "当前生效的故障预设" }),
    ).not.toBeInTheDocument();
  });
});
