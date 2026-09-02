// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AndroidNetworkView } from "./android-network-view";
import { testAndroidNetworkProfile } from "./android-network-test-profile";

const mocks = vi.hoisted(() => ({
  androidAdbGet: vi.fn(), androidDeviceList: vi.fn(), androidAdbSelect: vi.fn(),
  androidPackageList: vi.fn(), androidPackageRefresh: vi.fn(), androidPackageQuery: vi.fn(), deviceNetworkProfileList: vi.fn(), deviceNetworkStatus: vi.fn(),
  deviceNetworkRuntimeOwners: vi.fn(),
  deviceNetworkEndpoints: vi.fn(),
  deviceNetworkProfileNew: vi.fn(), deviceNetworkProfileGet: vi.fn(), deviceNetworkProfileApplyIntent: vi.fn(), deviceNetworkProfileSave: vi.fn(),
  androidCompanionInstall: vi.fn(), androidCompanionUpdate: vi.fn(), androidVpnOpenConsent: vi.fn(),
  deviceNetworkStart: vi.fn(), deviceNetworkApply: vi.fn(), deviceNetworkStop: vi.fn(),
  deviceNetworkEmergencyRestore: vi.fn(),
  workspaceList: vi.fn(), workspaceGet: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({ commands: mocks }));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: vi.fn(),
}));
function ok<T>(data: T) { return Promise.resolve({ status: "ok" as const, data }); }
function owner(serial = "device-1") { return { serial, epoch: "11111111-1111-4111-8111-111111111111", mode: "adb_reverse", profile_id: "profile-1", state: "active", source: "start", transition_reason: "activation_confirmed", updated_at: "2026-08-17T00:00:00Z" }; }
const profile = testAndroidNetworkProfile;
describe("Android targeted network page", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.androidAdbGet.mockReturnValue(ok({ available: true, executable: "/sdk/adb", version: "adb", selected_serial: "device-1" }));
    mocks.androidDeviceList.mockReturnValue(ok([{ serial: "device-1", runtime_epoch: "11111111-1111-4111-8111-111111111111", state: "device", product: null, model: "A920MAX", device: null, transport_id: "1", selected: true }]));
    mocks.androidPackageList.mockReturnValue(ok([{ package_name: "example.target", uid: 10001, shared_uid: null }]));
    mocks.androidPackageRefresh.mockReturnValue(ok([{ package_name: "example.target", uid: 10001, shared_uid: null }]));
    mocks.androidPackageQuery.mockReturnValue(ok([{ package_name: "example.target", uid: 10001, shared_uid: null }]));
    mocks.deviceNetworkProfileList.mockReturnValue(ok([]));
    mocks.workspaceList.mockReturnValue(ok([{ id: "workspace-1", name: "当前工作区", revision: 1, listener_count: 2, enabled_listener_count: 2, selected: true }]));
    mocks.workspaceGet.mockReturnValue(ok({
      id: "workspace-1", name: "当前工作区", revision: 1,
      listeners: [
        { id: "listener-1", name: "交易入口", bind_address: "0.0.0.0", port: 16627 },
        { id: "listener-2", name: "DLL 入口", bind_address: "0.0.0.0", port: 16127 },
      ],
      metadata_extractors: [], response_assertions: [], fault_presets: [], certificate_references: [], android_network_profiles: [],
    }));
    mocks.deviceNetworkStatus.mockReturnValue(ok({ serial: "device-1", runtime_epoch: "11111111-1111-4111-8111-111111111111", state: "stopped", state_text: "已停止", ui_tone: "neutral", verified: true, transport: "local_abstract_socket", active_profile_id: null, companion_process_running: true, message: "已停止", unsupported_fields: [], stats: null }));
    mocks.deviceNetworkRuntimeOwners.mockReturnValue(ok([]));
    mocks.deviceNetworkEndpoints.mockReturnValue(ok({
      configured_profile_id: null,
      configured: [],
      runtime_owner: null,
      runtime: [],
    }));
    mocks.deviceNetworkProfileNew.mockReturnValue(ok(profile));
    mocks.deviceNetworkProfileApplyIntent.mockImplementation((_serial, value, intent) => {
      if (intent.kind === "toggle_package") {
        return ok({
          ...value,
          target_applications: intent.selected ? [{
            package_name: intent.package_name,
            uid: 10001,
            display_name: intent.package_name,
          }] : [],
        });
      }
      if (intent.kind === "set_burst_loss_enabled") {
        return ok({
          ...value,
          weak_network: {
            ...value.weak_network,
            burst_loss: intent.enabled ? {
              enter_bad_state_basis_points: 0,
              leave_bad_state_basis_points: 0,
              good_state_loss_basis_points: 0,
              bad_state_loss_basis_points: 0,
            } : null,
          },
        });
      }
      if (intent.kind === "add_blackout_window") {
        return ok({ ...value, weak_network: { ...value.weak_network, blackout_windows: [{ start_after_millis: 0, duration_millis: 0 }] } });
      }
      return ok({ ...value, weak_network: { ...value.weak_network, nth_tcp_flag_drops: [{ direction: "upload", flag: "syn", nth: 1 }] } });
    });
    mocks.deviceNetworkProfileSave.mockImplementation((_serial, value) => ok(value));
    mocks.deviceNetworkStart.mockReturnValue(ok({ serial: "device-1", runtime_epoch: "11111111-1111-4111-8111-111111111111", state: "running", state_text: "运行中", ui_tone: "positive", verified: true, transport: "local_abstract_socket", active_profile_id: "profile-1", companion_process_running: true, message: "运行中", unsupported_fields: [], stats: null }));
  });

  it("uses compact Chinese labels for the initial operation flow", async () => {
    render(<AndroidNetworkView />);

    expect(await screen.findByRole("heading", { name: "应用网络接管" })).toBeVisible();
    expect(screen.getByText(/可将指定目标透明转交代理入口/)).toBeVisible();
    expect(screen.queryByText(/填写.*Proxy IP/i)).not.toBeInTheDocument();
    expect(screen.getByText("设备连接与控制")).toBeVisible();
    expect(screen.getByLabelText("目标设备")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "本机连接工具" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "设备端控制" })).not.toBeInTheDocument();
    expect(screen.getByText("设备网络方案")).toBeVisible();
    expect(await screen.findByRole("button", { name: "新建设备网络方案" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "新建" })).not.toBeInTheDocument();
    expect(screen.queryByText("尚未选择设备网络方案")).not.toBeInTheDocument();
    expect(screen.queryByText("Profiles")).not.toBeInTheDocument();
    expect(screen.queryByText("Companion 与 VPN")).not.toBeInTheDocument();
  });

  it("keeps one create action and only shows the selection hint when profiles exist", async () => {
    mocks.deviceNetworkProfileList.mockReturnValue(ok([{
      id: "saved-profile",
      name: "已保存方案",
      target_count: 1,
      auto_resume_after_reboot: false,
      stop_vpn_on_control_loss: true,
    }]));

    render(<AndroidNetworkView />);

    expect(await screen.findByText("尚未选择设备网络方案")).toBeVisible();
    expect(screen.getAllByRole("button", { name: "新建" })).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "新建设备网络方案" })).not.toBeInTheDocument();
  });

  it("uses a wrapping profile flow and marks the profile currently executed by VPN", async () => {
    mocks.deviceNetworkRuntimeOwners.mockReturnValue(ok([owner()]));
    mocks.deviceNetworkProfileList.mockReturnValue(ok([
      {
        id: "profile-1",
        name: "收银应用弱网",
        target_count: 1,
        auto_resume_after_reboot: false,
        stop_vpn_on_control_loss: true,
      },
      {
        id: "profile-2",
        name: "扫码应用代理",
        target_count: 2,
        auto_resume_after_reboot: false,
        stop_vpn_on_control_loss: true,
      },
    ]));
    mocks.deviceNetworkStatus.mockReturnValue(ok({
      serial: "device-1",
      runtime_epoch: "11111111-1111-4111-8111-111111111111",
      state: "running",
      state_text: "运行中",
      ui_tone: "positive",
      verified: true,
      transport: "local_abstract_socket",
      active_profile_id: "profile-1",
      companion_process_running: true,
      message: "设备网络接管正在运行。",
      unsupported_fields: [],
      stats: null,
    }));

    render(<AndroidNetworkView />);

    expect(await screen.findByText("正在执行 · 运行中")).toBeVisible();
    const profileFlow = screen.getByText("收银应用弱网").closest("div.grid");
    expect(profileFlow).toHaveClass("grid", "gap-2");
    expect(screen.getByText("扫码应用代理")).toBeVisible();
  });

  it("keeps showing a running profile that belongs to another Workspace", async () => {
    mocks.deviceNetworkRuntimeOwners.mockReturnValue(ok([owner()]));
    mocks.deviceNetworkStatus.mockReturnValue(ok({
      serial: "device-1",
      runtime_epoch: "11111111-1111-4111-8111-111111111111",
      state: "running",
      state_text: "运行中",
      ui_tone: "positive",
      verified: true,
      transport: "local_abstract_socket",
      active_profile_id: "profile-from-another-workspace",
      companion_process_running: true,
      message: "设备网络接管正在运行。",
      unsupported_fields: [],
      stats: null,
    }));

    render(<AndroidNetworkView />);

    expect(await screen.findByText("其他 Workspace 的方案正在运行")).toBeVisible();
    expect(screen.getByText(/仍使用其原 Workspace 的代理入口/)).toBeVisible();
  });

  it("waits for the previous runtime poll before scheduling the next one", async () => {
    mocks.deviceNetworkRuntimeOwners.mockReturnValue(ok([owner()]));
    vi.useFakeTimers();
    let resolvePoll: (() => void) | undefined;
    mocks.deviceNetworkStatus
      .mockReturnValueOnce(ok({ serial: "device-1", runtime_epoch: "11111111-1111-4111-8111-111111111111", state: "stopped", state_text: "已停止", ui_tone: "neutral", verified: true, transport: "local_abstract_socket", active_profile_id: null, companion_process_running: true, message: "已停止", unsupported_fields: [], stats: null }))
      .mockImplementation(() => new Promise((resolve) => {
        resolvePoll = () => resolve({ status: "ok" as const, data: { serial: "device-1", runtime_epoch: "11111111-1111-4111-8111-111111111111", state: "stopped", state_text: "已停止", ui_tone: "neutral", verified: true, transport: "local_abstract_socket", active_profile_id: null, companion_process_running: true, message: "已停止", unsupported_fields: [], stats: null } });
      }));

    const view = render(<AndroidNetworkView />);
    try {
      await act(async () => undefined);
      expect(mocks.deviceNetworkStatus).toHaveBeenCalledTimes(1);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(1_000);
      });
      expect(mocks.deviceNetworkStatus).toHaveBeenCalledTimes(2);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(5_000);
      });
      expect(mocks.deviceNetworkStatus).toHaveBeenCalledTimes(2);

      await act(async () => resolvePoll?.());
      await act(async () => {
        await vi.advanceTimersByTimeAsync(999);
      });
      expect(mocks.deviceNetworkStatus).toHaveBeenCalledTimes(2);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1);
      });
      expect(mocks.deviceNetworkStatus).toHaveBeenCalledTimes(3);
    } finally {
      view.unmount();
      vi.useRealTimers();
    }
  });

  it("selects the target device from a dropdown and delegates persistence to Rust", async () => {
    const user = userEvent.setup();
    mocks.androidDeviceList.mockReturnValue(ok([
      { serial: "device-1", runtime_epoch: "11111111-1111-4111-8111-111111111111", state: "device", product: null, model: "A920MAX", device: null, transport_id: "1", selected: true },
      { serial: "device-2", state: "device", product: null, model: "备用设备", device: null, transport_id: "2", selected: false },
    ]));

    render(<AndroidNetworkView />);
    await user.click(await screen.findByLabelText("目标设备"));
    await user.click(await screen.findByRole("option", { name: /备用设备/ }));

    await waitFor(() => expect(mocks.androidAdbSelect).toHaveBeenCalledWith("device-2"));
  });

  it("reports discovered devices before one is selected", async () => {
    mocks.androidAdbGet.mockReturnValue(ok({
      available: true,
      executable: "/sdk/adb",
      version: "adb",
      selected_serial: null,
    }));
    mocks.androidDeviceList.mockReturnValue(ok([{
      serial: "127.0.0.1:6555",
      state: "device",
      product: "motion_phone_arm64",
      model: "Phone",
      device: "motion_phone_arm64",
      transport_id: "1",
      selected: false,
    }]));

    render(<AndroidNetworkView />);

    expect(await screen.findByText("已发现 1 台在线设备，请从下拉框选择。")).toBeVisible();
    expect(screen.queryByText("没有检测到在线设备")).not.toBeInTheDocument();
  });

  it("creates a Rust-owned profile and saves selected applications through Rust", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建设备网络方案" }));
    expect(screen.queryByRole("button", { name: "选择" })).not.toBeInTheDocument();
    expect(screen.getByText(/点击应用所在整行即可选择/)).toBeVisible();
    expect(screen.queryByRole("columnheader", { name: "状态" })).not.toBeInTheDocument();
    expect(screen.queryByText("点击整行选择")).not.toBeInTheDocument();
    const packageRow = await screen.findByRole("row", { name: /example\.target/ });
    await user.click(packageRow);
    await waitFor(() => expect(mocks.deviceNetworkProfileApplyIntent).toHaveBeenCalledWith(
      "device-1",
      profile,
      { kind: "toggle_package", package_name: "example.target", selected: true },
    ));
    expect(await screen.findByLabelText("已选中")).toBeVisible();
    expect(screen.getByLabelText("已选择应用")).toBeVisible();
    expect(screen.getByText("已选择应用（1）")).toBeVisible();
    expect(screen.getByLabelText("已选择 example.target")).toBeVisible();
    expect(screen.getByRole("row", { name: /example\.target/ })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "保存方案" }));
    await waitFor(() => expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1));
    expect(mocks.deviceNetworkProfileSave.mock.calls[0][1].target_applications[0].package_name).toBe("example.target");
  });

  it("keeps auto-resume track inside the clickable HeroUI switch hit area", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建设备网络方案" }));

    const autoResume = screen.getByRole("switch", {
      name: "解锁且网络可用后自动恢复",
    });
    const content = autoResume.closest('[data-slot="switch-content"]');
    const control = content?.querySelector<HTMLElement>(
      '[data-slot="switch-control"]',
    );

    expect(autoResume).not.toBeChecked();
    expect(control).toBeTruthy();
    expect(content).toContainElement(control!);
    // jsdom 不执行 React Aria SwitchButton 的浏览器级 label 默认动作；结构断言负责
    // 保证可视轨道位于该点击区，隐藏 input 的点击则验证受控状态和保存意图。
    await user.click(autoResume);
    await waitFor(() => expect(screen.getByRole("switch", {
      name: "解锁且网络可用后自动恢复",
    })).toBeChecked());

    await user.click(screen.getByRole("button", { name: "保存方案" }));
    await waitFor(() => expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1));
    expect(mocks.deviceNetworkProfileSave.mock.calls[0][1].auto_resume_after_reboot).toBe(true);
  });

  it("defaults control-loss protection on and persists the user choice", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建设备网络方案" }));

    const protection = screen.getByRole("switch", {
      name: "ADB 或桌面控制失联 5 秒后自动关闭 VPN",
    });
    expect(protection).toBeChecked();

    await user.click(protection);
    expect(protection).not.toBeChecked();
    await user.click(screen.getByRole("button", { name: "保存方案" }));

    await waitFor(() => expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1));
    expect(mocks.deviceNetworkProfileSave.mock.calls[0][1].stop_vpn_on_control_loss).toBe(false);
  });

  it("sends package-name filters to Rust instead of filtering the inventory in the page", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建设备网络方案" }));
    await user.type(screen.getByRole("textbox", { name: "包名筛选" }), "example.target");
    await user.click(screen.getByRole("button", { name: "筛选" }));

    await waitFor(() => expect(mocks.androidPackageQuery).toHaveBeenCalledWith("device-1", "example.target"));
    expect(screen.getByText("example.target")).toBeVisible();
  });

  it("refreshes the Rust package inventory and reapplies the active package filter", async () => {
    const user = userEvent.setup();
    mocks.androidPackageQuery
      .mockReturnValueOnce(ok([]))
      .mockReturnValueOnce(ok([{
        package_name: "com.example.installed",
        uid: 10002,
        shared_uid: null,
      }]));
    mocks.androidPackageRefresh.mockReturnValue(ok([{
      package_name: "com.example.installed",
      uid: 10002,
      shared_uid: null,
    }]));

    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建设备网络方案" }));
    fireEvent.change(screen.getByRole("textbox", { name: "包名筛选" }), {
      target: { value: "com.example.installed" },
    });
    await user.click(screen.getByRole("button", { name: "筛选" }));
    expect(await screen.findByText("没有匹配该包名的应用。")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "刷新应用列表" }));

    await waitFor(() => expect(mocks.androidPackageRefresh).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(mocks.androidPackageQuery).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("com.example.installed")).toBeVisible();
  });

  it("uses a single-column card flow while keeping the full package inventory vertically scrollable", async () => {
    const user = userEvent.setup();
    mocks.androidPackageList.mockReturnValue(ok(Array.from({ length: 20 }, (_, index) => ({
      package_name: `example.target.${index}`,
      uid: 10_001 + index,
      shared_uid: null,
    }))));

    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建设备网络方案" }));

    const applicationTable = await screen.findByLabelText("安卓应用列表");
    const scrollContainer = applicationTable.closest('[data-slot="table-scroll-container"]');
    expect(scrollContainer).toHaveClass("h-80", "overflow-y-auto", "overscroll-contain");
    expect(screen.getByText("example.target.19")).toBeInTheDocument();

    const targetApplicationCard = screen.getByText("目标应用").closest('[data-slot="card"]');
    expect(targetApplicationCard).toHaveClass("border", "border-[var(--telemetry-line)]", "shadow-sm");
    expect(targetApplicationCard?.parentElement).toHaveClass("space-y-4", "overflow-auto");
    expect(targetApplicationCard?.parentElement).not.toHaveClass("grid", "grid-cols-[380px_minmax(0,1fr)]");
  });

  it("starts only after Rust saves the selected profile", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建设备网络方案" }));
    await user.click(await screen.findByRole("row", { name: /example\.target/ }));
    await user.click(screen.getByRole("button", { name: "启动" }));
    await waitFor(() => expect(mocks.deviceNetworkStart).toHaveBeenCalledWith("device-1", "profile-1", false));
    expect(mocks.deviceNetworkProfileSave.mock.invocationCallOrder[0]).toBeLessThan(mocks.deviceNetworkStart.mock.invocationCallOrder[0]);
  });

  it("collects multiple transparent proxy routes using current Workspace listeners", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建设备网络方案" }));

    expect(screen.getByText(/业务 App 仍访问原始 Server/)).toBeVisible();
    expect(screen.queryByLabelText(/Proxy IP/i)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "添加透明代理路由" }));
    await user.click(screen.getByRole("button", { name: "添加透明代理路由" }));
    expect(screen.getAllByText(/必须至少添加一个原始端口/)).toHaveLength(2);
    await user.type(screen.getByLabelText("原始目标 1"), "api.example.test");
    await user.type(screen.getByLabelText("原始目标 2"), "10.0.34.50");

    await user.click(screen.getAllByRole("button", { name: "添加端口" })[0]);
    await user.click(screen.getAllByRole("button", { name: "添加端口" })[0]);
    const firstPort = screen.getByLabelText("原始目标 1 端口 1", { selector: "input" });
    const secondPort = screen.getByLabelText("原始目标 1 端口 2", { selector: "input" });
    await user.clear(firstPort);
    await user.type(firstPort, "443");
    await user.clear(secondPort);
    await user.type(secondPort, "8443");

    await user.click(screen.getByLabelText("原始目标 2 代理入口"));
    await user.click(await screen.findByRole("option", { name: /DLL 入口/ }));
    await user.click(screen.getByRole("button", { name: "保存方案" }));

    await waitFor(() => expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1));
    expect(mocks.deviceNetworkProfileSave.mock.calls[0][1].proxy_routes).toEqual([
      { destination: "api.example.test", ports: [443, 8443], listener_id: "listener-1" },
      { destination: "10.0.34.50", ports: [], listener_id: "listener-2" },
    ]);
  }, 20_000);

  it("collects multiple weak-network coverage addresses without treating them as proxy routes", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建设备网络方案" }));

    expect(screen.getByText("弱网覆盖范围（可选）")).toBeVisible();
    expect(screen.getByText(/这里只限制哪些连接实施弱网，不改变请求去向/)).toBeVisible();

    await user.click(screen.getByRole("button", { name: "添加弱网覆盖地址" }));
    await user.click(screen.getByRole("button", { name: "添加弱网覆盖地址" }));
    fireEvent.change(screen.getByLabelText("目标地址 1"), { target: { value: "10.0.34.50" } });
    fireEvent.change(screen.getByLabelText("目标地址 2"), { target: { value: "2001:db8::/32" } });
    const firstPort = screen.getByLabelText("目标地址 1 端口", { selector: "input" });
    await user.clear(firstPort);
    await user.type(firstPort, "16127");

    await user.click(screen.getByRole("button", { name: "保存方案" }));
    await waitFor(() => expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1));
    expect(mocks.deviceNetworkProfileSave.mock.calls[0][1].destination_targets).toEqual([
      { cidr: "10.0.34.50", ports: [16127] },
      { cidr: "2001:db8::/32", ports: [] },
    ]);
  });

  it("collects advanced weak-network intent without validating it in the frontend", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建设备网络方案" }));

    await user.click(screen.getByRole("switch", { name: "启用连续丢包" }));
    await user.click(screen.getByRole("button", { name: "添加断网窗口" }));
    await user.click(screen.getByRole("button", { name: "添加 TCP 丢弃" }));

    await waitFor(() => expect(mocks.deviceNetworkProfileApplyIntent).toHaveBeenCalledTimes(3));

    const reorderHold = screen.getByLabelText("乱序保持时间（ms）");
    await user.clear(reorderHold);
    await user.type(reorderHold, "250");
    const corruptionBits = screen.getByLabelText("每包翻转位数", { selector: "input" });
    await user.clear(corruptionBits);
    await user.type(corruptionBits, "65");

    await user.click(screen.getByRole("button", { name: "保存方案" }));
    await waitFor(() => expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1));
    const savedWeak = mocks.deviceNetworkProfileSave.mock.calls[0][1].weak_network;
    expect(savedWeak.burst_loss).toEqual({
      enter_bad_state_basis_points: 0,
      leave_bad_state_basis_points: 0,
      good_state_loss_basis_points: 0,
      bad_state_loss_basis_points: 0,
    });
    expect(savedWeak.blackout_windows).toEqual([{ start_after_millis: 0, duration_millis: 0 }]);
    expect(savedWeak.nth_tcp_flag_drops).toEqual([{ direction: "upload", flag: "syn", nth: 1 }]);
    expect(savedWeak.maximum_reorder_hold_millis).toBe(250);
    // 65 超过 Rust 当前允许范围；前端仍原样提交，字段错误应由 Rust 返回。
    expect(savedWeak.corruption.bits_per_packet).toBe(65);
  }, 10_000);
});
