// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProxyStatusViewModel } from "@/generated/rust-types";
import { ConsoleView } from "./console-view";

const commandMocks = vi.hoisted(() => ({
  proxyStart: vi.fn(),
  proxyStop: vi.fn(),
  proxyRestart: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: commandMocks,
}));

const status: ProxyStatusViewModel = {
  state: "running",
  state_text: "运行中",
  ui_tone: "positive",
  runtime_epoch: "epoch-1",
  revision: 7,
  channels: [
    {
      id: "transaction",
      display_name: "交易通道",
      state: "listening",
      state_text: "正在监听",
      ui_tone: "positive",
      listen_address: "0.0.0.0:16627",
      mtls_enabled: true,
      connected_clients: 8,
      request_count: 2354,
      error_count: 3,
      enabled: true,
      upstream_url: "https://transaction.example.test",
      upstream_state_text: "最近转发成功",
      upstream_ui_tone: "positive",
    },
    {
      id: "dll",
      display_name: "DLL 通道",
      state: "listening",
      state_text: "正在监听",
      ui_tone: "positive",
      listen_address: "0.0.0.0:16127",
      mtls_enabled: true,
      connected_clients: 4,
      request_count: 1128,
      error_count: 1,
      enabled: true,
      upstream_url: "https://dll.example.test",
      upstream_state_text: "等待首个请求",
      upstream_ui_tone: "info",
    },
  ],
  app_to_proxy_health: {
    state: "healthy",
    state_text: "终端连接健康",
    detail: "12 个终端连接",
    ui_tone: "positive",
  },
  proxy_to_server_health: {
    state: "healthy",
    state_text: "上游连接健康",
    detail: "最近转发成功",
    ui_tone: "positive",
  },
  active_sessions: 12,
  pending_breakpoints: 5,
  logical_memory_bytes: 128 * 1024 * 1024,
  logical_memory_text: "128 MiB",
  memory_capacity_bytes: 256 * 1024 * 1024,
  memory_capacity_text: "256 MiB",
  memory_usage_percent: 50,
  session_capacity: 500,
  default_timeout_seconds: 70,
  can_start: false,
  start_disabled_reason: {
    code: "PROXY_ALREADY_RUNNING",
    message: "Proxy 已运行。",
  },
  can_stop: true,
  stop_disabled_reason: null,
  can_restart: true,
  restart_disabled_reason: null,
  fault_reason: null,
};

describe("ConsoleView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    commandMocks.proxyStop.mockResolvedValue({
      status: "ok",
      data: { ...status, state: "stopped", state_text: "已停止" },
    });
  });

  it("renders the Rust proxy ViewModel without recomputing business state", () => {
    render(
      <ConsoleView
        status={status}
        recentCaptureLoading={false}
        onRecentCaptureRetry={vi.fn()}
        onRefresh={vi.fn()}
      />,
    );

    expect(screen.getByRole("heading", { name: "代理控制台" })).toBeVisible();
    expect(screen.getByText("交易通道")).toBeVisible();
    expect(screen.getByText("0.0.0.0:16627")).toBeVisible();
    expect(screen.getByText("12")).toBeVisible();
    expect(screen.getByText("5")).toBeVisible();
  });

  it("dispatches the generated proxyStop intent and refreshes the snapshot", async () => {
    const onRefresh = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(
      <ConsoleView
        status={status}
        recentCaptureLoading={false}
        onRecentCaptureRetry={vi.fn()}
        onRefresh={onRefresh}
      />,
    );

    await user.click(screen.getByRole("button", { name: "停止代理" }));

    expect(commandMocks.proxyStop).toHaveBeenCalledOnce();
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it("disables duplicate lifecycle requests while Rust is still working", async () => {
    let finish!: (value: unknown) => void;
    commandMocks.proxyStop.mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const user = userEvent.setup();
    render(
      <ConsoleView
        status={status}
        recentCaptureLoading={false}
        onRecentCaptureRetry={vi.fn()}
        onRefresh={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    const stop = screen.getByRole("button", { name: "停止代理" });
    await user.click(stop);
    expect(
      screen.getByRole("button", { name: "正在停止…" }),
    ).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "正在停止…" }));
    expect(commandMocks.proxyStop).toHaveBeenCalledOnce();

    finish({
      status: "ok",
      data: { ...status, state: "stopped", state_text: "已停止" },
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "停止代理" })).toBeEnabled(),
    );
  });
});
