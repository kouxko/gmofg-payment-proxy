// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ExternalPackageServiceStatusViewModel,
  SettingsDraft,
} from "@/generated/rust-types";
import {
  ExternalPackageServiceSettings,
  isExternalPackageServiceStatus,
} from "./external-package-service-settings";

const queryState = vi.hoisted(() => ({
  data: undefined as unknown,
  error: undefined as string | undefined,
  isLoading: false,
  refresh: vi.fn(async () => undefined),
}));
const eventRefreshMock = vi.hoisted(() => vi.fn());

vi.mock("@/generated/rust-types", () => ({
  commands: { externalPackageServiceStatus: vi.fn() },
}));
vi.mock("@/lib/ipc/client", () => ({ callCommand: (value: unknown) => value }));
vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: () => queryState,
}));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: eventRefreshMock,
}));

const listeningStatus: ExternalPackageServiceStatusViewModel = {
  websocket_url: "ws://127.0.0.1:8765/packages",
  fixed_path: "/packages",
  online_connection_count: 2,
  state: { state: "listening" },
  authentication_enabled: false,
};

const initialDraft = {
  external_package_service: {
    bind_address: "127.0.0.1",
    port: 8765,
    rpc_timeout_seconds: 5,
    max_in_flight: 256,
  },
} as SettingsDraft;

function Harness({
  fieldError = () => undefined,
  isDisabled = false,
}: {
  fieldError?: (field: string) => string | undefined;
  isDisabled?: boolean;
}) {
  const [draft, setDraft] = useState(initialDraft);
  return <ExternalPackageServiceSettings draft={draft} fieldError={fieldError}
    isDisabled={isDisabled} onDraftChange={setDraft} />;
}

describe("ExternalPackageServiceSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    queryState.data = listeningStatus;
    queryState.error = undefined;
    queryState.isLoading = false;
  });

  it("shows authoritative runtime state and updates every draft field", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    expect(screen.getByText("正在监听")).toBeVisible();
    expect(screen.getByText("ws://127.0.0.1:8765/packages")).toBeVisible();
    expect(screen.getByText("2 个")).toBeVisible();
    expect(screen.getByText("未启用")).toBeVisible();
    expect(eventRefreshMock).toHaveBeenCalledWith(
      ["external_package_service_status_changed", "snapshot_required"],
      queryState.refresh,
    );

    const bindAddress = screen.getByRole("textbox", { name: "监听地址" });
    await user.clear(bindAddress);
    await user.type(bindAddress, "0.0.0.0");
    expect(bindAddress).toHaveValue("0.0.0.0");

    await user.click(screen.getByRole("button", { name: "Increase 端口" }));
    await user.click(screen.getByRole("button", { name: "Increase RPC 超时（秒）" }));
    await user.click(screen.getByRole("button", { name: "Increase 最大并发 RPC" }));
    expect(screen.getByRole("textbox", { name: "端口" })).toHaveValue("8,766");
    expect(screen.getByRole("textbox", { name: "RPC 超时（秒）" })).toHaveValue("6");
    expect(screen.getByRole("textbox", { name: "最大并发 RPC" })).toHaveValue("257");
  });

  it("renders field errors and locks all editable controls", () => {
    render(<Harness isDisabled fieldError={(field) =>
      field.endsWith("bind_address") ? "必须是可绑定地址" : "必须在允许范围内"} />);

    expect(screen.getByText("必须是可绑定地址")).toBeVisible();
    expect(screen.getAllByText("必须在允许范围内")).toHaveLength(3);
    expect(screen.getByRole("textbox", { name: "监听地址" })).toBeDisabled();
    for (const name of ["端口", "RPC 超时（秒）", "最大并发 RPC"]) {
      expect(screen.getByRole("textbox", { name })).toBeDisabled();
    }
  });

  it("shows a failed service state and its stable startup error", () => {
    queryState.data = {
      ...listeningStatus,
      state: { state: "failed", error: "端口已被占用" },
      authentication_enabled: true,
    };
    render(<Harness />);

    expect(screen.getByText("启动失败")).toBeVisible();
    expect(screen.getByText("端口已被占用")).toBeVisible();
    expect(screen.getByText("已启用")).toBeVisible();
  });

  it("fails closed for malformed status and exposes a retry action", async () => {
    queryState.data = { ...listeningStatus, fixed_path: "/legacy" };
    const user = userEvent.setup();
    render(<Harness />);

    expect(screen.getByText("外部软件包服务状态数据不完整。")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "重试" }));
    expect(queryState.refresh).toHaveBeenCalledTimes(1);
  });

  it("keeps loading and transport errors explicit", () => {
    queryState.isLoading = true;
    queryState.error = "服务状态读取失败";
    render(<Harness />);

    expect(screen.getByLabelText("正在读取外部软件包服务状态")).toBeVisible();
    expect(screen.getByText("服务状态读取失败")).toBeVisible();
  });
});

describe("external package service status closed union", () => {
  it("accepts both variants and rejects malformed discriminants", () => {
    expect(isExternalPackageServiceStatus(listeningStatus)).toBe(true);
    expect(isExternalPackageServiceStatus({
      ...listeningStatus,
      state: { state: "failed", error: "bind failed" },
    })).toBe(true);
    for (const malformed of [
      null,
      [],
      { ...listeningStatus, websocket_url: "http://127.0.0.1" },
      { ...listeningStatus, online_connection_count: 1.5 },
      { ...listeningStatus, state: { state: "failed", error: "" } },
      { ...listeningStatus, state: { state: "unknown" } },
      { ...listeningStatus, state: null },
    ]) {
      expect(isExternalPackageServiceStatus(malformed)).toBe(false);
    }
  });
});
