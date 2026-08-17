// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AndroidNetworkView } from "./android-network-view";

const mocks = vi.hoisted(() => ({
  androidAdbGet: vi.fn(),
  androidDeviceList: vi.fn(),
  androidAdbSelect: vi.fn(),
  androidPackageList: vi.fn(),
  androidPackageRefresh: vi.fn(),
  androidPackageQuery: vi.fn(),
  deviceNetworkProfileList: vi.fn(),
  deviceNetworkProfileGet: vi.fn(),
  deviceNetworkProfileNew: vi.fn(),
  deviceNetworkProfileApplyIntent: vi.fn(),
  deviceNetworkProfileSave: vi.fn(),
  deviceNetworkRuntimeOwner: vi.fn(),
  deviceNetworkStatus: vi.fn(),
  androidCompanionInstall: vi.fn(),
  androidCompanionUpdate: vi.fn(),
  androidVpnOpenConsent: vi.fn(),
  deviceNetworkStart: vi.fn(),
  deviceNetworkApply: vi.fn(),
  deviceNetworkStop: vi.fn(),
  deviceNetworkEmergencyRestore: vi.fn(),
  workspaceList: vi.fn(),
  workspaceGet: vi.fn(),
  useAppEventRefresh: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({ commands: mocks }));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: mocks.useAppEventRefresh,
}));

function ok<T>(data: T) {
  return Promise.resolve({ status: "ok" as const, data });
}

const ownerA = {
  serial: "device-a",
  epoch: "11111111-1111-4111-8111-111111111111",
  mode: "adb_reverse",
  profile_id: "profile-a",
  state: "active",
  source: "start",
  transition_reason: "activation_confirmed",
  updated_at: "2026-08-17T00:00:00Z",
};

const runningA = {
  serial: "device-a",
  state: "running",
  state_text: "运行中",
  ui_tone: "positive",
  verified: true,
  transport: "local_abstract_socket",
  active_profile_id: "profile-a",
  companion_process_running: true,
  message: "设备 A 正在运行。",
  unsupported_fields: [],
  stats: null,
};

const profile = {
  id: "profile-a",
  name: "设备 A 方案",
  target_applications: [],
  destination_targets: [],
  proxy_routes: [],
  confirmed_shared_uids: [],
  auto_resume_after_reboot: false,
  weak_network: {
    seed: 1,
    fixed_delay_millis: 0,
    uniform_jitter_millis: 0,
    upload_bytes_per_second: null,
    download_bytes_per_second: null,
    random_loss_basis_points: 0,
    burst_loss: null,
    duplicate_basis_points: 0,
    reorder_basis_points: 0,
    maximum_reorder_hold_millis: 0,
    blackout_windows: [],
    dns_blackhole: false,
    nth_tcp_flag_drops: [],
    path_mtu: { mtu: null, mss_clamp: null, mode: "pass" },
    corruption: { probability_basis_points: 0, bits_per_packet: 0 },
  },
};

