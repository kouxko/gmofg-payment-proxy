// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FaultTemplateViewModel } from "@/generated/rust-types";
import { FaultsView } from "./faults-view";

const commandMocks = vi.hoisted(() => ({
  faultTemplateList: vi.fn(),
  faultActiveList: vi.fn(),
  faultConfigure: vi.fn(),
  faultStop: vi.fn(),
}));

const refreshMocks = vi.hoisted(() => ({
  templates: vi.fn(),
  active: vi.fn(),
}));

const workspaceNavigationMocks = vi.hoisted(() => ({
  navigate: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: commandMocks,
}));

vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceNavigation: () => ({
    pathname: "/faults",
    searchParams: new URLSearchParams(),
    navigate: workspaceNavigationMocks.navigate,
  }),
}));

vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: () => "error",
}));

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) =>
    key === "fault-template-list"
      ? { data: templates, refresh: refreshMocks.templates }
      : { data: [], refresh: refreshMocks.active },
}));

vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: vi.fn(),
  useBootstrap: () => ({
    bootstrap: {
      channel_catalog: [
        { id: "transaction", display_name: "交易" },
        { id: "dll", display_name: "Payment DLL" },
      ],
    },
  }),
}));

const templates: FaultTemplateViewModel[] = [
  {
    template_id: "mock_shift_jis_json",
    name: "Mock Shift-JIS JSON",
    stage_text: "请求阶段",
    behavior_text: "绕过上游并返回 Mock",
    affected_party_text: "Payment App",
    default_channel: "transaction",
    default_nth_hit: 1,
    default_one_shot: false,
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
        label: "Shift-JIS JSON Body",
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

describe("FaultsView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    commandMocks.faultConfigure.mockResolvedValue({
      rule_id: "rule-1",
      template_name: "Mock Shift-JIS JSON",
      target_summary: "全部请求",
      priority: 100,
      hit_count: 0,
      enabled: true,
      status_text: "活动中",
      ui_tone: "warning",
      revision: 1,
    });
  });

  it("renders schema-driven fields and submits tagged typed defaults", async () => {
    const user = userEvent.setup();
    render(<FaultsView />);

    expect(
      screen.getByRole("textbox", { name: "HTTP 状态码" }),
    ).toHaveValue("200");
    expect(
      screen.getByRole("textbox", { name: "Shift-JIS JSON Body" }),
    ).toHaveValue("{}");
    expect(screen.getByLabelText("代理通道")).toBeInTheDocument();
    await user.click(screen.getByLabelText("代理通道"));
    expect(
      await screen.findByRole("option", { name: "Payment DLL" }),
    ).toBeVisible();
    await user.click(screen.getByRole("option", { name: "交易" }));

    await user.click(screen.getByRole("button", { name: "启用模拟" }));

    expect(commandMocks.faultConfigure).toHaveBeenCalledWith(
      expect.objectContaining({
        template_id: "mock_shift_jis_json",
        channel: "transaction",
        parameters: {
          status: { kind: "integer", value: 200 },
          body: { kind: "json", value: "{}" },
        },
      }),
    );
  });

  it("submits the explicitly selected DLL channel", async () => {
    const user = userEvent.setup();
    render(<FaultsView />);

    await user.click(screen.getByLabelText("代理通道"));
    await user.click(
      await screen.findByRole("option", { name: "Payment DLL" }),
    );
    await user.click(screen.getByRole("button", { name: "启用模拟" }));

    expect(commandMocks.faultConfigure).toHaveBeenCalledWith(
      expect.objectContaining({
        template_id: "mock_shift_jis_json",
        channel: "dll",
      }),
    );
  });

  it("opens the saved rule through client navigation", async () => {
    const user = userEvent.setup();
    render(<FaultsView />);

    await user.click(screen.getByRole("button", { name: "保存为规则" }));

    expect(workspaceNavigationMocks.navigate).toHaveBeenCalledWith("/rules");
  });
});
