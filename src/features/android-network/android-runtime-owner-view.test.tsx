// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  deviceNetworkRuntimeOwners: vi.fn(),
  deviceNetworkEndpoints: vi.fn(),
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

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
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
  runtime_epoch: ownerA.epoch,
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
    mocks.deviceNetworkProfileSave.mockImplementation((_serial, value) => ok(value));
    mocks.deviceNetworkRuntimeOwners.mockReturnValue(ok([ownerA]));
    mocks.deviceNetworkEndpoints.mockImplementation((serial, profileId) => ok({
      configured_profile_id: profileId,
      configured: profileId ? [{
        profile_id: profileId,
        original_destination: "payments.example.test",
        original_ports: [443],
        listener_id: "listener-selected",
        listener_name: "当前方案入口",
        listener_bind_address: "0.0.0.0",
        listener_port: 16627,
      }] : [],
      runtime_owner: serial === ownerA.serial ? ownerA : null,
      runtime: [{
        serial: "device-a",
        epoch: ownerA.epoch,
        mode: "adb_reverse",
        original_destination: "owner.example.test",
        original_ports: [443],
        resolved_original_ips: ["203.0.113.8"],
        listener_id: "listener-owner",
        listener_name: "实际运行入口",
        desktop_listener_port: 16627,
        proxy_host: "127.0.0.1",
        proxy_port: 16627,
        resolved_at: "2026-08-18T01:02:03Z",
        health: "healthy",
      }],
    }));
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

  it("keeps owner A separate while selected device B can start independently", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);

    await waitFor(() => expect(mocks.deviceNetworkRuntimeOwners).toHaveBeenCalledOnce());
    expect(screen.queryByText("设备 A 正在运行。")).not.toBeInTheDocument();
    expect(screen.getByLabelText("目标设备")).toHaveTextContent("A8700");
    expect(screen.getByLabelText("设备网络运行所有者")).toHaveTextContent("device-a");
    expect(screen.getByLabelText("实际运行端点")).not.toHaveTextContent("device-a");
    await user.click(screen.getByRole("button", { name: /设备 A 方案/ }));
    await waitFor(() =>
      expect(mocks.deviceNetworkEndpoints).toHaveBeenCalledWith("device-b", "profile-a"),
    );
    expect(screen.getByLabelText("方案配置端点")).toHaveTextContent("当前方案入口");
    expect(await screen.findByRole("button", { name: "启动" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "应用修改" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "停止网络接管 device-a" })).toBeEnabled();
    expect(mocks.useAppEventRefresh).toHaveBeenCalledWith(
      ["android_vpn_status_changed"],
      expect.any(Function),
    );
    expect(mocks.useAppEventRefresh).toHaveBeenCalledWith(
      ["android_vpn_status_changed"],
      expect.any(Function),
      { paused: true, entityId: undefined },
    );
  });

  it("refreshes all owner cards when an unselected device emits a runtime event", async () => {
    render(<AndroidNetworkView />);
    await waitFor(() => expect(mocks.deviceNetworkRuntimeOwners).toHaveBeenCalledOnce());
    const globalRefresh = mocks.useAppEventRefresh.mock.calls.find(
      (call) => call.length === 2,
    )?.[1];

    await act(async () => globalRefresh());

    expect(mocks.deviceNetworkRuntimeOwners).toHaveBeenCalledTimes(2);
    expect(mocks.deviceNetworkStatus).not.toHaveBeenCalled();
  });

  it("keeps the selected device enabled while the one-second discovery refresh is pending", async () => {
    vi.useFakeTimers();
    const refresh = deferred<Awaited<ReturnType<typeof ok<unknown[]>>>>();
    mocks.androidDeviceList
      .mockReturnValueOnce(ok([
        { serial: "device-a", state: "device", product: null, model: "A920MAX", device: null, transport_id: "1", selected: false },
        { serial: "device-b", state: "device", product: null, model: "A8700", device: null, transport_id: "2", selected: true },
      ]))
      .mockReturnValueOnce(refresh.promise);

    const view = render(<AndroidNetworkView />);
    try {
      await act(async () => undefined);
      const deviceSelect = screen.getByLabelText("目标设备");
      expect(deviceSelect).toHaveTextContent("A8700");
      expect(deviceSelect).toBeEnabled();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(1_000);
      });

      expect(mocks.androidDeviceList).toHaveBeenCalledTimes(2);
      expect(deviceSelect).toHaveTextContent("A8700");
      expect(deviceSelect).toBeEnabled();
    } finally {
      view.unmount();
      vi.useRealTimers();
    }
  });

  it("keeps start available while a background runtime-owner refresh is pending", async () => {
    const refresh = deferred<Awaited<ReturnType<typeof ok<typeof ownerA[]>>>>();
    mocks.deviceNetworkRuntimeOwners
      .mockReturnValueOnce(ok([ownerA]))
      .mockReturnValueOnce(refresh.promise);
    render(<AndroidNetworkView />);
    await waitFor(() => expect(mocks.deviceNetworkRuntimeOwners).toHaveBeenCalledOnce());
    fireEvent.click(await screen.findByRole("button", { name: /设备 A 方案/ }));
    expect(await screen.findByRole("button", { name: "启动" })).toBeEnabled();
    const refreshOwners = mocks.useAppEventRefresh.mock.calls
      .find((call) => call.length === 2)?.[1];
    act(() => { void refreshOwners(); });
    await act(async () => undefined);
    expect(screen.getByRole("button", { name: "启动" })).toBeEnabled();
    expect(screen.queryByText("正在确认实际运行设备；确认完成前不能启动或应用方案。"))
      .not.toBeInTheDocument();
    await act(async () => refresh.resolve(await ok([ownerA])));
  });

  it("keeps the target disabled while the first device snapshot is pending", async () => {
    const initial = deferred<Awaited<ReturnType<typeof ok<unknown[]>>>>();
    mocks.androidDeviceList.mockReturnValue(initial.promise);
    const view = render(<AndroidNetworkView />);
    try {
      await act(async () => undefined);
      expect(screen.getByLabelText("目标设备")).toBeDisabled();
      expect(screen.getAllByText("正在读取设备列表…").length).toBeGreaterThan(0);
      expect(screen.queryByText(/离线运行设备/)).not.toBeInTheDocument();
    } finally {
      view.unmount();
    }
  });

  it("does not call a failed initial device discovery offline", async () => {
    mocks.androidDeviceList.mockRejectedValueOnce({ message: "ADB discovery failed", field_errors: {} });
    render(<AndroidNetworkView />);
    expect(await screen.findByText("ADB discovery failed")).toBeVisible();
    expect(screen.getByText("设备列表读取失败，当前无法确认在线状态。")).toBeVisible();
    expect(screen.getByLabelText("目标设备")).toHaveTextContent("无法确认设备状态");
    expect(screen.queryByText(/离线运行设备/)).not.toBeInTheDocument();
  });

  it("keeps the previous device snapshot selectable when a discovery refresh fails", async () => {
    vi.useFakeTimers();
    mocks.androidDeviceList
      .mockReturnValueOnce(ok([
        { serial: "device-a", state: "device", product: null, model: "A920MAX", device: null, transport_id: "1", selected: false },
        { serial: "device-b", state: "device", product: null, model: "A8700", device: null, transport_id: "2", selected: true },
      ]))
      .mockRejectedValueOnce({ message: "ADB discovery failed", field_errors: {} });

    const view = render(<AndroidNetworkView />);
    try {
      await act(async () => undefined);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1_000);
      });

      const deviceSelect = screen.getByLabelText("目标设备");
      expect(deviceSelect).toHaveTextContent("A8700");
      expect(deviceSelect).toBeEnabled();
      expect(screen.getByText("ADB discovery failed")).toBeVisible();
    } finally {
      view.unmount();
      vi.useRealTimers();
    }
  });

  it("shows a hot-plugged device and ignores an older discovery response that arrives later", async () => {
    vi.useFakeTimers();
    const olderRefresh = deferred<Awaited<ReturnType<typeof ok<unknown[]>>>>();
    const initialDevices = [
      { serial: "device-a", state: "device", product: null, model: "A920MAX", device: null, transport_id: "1", selected: false },
      { serial: "device-b", state: "device", product: null, model: "A8700", device: null, transport_id: "2", selected: true },
    ];
    const hotPluggedDevices = [
      ...initialDevices,
      { serial: "device-c", state: "device", product: null, model: "PAX C", device: null, transport_id: "3", selected: false },
    ];
    mocks.androidDeviceList
      .mockReturnValueOnce(ok(initialDevices))
      .mockReturnValueOnce(olderRefresh.promise)
      .mockReturnValueOnce(ok(hotPluggedDevices));

    const view = render(<AndroidNetworkView />);
    try {
      await act(async () => undefined);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1_000);
      });
      fireEvent.click(screen.getByRole("button", { name: "刷新设备列表" }));
      await act(async () => undefined);
      const deviceSelect = screen.getByLabelText("目标设备");
      fireEvent.click(deviceSelect);
      expect(screen.getByRole("option", { name: /PAX C/ })).toBeVisible();

      await act(async () => olderRefresh.resolve(await ok(initialDevices)));

      expect(screen.getByRole("option", { name: /PAX C/ })).toBeVisible();
      expect(deviceSelect).toHaveTextContent("A8700");
    } finally {
      view.unmount();
      vi.useRealTimers();
    }
  });

  it("does not query runtime status from selection when no owner exists", async () => {
    mocks.deviceNetworkRuntimeOwners.mockReturnValue(ok([]));
    render(<AndroidNetworkView />);

    await waitFor(() => expect(mocks.deviceNetworkRuntimeOwners).toHaveBeenCalledOnce());
    expect(mocks.deviceNetworkStatus).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /刷新运行状态/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /紧急恢复网络/ })).not.toBeInTheDocument();
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

    await waitFor(() => expect(mocks.deviceNetworkRuntimeOwners).toHaveBeenCalledOnce());
    const restore = screen.getByRole("button", { name: "紧急恢复网络 device-a" });
    expect(restore).toBeEnabled();
    await user.click(restore);
    await waitFor(() => expect(mocks.deviceNetworkEmergencyRestore).toHaveBeenCalledOnce());
    await waitFor(() => expect(mocks.androidAdbGet).toHaveBeenCalledTimes(2));
    expect(mocks.deviceNetworkRuntimeOwners.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(mocks.deviceNetworkStatus).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "授权网络接管" })).toBeDisabled();
  });

  it("drops a stale runtime response after the owner epoch changes", async () => {
    mocks.androidAdbGet.mockReturnValue(ok({
      available: true,
      executable: "/sdk/adb",
      version: "adb",
      selected_serial: "device-a",
    }));
    let resolveOldStatus: ((value: ReturnType<typeof statusResult>) => void) | undefined;
    const nextOwner = { ...ownerA, epoch: "22222222-2222-4222-8222-222222222222" };
    mocks.deviceNetworkRuntimeOwners
      .mockReturnValueOnce(ok([ownerA]))
      .mockReturnValue(ok([nextOwner]));
    mocks.deviceNetworkStatus
      .mockReturnValueOnce(new Promise((resolve) => {
        resolveOldStatus = resolve;
      }))
      .mockReturnValue(ok({ ...runningA, runtime_epoch: nextOwner.epoch, message: "新 epoch 的设备 A 状态" }));

    render(<AndroidNetworkView />);
    await waitFor(() => expect(mocks.deviceNetworkStatus).toHaveBeenCalledOnce());
    const refreshOwners = mocks.useAppEventRefresh.mock.calls.find(
      (call) => call.length === 2,
    )?.[1];
    await act(async () => refreshOwners());
    await waitFor(() => expect(mocks.deviceNetworkRuntimeOwners).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(mocks.deviceNetworkStatus).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("新 epoch 的设备 A 状态")).toBeVisible();
    await act(async () => resolveOldStatus?.(statusResult({
      ...runningA,
      message: "陈旧 epoch 的设备 A 状态",
    })));

    expect(screen.queryByText("陈旧 epoch 的设备 A 状态")).not.toBeInTheDocument();
    expect(screen.getByText("新 epoch 的设备 A 状态")).toBeVisible();
  });

  it("does not let a late stop for epoch one remove epoch two when refresh fails", async () => {
    const user = userEvent.setup();
    const nextOwner = { ...ownerA, epoch: "22222222-2222-4222-8222-222222222222" };
    const nextStatus = {
      ...runningA,
      runtime_epoch: nextOwner.epoch,
      message: "新 epoch 的设备 A 状态",
    };
    let resolveStop: ((value: ReturnType<typeof ok<typeof runningA>>) => void) | undefined;
    mocks.androidAdbGet.mockReturnValue(ok({
      available: true,
      executable: "/sdk/adb",
      version: "adb",
      selected_serial: "device-a",
    }));
    mocks.androidDeviceList.mockReturnValue(ok([
      { serial: "device-a", state: "device", product: null, model: "A920MAX", device: null, transport_id: "1", selected: true },
    ]));
    mocks.deviceNetworkRuntimeOwners
      .mockReturnValueOnce(ok([ownerA]))
      .mockReturnValue(ok([nextOwner]));
    mocks.deviceNetworkStatus
      .mockReturnValueOnce(ok(runningA))
      .mockReturnValue(ok(nextStatus));
    mocks.deviceNetworkStop.mockReturnValue(new Promise((resolve) => {
      resolveStop = resolve;
    }));
    render(<AndroidNetworkView />);
    await waitFor(() => expect(mocks.deviceNetworkRuntimeOwners).toHaveBeenCalledOnce());
    expect(await screen.findByText("设备 A 正在运行。")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "停止网络接管 device-a" }));
    await waitFor(() => expect(mocks.deviceNetworkStop).toHaveBeenCalledOnce());
    const globalRefresh = mocks.useAppEventRefresh.mock.calls.find(
      (call) => call.length === 2,
    )?.[1];
    await act(async () => globalRefresh());
    expect(await screen.findByText("新 epoch 的设备 A 状态")).toBeVisible();
    mocks.deviceNetworkRuntimeOwners.mockReturnValue(Promise.reject(new Error("refresh failed")));

    await act(async () => resolveStop?.(ok({ ...runningA, state: "stopped" })));

    expect(screen.getByRole("button", { name: "停止网络接管 device-a" })).toBeEnabled();
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
    mocks.deviceNetworkRuntimeOwners
      .mockReturnValueOnce(ok([{ ...ownerA, profile_id: "outside-profile" }]))
      .mockReturnValue(ok([]));
    render(<AndroidNetworkView />);

    await waitFor(() => expect(mocks.deviceNetworkRuntimeOwners).toHaveBeenCalledOnce());
    expect(await screen.findByText("尚未选择设备网络方案")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "停止网络接管 device-a" }));
    await waitFor(() => expect(mocks.deviceNetworkStop).toHaveBeenCalledOnce());
    await waitFor(() => expect(mocks.deviceNetworkRuntimeOwners).toHaveBeenCalledTimes(2));
    expect(mocks.deviceNetworkStop).toHaveBeenCalledWith(ownerA.serial, ownerA.epoch);
    expect(screen.queryByRole("button", { name: "停止网络接管 device-a" })).not.toBeInTheDocument();
  });
});

function statusResult(data: typeof runningA) {
  return { status: "ok" as const, data };
}