describe("Android runtime owner view", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.androidAdbGet.mockReturnValue(ok({
      available: true,
      executable: "/sdk/adb",
      version: "adb",
      selected_serial: "device-b",
    }));
    mocks.androidDeviceList.mockReturnValue(ok([
      { serial: "device-a", state: "device", product: null, model: "A920MAX", device: null, transport_id: "1", selected: false },
      { serial: "device-b", state: "device", product: null, model: "A8700", device: null, transport_id: "2", selected: true },
    ]));
    mocks.androidPackageList.mockReturnValue(ok([]));
    mocks.androidPackageRefresh.mockReturnValue(ok([]));
    mocks.androidPackageQuery.mockReturnValue(ok([]));
    mocks.deviceNetworkProfileList.mockReturnValue(ok([{
      id: "profile-a",
      name: "设备 A 方案",
      target_count: 0,
      auto_resume_after_reboot: false,
    }]));
    mocks.deviceNetworkProfileGet.mockReturnValue(ok(profile));
    mocks.deviceNetworkProfileSave.mockImplementation((value) => ok(value));
    mocks.deviceNetworkRuntimeOwner.mockReturnValue(ok(ownerA));
    mocks.deviceNetworkStatus.mockReturnValue(ok(runningA));
    mocks.workspaceList.mockReturnValue(ok([{
      id: "workspace-1",
      name: "当前工作区",
      revision: 1,
      listener_count: 0,
      enabled_listener_count: 0,
      selected: true,
    }]));
    mocks.workspaceGet.mockReturnValue(ok({
      id: "workspace-1",
      name: "当前工作区",
      revision: 1,
      listeners: [],
      metadata_extractors: [],
      response_assertions: [],
      fault_presets: [],
      certificate_references: [],
      android_network_profiles: [],
    }));
    mocks.deviceNetworkStop.mockReturnValue(ok({ ...runningA, state: "stopped" }));
    mocks.deviceNetworkEmergencyRestore.mockReturnValue(ok({ ...runningA, state: "stopped" }));
  });

  it("renders owner A status while selected device is B and blocks takeover", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);

    expect(await screen.findByText("设备 A 正在运行。")).toBeVisible();
    expect(screen.getByLabelText("目标设备")).toHaveTextContent("A8700");
    expect(screen.getByLabelText("设备网络运行所有者")).toHaveTextContent("device-a");
    await user.click(screen.getByRole("button", { name: /设备 A 方案/ }));
    expect(await screen.findByRole("button", { name: "启动" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "应用修改" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "停止网络接管" })).toBeEnabled();
    expect(mocks.useAppEventRefresh).toHaveBeenCalledWith(
      ["android_vpn_status_changed"],
      expect.any(Function),
      { paused: false, entityId: "device-a" },
    );
  });

  it("does not query runtime status from selection when no owner exists", async () => {
    mocks.deviceNetworkRuntimeOwner.mockReturnValue(ok(null));
    render(<AndroidNetworkView />);

    await waitFor(() => expect(mocks.deviceNetworkRuntimeOwner).toHaveBeenCalledOnce());
    expect(mocks.deviceNetworkStatus).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "刷新运行状态" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "紧急恢复网络" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "授权网络接管" })).toBeEnabled();
  });

  it("keeps status and owner recovery available without a selection", async () => {
    const user = userEvent.setup();
    mocks.androidAdbGet.mockReturnValue(ok({
      available: true,
      executable: "/sdk/adb",
      version: "adb",
      selected_serial: null,
    }));
    render(<AndroidNetworkView />);

    expect(await screen.findByText("设备 A 正在运行。")).toBeVisible();
    expect(screen.getByRole("button", { name: "刷新运行状态" })).toBeEnabled();
    const restore = screen.getByRole("button", { name: "紧急恢复网络" });
    expect(restore).toBeEnabled();
    await user.click(restore);
    await waitFor(() => expect(mocks.deviceNetworkEmergencyRestore).toHaveBeenCalledOnce());
    await waitFor(() => expect(mocks.androidAdbGet).toHaveBeenCalledTimes(2));
    expect(mocks.deviceNetworkRuntimeOwner.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(mocks.deviceNetworkStatus).toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "授权网络接管" })).toBeDisabled();
  });

  it("drops a stale runtime response after the owner epoch changes", async () => {
    let resolveOldStatus: ((value: ReturnType<typeof statusResult>) => void) | undefined;
    const nextOwner = { ...ownerA, epoch: "22222222-2222-4222-8222-222222222222" };
    mocks.deviceNetworkRuntimeOwner
      .mockReturnValueOnce(ok(ownerA))
      .mockReturnValue(ok(nextOwner));
    mocks.deviceNetworkStatus
      .mockReturnValueOnce(new Promise((resolve) => {
        resolveOldStatus = resolve;
      }))
      .mockReturnValue(ok({ ...runningA, message: "新 epoch 的设备 A 状态" }));

    render(<AndroidNetworkView />);
    await waitFor(() => expect(mocks.deviceNetworkStatus).toHaveBeenCalledOnce());
    const refresh = mocks.useAppEventRefresh.mock.calls.at(-1)?.[1];
    await act(async () => refresh());
    expect(await screen.findByText("新 epoch 的设备 A 状态")).toBeVisible();
    await act(async () => resolveOldStatus?.(statusResult({
      ...runningA,
      message: "陈旧 epoch 的设备 A 状态",
    })));

    expect(screen.queryByText("陈旧 epoch 的设备 A 状态")).not.toBeInTheDocument();
    expect(screen.getByText("新 epoch 的设备 A 状态")).toBeVisible();
  });

  it("stops an owner outside the current Workspace without selection or draft", async () => {
    const user = userEvent.setup();
    mocks.androidAdbGet.mockReturnValue(ok({
      available: true,
      executable: "/sdk/adb",
      version: "adb",
      selected_serial: null,
    }));
    mocks.deviceNetworkRuntimeOwner
      .mockReturnValueOnce(ok({ ...ownerA, profile_id: "outside-profile" }))
      .mockReturnValue(ok(null));
    mocks.deviceNetworkStatus.mockReturnValue(ok({
      ...runningA,
      active_profile_id: "outside-profile",
    }));
    render(<AndroidNetworkView />);

    await waitFor(() => expect(mocks.deviceNetworkStatus).toHaveBeenCalledOnce());
    expect(screen.getByText("其他 Workspace 的方案正在运行")).toBeVisible();
    expect(screen.getByText("尚未选择设备网络方案")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "停止网络接管" }));
    await waitFor(() => expect(mocks.deviceNetworkStop).toHaveBeenCalledOnce());
    await waitFor(() => expect(mocks.deviceNetworkRuntimeOwner).toHaveBeenCalledTimes(2));
    expect(Math.max(...mocks.deviceNetworkStatus.mock.invocationCallOrder)).toBeLessThan(
      mocks.deviceNetworkStop.mock.invocationCallOrder[0],
    );
    expect(screen.getByRole("button", { name: "停止网络接管" })).toBeDisabled();
  });
});

function statusResult(data: typeof runningA) {
  return { status: "ok" as const, data };
}
